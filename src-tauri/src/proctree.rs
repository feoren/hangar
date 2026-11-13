//! Kills a child process together with all its descendants, so no zombie agents remain.
//!
//! Windows: the process goes into a Job Object with `KILL_ON_JOB_CLOSE`. When the handle is
//! closed (session over, app exited, app crashed), Windows terminates every process in the job.
//! Unix: portable-pty starts the child as session leader; its PID is the process group.
//!
//! [`usages`] reads what a whole tree uses right now (CPU time, resident memory and the ids of its
//! processes, which `gpu.rs` matches against the GPU counters), for the
//! resources of a session (`resources.rs`). Windows asks the Job Object; Linux reads `/proc` once
//! for all trees and sums every process of the child's terminal session; other systems report
//! nothing, so the window shows nothing.

use std::time::Duration;

#[cfg(windows)]
pub use windows::ProcTree;

#[cfg(unix)]
pub use unix::ProcTree;

/// CPU time, resident memory and processes of one process tree at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeUsage {
    /// CPU time (user plus kernel) the tree has used so far. It includes processes of the tree
    /// that have already ended, so it does not shrink while the tree lives.
    pub cpu: Duration,
    /// Resident memory (working set) of the processes alive now, in bytes.
    pub memory: u64,
    /// Ids of the processes alive now.
    pub processes: Vec<u32>,
}

/// What each tree uses right now, in the order given; `None` where it cannot be read.
pub fn usages(trees: &[std::sync::Arc<ProcTree>]) -> Vec<Option<TreeUsage>> {
    #[cfg(windows)]
    {
        trees.iter().map(|tree| tree.usage().ok()).collect()
    }
    #[cfg(target_os = "linux")]
    {
        linux::usages(trees)
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        vec![None; trees.len()]
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::io;
    use std::ptr;

    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_MORE_DATA, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
        JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    use super::TreeUsage;

    /// Process ids read from the job at most; a larger tree is summed partially.
    const MAX_PROCESSES: usize = 4096;

    #[derive(Debug)]
    pub struct ProcTree {
        job: HANDLE,
    }

    // SAFETY: A job handle is a process-wide kernel handle not bound to any thread. All functions
    // used here (TerminateJobObject, CloseHandle) are thread-safe per the Win32 docs.
    unsafe impl Send for ProcTree {}
    // SAFETY: see Send; &self methods only change kernel state through the handle.
    unsafe impl Sync for ProcTree {}

    impl ProcTree {
        pub fn attach(pid: u32) -> io::Result<Self> {
            // SAFETY: Null pointers are allowed for attributes and name (anonymous job, default ACL).
            let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Self { job };

            // SAFETY: The struct is POD; an all-zero bit pattern is a valid initial value.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: Pointer and size describe exactly `info`, which lives for the whole call.
            let ok = unsafe {
                SetInformationJobObject(
                    tree.job,
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }

            // SAFETY: Plain system call; an invalid result is checked below.
            let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: Both handles are valid; `process` is closed right afterwards.
            let assigned = unsafe { AssignProcessToJobObject(tree.job, process) };
            let error = (assigned == 0).then(io::Error::last_os_error);
            // SAFETY: `process` comes from OpenProcess and is closed exactly once.
            unsafe { CloseHandle(process) };
            match error {
                Some(error) => Err(error),
                None => Ok(tree),
            }
        }

        /// CPU time from the job's accounting (it counts ended processes too), memory as the
        /// sum of the working sets of the processes in the job now.
        pub fn usage(&self) -> io::Result<TreeUsage> {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: Pointer and size describe exactly `accounting`, which outlives the call.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)
                        .cast::<c_void>(),
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            // Both totals count in units of 100 ns and are never negative.
            let ticks = u64::try_from(accounting.TotalUserTime)
                .unwrap_or(0)
                .saturating_add(u64::try_from(accounting.TotalKernelTime).unwrap_or(0));
            // Only the ids whose process is confirmed in the job count, for the memory and the
            // GPU alike.
            let (processes, sets): (Vec<u32>, Vec<u64>) = self
                .process_ids()?
                .into_iter()
                .filter_map(|pid| Some((pid, working_set(pid, self.job)?)))
                .unzip();
            Ok(TreeUsage {
                cpu: Duration::from_nanos(ticks.saturating_mul(100)),
                memory: sets.into_iter().sum(),
                processes,
            })
        }

        /// Ids of the processes in the job now.
        fn process_ids(&self) -> io::Result<Vec<u32>> {
            // The list is a header of two u32 followed by ULONG_PTR ids; a buffer of usize keeps
            // the alignment right on 32 and 64 bit.
            let header = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
                / std::mem::size_of::<usize>();
            let mut capacity = 64;
            loop {
                let mut buffer = vec![0usize; header + capacity];
                let bytes = std::mem::size_of_val(buffer.as_slice());
                // SAFETY: The buffer is `bytes` long, aligned for the struct and lives for the
                // whole call; the call writes at most that many bytes.
                let ok = unsafe {
                    QueryInformationJobObject(
                        self.job,
                        JobObjectBasicProcessIdList,
                        buffer.as_mut_ptr().cast::<c_void>(),
                        u32::try_from(bytes).unwrap_or(u32::MAX),
                        ptr::null_mut(),
                    )
                };
                // SAFETY: The buffer starts with the struct's two u32 counters, written by the
                // call (or still zero after a failure); the id list is read from `buffer` below.
                let (assigned, listed) = unsafe {
                    let list = buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
                    (
                        (*list).NumberOfAssignedProcesses as usize,
                        (*list).NumberOfProcessIdsInList as usize,
                    )
                };
                if ok == 0 {
                    let error = io::Error::last_os_error();
                    let more = error.raw_os_error() == Some(ERROR_MORE_DATA as i32);
                    if more && capacity < MAX_PROCESSES {
                        capacity = assigned.max(capacity * 2).min(MAX_PROCESSES);
                        continue;
                    }
                    return Err(error);
                }
                let count = listed.min(capacity);
                return Ok(buffer[header..header + count]
                    .iter()
                    .filter_map(|&id| u32::try_from(id).ok())
                    .collect());
            }
        }

        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: `self.job` is valid until Drop.
            if unsafe { TerminateJobObject(self.job, 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for ProcTree {
        fn drop(&mut self) {
            // SAFETY: `self.job` comes from CreateJobObjectW and is closed only here.
            // Because of KILL_ON_JOB_CLOSE, Windows terminates every process still running in the job.
            unsafe { CloseHandle(self.job) };
        }
    }

    /// Working set of one process in bytes; `None` when it has ended, refuses the query or is no
    /// longer in `job` (its id was listed, then the process ended and the id went to another one).
    fn working_set(pid: u32, job: HANDLE) -> Option<u64> {
        // SAFETY: Plain system call; a null handle is checked below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut in_job = 0;
        // SAFETY: Both handles are valid for the call; `in_job` is a valid out pointer.
        let asked = unsafe { IsProcessInJob(process, job, &mut in_job) };
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..PROCESS_MEMORY_COUNTERS::default()
        };
        let ok = if asked != 0 && in_job != 0 {
            // SAFETY: `process` is valid; pointer and size describe exactly `counters`.
            unsafe { K32GetProcessMemoryInfo(process, &mut counters, counters.cb) }
        } else {
            0
        };
        // SAFETY: `process` comes from OpenProcess and is closed exactly once.
        unsafe { CloseHandle(process) };
        (ok != 0).then_some(counters.WorkingSetSize as u64)
    }
}

#[cfg(unix)]
mod unix {
    use std::io;

    #[derive(Debug)]
    pub struct ProcTree {
        group: libc::pid_t,
    }

    impl ProcTree {
        /// The child's session and process group id (it is the session leader).
        #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
        pub(super) fn group(&self) -> libc::pid_t {
            self.group
        }

        pub fn attach(pid: u32) -> io::Result<Self> {
            let group = libc::pid_t::try_from(pid)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            Ok(Self { group })
        }

        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: killpg is a plain system call without memory access.
            if unsafe { libc::killpg(self.group, libc::SIGHUP) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

/// Linux: one pass over `/proc` sums every process by its terminal session. The session, not the
/// process group, because an interactive shell puts each job into a group of its own.
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::{stat, ProcTree, TreeUsage};

    pub fn usages(trees: &[std::sync::Arc<ProcTree>]) -> Vec<Option<TreeUsage>> {
        // SAFETY: sysconf only reads system configuration.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        // SAFETY: as above.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let (Ok(ticks), Ok(page)) = (u64::try_from(ticks), u64::try_from(page)) else {
            return vec![None; trees.len()];
        };
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return vec![None; trees.len()];
        };
        if ticks == 0 {
            return vec![None; trees.len()];
        }
        let wanted: HashMap<i64, usize> = trees
            .iter()
            .enumerate()
            .map(|(index, tree)| (i64::from(tree.group()), index))
            .collect();
        let mut sums: Vec<Option<(u64, u64, Vec<u32>)>> = vec![None; trees.len()];
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            // A process may end between listing and reading; it is simply not counted.
            let Ok(text) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            let Some(fields) = stat::parse(&text) else {
                continue;
            };
            if let Some(&index) = wanted.get(&fields.session) {
                let sum = sums[index].get_or_insert((0, 0, Vec::new()));
                sum.0 = sum.0.saturating_add(fields.cpu_ticks);
                sum.1 = sum.1.saturating_add(fields.rss_pages);
                sum.2.push(pid);
            }
        }
        sums.into_iter()
            .map(|sum| {
                sum.map(|(cpu, rss, processes)| TreeUsage {
                    cpu: Duration::from_nanos(cpu.saturating_mul(1_000_000_000 / ticks)),
                    memory: rss.saturating_mul(page),
                    processes,
                })
            })
            .collect()
    }
}

/// The fields of `/proc/<pid>/stat` the resources need. Plain text parsing, so it is tested on
/// every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod stat {
    #[derive(Debug, PartialEq, Eq)]
    pub struct Fields {
        pub session: i64,
        /// utime + stime + cutime + cstime: a child's times move to its parent once it is waited
        /// for, so a tree's total does not drop when one of its processes ends.
        pub cpu_ticks: u64,
        pub rss_pages: u64,
    }

    /// `None` for a line of another shape. The command name in parentheses may contain spaces
    /// and parentheses itself, so the fields start after the last `)`.
    pub fn parse(line: &str) -> Option<Fields> {
        let rest = &line[line.rfind(')')? + 1..];
        // Field 3 (state) is the first one after the name; field n sits at index n - 3.
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let number = |field: usize| -> Option<i64> { fields.get(field - 3)?.parse().ok() };
        let ticks = |field: usize| -> Option<u64> { u64::try_from(number(field)?).ok() };
        Some(Fields {
            session: number(6)?,
            cpu_ticks: ticks(14)?
                .saturating_add(ticks(15)?)
                .saturating_add(ticks(16)?)
                .saturating_add(ticks(17)?),
            rss_pages: ticks(24)?,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reads_session_times_and_resident_pages() {
            let line = "4242 (my (odd) prog) S 1 4242 4242 34816 4242 4194560 1200 0 0 0 \
                        150 50 10 5 20 0 3 0 12345 104857600 2560 18446744073709551615";
            assert_eq!(
                parse(line),
                Some(Fields {
                    session: 4242,
                    cpu_ticks: 215,
                    rss_pages: 2560,
                })
            );
        }

        #[test]
        fn rejects_lines_of_another_shape() {
            assert_eq!(parse(""), None);
            assert_eq!(parse("12 (short) S 1 2"), None);
            assert_eq!(
                parse("12 (neg) S 1 2 3 4 5 6 7 8 9 10 -1 0 0 0 0 0 0 0 0 0 0"),
                None
            );
        }
    }
}
