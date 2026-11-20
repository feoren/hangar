//! Kills a child process together with all its descendants, so no zombie agents remain.
//!
//! Windows: the process goes into a Job Object with `KILL_ON_JOB_CLOSE`. When the handle is
//! closed (session over, app exited, app crashed), Windows terminates every process in the job.
//! Unix: portable-pty starts the child as session leader; its PID is the process group.
//!
//! [`usages`] reads what a whole tree uses right now (CPU time, resident memory and the ids of its
//! processes, which `gpu.rs` matches against the GPU counters), for the
//! resources of a session (`resources.rs`). Windows asks the Job Object; Linux reads `/proc` once
//! for all trees and sums every process of the child's terminal session; macOS lists every
//! process once through libproc and sums the child's session and descendants ([`darwin`]); other
//! systems report nothing, so the window shows nothing.

use std::time::Duration;

/// Unix: how long a program gets after SIGHUP before the rest of its tree gets SIGKILL
/// ([`ProcTree::terminate_tree`]).
#[cfg_attr(windows, allow(dead_code))]
pub const KILL_GRACE: Duration = Duration::from_secs(2);

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
    #[cfg(target_os = "macos")]
    {
        mac::usages(trees)
    }
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
    {
        vec![None; trees.len()]
    }
}

/// Ends every tree when the app quits, before it returns. Windows: each Job Object ends at once.
/// Unix: SIGHUP to all trees, then at most [`KILL_GRACE`] in all for them to end, then SIGKILL
/// to whatever is left, synchronously. [`ProcTree::terminate_tree`]'s grace runs on a thread of
/// its own, which would die with the process before it sends SIGKILL.
pub fn end_all(trees: &[std::sync::Arc<ProcTree>]) {
    #[cfg(windows)]
    for tree in trees {
        let _ = tree.terminate();
    }
    #[cfg(unix)]
    {
        const POLL: Duration = Duration::from_millis(50);
        for tree in trees {
            let _ = tree.hang_up();
        }
        let deadline = std::time::Instant::now() + KILL_GRACE;
        while trees.iter().any(|tree| tree.alive()) && std::time::Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        for tree in trees {
            tree.kill_rest();
        }
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

        /// Nothing to do: the job handle names the tree, never a process id that could be
        /// reused once the child was waited for.
        pub fn mark_reaped(&self) {}

        /// The job ends every process at once; there is no grace to wait for.
        pub fn terminate_tree(self: &std::sync::Arc<Self>) -> io::Result<()> {
            self.terminate()
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
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;

    use super::KILL_GRACE;

    #[derive(Debug)]
    pub struct ProcTree {
        group: libc::pid_t,
        /// The child (the group's leader) was waited for. Its id may belong to another process
        /// group once the last member is gone, so the group is no longer signalled.
        reaped: AtomicBool,
        /// Linux: when the child was reaped, in clock ticks since boot (the unit of
        /// `/proc/<pid>/stat` field 22); 0 while it was not, or when the clock could not be read.
        #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
        reaped_at: AtomicU64,
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
            // Group 0 or 1 would make killpg reach the caller's own group or every process.
            if group <= 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a child process id",
                ));
            }
            Ok(Self {
                group,
                reaped: AtomicBool::new(false),
                reaped_at: AtomicU64::new(0),
            })
        }

        /// Called by whoever waited for the child, right after the wait returned.
        pub fn mark_reaped(&self) {
            #[cfg(target_os = "linux")]
            self.reaped_at
                .store(super::linux::now_ticks().unwrap_or(0), Ordering::SeqCst);
            self.reaped.store(true, Ordering::SeqCst);
        }

        /// SIGHUP to the group, like a terminal that hangs up. Nothing once the child was
        /// reaped ([`Self::mark_reaped`]).
        pub fn terminate(&self) -> io::Result<()> {
            self.signal(libc::SIGHUP)
        }

        /// [`Self::terminate`], and on Linux SIGHUP to every process still in the child's
        /// terminal session (an interactive shell puts each job into a group of its own).
        pub fn hang_up(&self) -> io::Result<()> {
            let result = self.terminate();
            #[cfg(target_os = "linux")]
            self.signal_session(libc::SIGHUP);
            result
        }

        /// SIGKILL to whatever is left: the group while its leader was not reaped, and on Linux
        /// every process of the child's terminal session (a program may ignore SIGHUP).
        pub fn kill_rest(&self) {
            let _ = self.signal(libc::SIGKILL);
            #[cfg(target_os = "linux")]
            self.signal_session(libc::SIGKILL);
        }

        /// Whether a process of the tree may still run: the group while its leader was not
        /// reaped, on Linux also a live (not zombie) process of the child's session.
        pub fn alive(&self) -> bool {
            let group = !self.reaped.load(Ordering::SeqCst)
                // SAFETY: killpg with signal 0 only checks; no memory is accessed.
                && (unsafe { libc::killpg(self.group, 0) } == 0
                    || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM));
            #[cfg(target_os = "linux")]
            let session = self.signal_session(0) > 0;
            #[cfg(not(target_os = "linux"))]
            let session = false;
            group || session
        }

        /// [`Self::hang_up`], then after [`KILL_GRACE`] [`Self::kill_rest`]. The grace runs on a
        /// thread of its own; the caller does not wait. Only for callers that report the reaping
        /// through [`Self::mark_reaped`]. At the app's end [`super::end_all`] waits itself: a
        /// grace thread would not outlive the process.
        pub fn terminate_tree(self: &Arc<Self>) -> io::Result<()> {
            let result = self.hang_up();
            let tree = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name(format!("proctree-kill-{}", self.group))
                .spawn(move || {
                    std::thread::sleep(KILL_GRACE);
                    tree.kill_rest();
                });
            if let Err(error) = spawned {
                eprintln!("hangar: no SIGKILL after the grace period: {error}");
            }
            result
        }

        /// Linux: `signal` to the child's session, with the reaping time so a session id that
        /// may have been taken over is left alone ([`super::linux::signal_session`]). Returns
        /// how many processes got it.
        #[cfg(target_os = "linux")]
        fn signal_session(&self, signal: libc::c_int) -> usize {
            let reaped_at = self.reaped.load(Ordering::SeqCst).then(|| {
                // An unknown reaping time counts as "now": strictly older members still pass.
                match self.reaped_at.load(Ordering::SeqCst) {
                    0 => super::linux::now_ticks().unwrap_or(0),
                    at => at,
                }
            });
            super::linux::signal_session(self.group, signal, reaped_at)
        }

        fn signal(&self, signal: libc::c_int) -> io::Result<()> {
            // A small race stays between the wait in another thread and its mark_reaped; the
            // id is only reused after every member of the group is gone as well.
            if self.reaped.load(Ordering::SeqCst) {
                return Ok(());
            }
            // SAFETY: killpg is a plain system call without memory access.
            if unsafe { libc::killpg(self.group, signal) } != 0 {
                let error = io::Error::last_os_error();
                // The group is already gone: nothing left to end.
                if error.raw_os_error() == Some(libc::ESRCH) {
                    return Ok(());
                }
                return Err(error);
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::process::CommandExt;
        use std::process::Command;
        use std::time::{Duration, Instant};

        fn ends_within(child: &mut std::process::Child, limit: Duration) -> bool {
            let deadline = Instant::now() + limit;
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        }

        #[test]
        fn a_reaped_child_is_never_signalled_again() {
            let mut child = Command::new("sleep")
                .arg("0")
                .process_group(0)
                .spawn()
                .unwrap();
            let tree = ProcTree::attach(child.id()).unwrap();
            child.wait().unwrap();
            tree.mark_reaped();
            assert!(tree.terminate().is_ok());
            assert!(!tree.alive());
            assert!(Arc::new(tree).terminate_tree().is_ok());
        }

        #[test]
        fn a_program_that_ignores_sighup_is_killed_after_the_grace() {
            let mut child = Command::new("sh")
                .args(["-c", "trap '' HUP; sleep 1000 & wait"])
                .process_group(0)
                .spawn()
                .unwrap();
            let tree = Arc::new(ProcTree::attach(child.id()).unwrap());
            std::thread::sleep(Duration::from_millis(300));
            tree.terminate_tree().unwrap();
            assert!(
                !ends_within(&mut child, Duration::from_millis(500)),
                "SIGHUP alone ended a program that ignores it"
            );
            assert!(
                ends_within(&mut child, KILL_GRACE + Duration::from_secs(5)),
                "still running after SIGKILL"
            );
        }

        /// A child in a group of its own whose waiter reports the reaping, as the session
        /// manager's wait thread does.
        fn watched(script: &str) -> (Arc<ProcTree>, std::sync::mpsc::Receiver<()>) {
            let mut child = Command::new("sh")
                .args(["-c", script])
                .process_group(0)
                .spawn()
                .unwrap();
            let tree = Arc::new(ProcTree::attach(child.id()).unwrap());
            let (tx, rx) = std::sync::mpsc::channel();
            let waiter = Arc::clone(&tree);
            std::thread::spawn(move || {
                let _ = child.wait();
                waiter.mark_reaped();
                let _ = tx.send(());
            });
            (tree, rx)
        }

        /// The app's end: every tree gets SIGHUP, the rest SIGKILL before `end_all` returns, and
        /// the grace is waited for once for all trees, not per tree.
        #[test]
        fn shutdown_kills_every_stubborn_tree_before_it_returns() {
            let stubborn = "trap '' HUP; sleep 1000 & wait";
            let trees: Vec<_> = (0..3).map(|_| watched(stubborn)).collect();
            std::thread::sleep(Duration::from_millis(300));
            let started = Instant::now();
            super::super::end_all(
                &trees
                    .iter()
                    .map(|(tree, _)| Arc::clone(tree))
                    .collect::<Vec<_>>(),
            );
            let took = started.elapsed();
            assert!(took >= KILL_GRACE, "returned before the grace: {took:?}");
            assert!(
                took < KILL_GRACE + Duration::from_secs(1),
                "waited per tree: {took:?}"
            );
            for (_, ended) in &trees {
                assert!(
                    ended.recv_timeout(Duration::from_secs(2)).is_ok(),
                    "a tree outlived the shutdown"
                );
            }
        }

        #[test]
        fn shutdown_does_not_wait_for_trees_that_end_on_sighup() {
            let trees: Vec<_> = (0..2).map(|_| watched("sleep 1000")).collect();
            std::thread::sleep(Duration::from_millis(200));
            let started = Instant::now();
            super::super::end_all(
                &trees
                    .iter()
                    .map(|(tree, _)| Arc::clone(tree))
                    .collect::<Vec<_>>(),
            );
            assert!(started.elapsed() < KILL_GRACE, "{:?}", started.elapsed());
        }

        #[test]
        fn process_ids_zero_and_one_are_refused() {
            assert!(ProcTree::attach(0).is_err());
            assert!(ProcTree::attach(1).is_err());
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

    /// Now, in clock ticks since boot: the clock and unit of `/proc/<pid>/stat` field 22
    /// (`start_boottime`).
    pub fn now_ticks() -> Option<u64> {
        // SAFETY: sysconf only reads system configuration.
        let ticks = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid, writable timespec for the duration of the call.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut now) } != 0 {
            return None;
        }
        let secs = u64::try_from(now.tv_sec).ok()?;
        let nanos = u64::try_from(now.tv_nsec).ok()?;
        Some(
            secs.saturating_mul(ticks)
                .saturating_add(nanos.saturating_mul(ticks) / 1_000_000_000),
        )
    }

    /// Sends `signal` (0: only counts) to every live process whose terminal session is
    /// `session`, the child's own (it is the session leader, so its id names the session).
    /// Returns how many there were.
    ///
    /// A session id is not reused while a process of that session lives. Once the child was
    /// reaped (`reaped_at`, ticks since boot) and every member had ended, a new session could
    /// carry the same id; its processes all started after the reaping. So after the reaping the
    /// session is signalled only while one of its members started before it, which proves the
    /// session is still ours ([`super::stat::still_ours`]).
    pub fn signal_session(
        session: libc::pid_t,
        signal: libc::c_int,
        reaped_at: Option<u64>,
    ) -> usize {
        // SAFETY: getpid only reads the caller's own id.
        let own = unsafe { libc::getpid() };
        if session <= 1 || session == own {
            return 0;
        }
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return 0;
        };
        let mut members = Vec::new();
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<libc::pid_t>() else {
                continue;
            };
            if pid <= 1 || pid == own {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            if let Some(fields) = stat::parse(&text) {
                if fields.session == i64::from(session) && !fields.zombie {
                    members.push((pid, fields.start_ticks));
                }
            }
        }
        if !stat::still_ours(reaped_at, members.iter().map(|(_, start)| *start)) {
            return 0;
        }
        for (pid, _) in &members {
            // SAFETY: kill is a plain system call without memory access; a process that ended
            // meanwhile only yields ESRCH.
            unsafe { libc::kill(*pid, signal) };
        }
        members.len()
    }

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

    #[cfg(test)]
    mod tests {
        use std::process::Command;
        use std::time::{Duration, Instant};

        use super::*;

        /// A session leader of its own (`setsid` execs directly: its child is no group leader).
        fn session_leader() -> std::process::Child {
            Command::new("setsid")
                .arg("sleep")
                .arg("1000")
                .spawn()
                .unwrap()
        }

        fn start_of(pid: u32) -> u64 {
            let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
            stat::parse(&text).unwrap().start_ticks
        }

        /// After the reaping, a session whose members all started later may be someone else's:
        /// it gets nothing. A member older than the reaping proves it is still ours.
        #[test]
        fn a_session_id_that_may_be_reused_is_left_alone() {
            let mut child = session_leader();
            let pid = libc::pid_t::try_from(child.id()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while signal_session(pid, 0, None) == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            let start = start_of(child.id());
            // "Reaped" before this process started: it would belong to a reused session id.
            assert_eq!(signal_session(pid, libc::SIGKILL, Some(start)), 0);
            assert!(
                child.try_wait().unwrap().is_none(),
                "a foreign session was signalled"
            );
            // Reaped after it started: still ours.
            assert_eq!(signal_session(pid, libc::SIGKILL, Some(start + 1)), 1);
            let deadline = Instant::now() + Duration::from_secs(5);
            while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                child.try_wait().unwrap().is_some(),
                "SIGKILL did not arrive"
            );
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::time::Duration;

    use super::darwin::{self, Process};
    use super::{ProcTree, TreeUsage};

    /// Every process id; `None` when libproc does not answer.
    fn all_pids() -> Option<Vec<libc::pid_t>> {
        // SAFETY: A null buffer asks only for the number of processes.
        let estimate = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let estimate = usize::try_from(estimate).ok()?;
        // Room for processes started between the two calls.
        let mut pids: Vec<libc::pid_t> = vec![0; estimate + 64];
        let bytes = i32::try_from(pids.len() * std::mem::size_of::<libc::pid_t>()).ok()?;
        // SAFETY: `pids` holds `bytes` writable bytes; libproc writes at most that many.
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).ok()?.min(pids.len());
        pids.truncate(count);
        pids.retain(|pid| *pid > 0);
        Some(pids)
    }

    fn process(pid: libc::pid_t) -> Option<Process> {
        // SAFETY: An all-zero proc_bsdinfo is a valid value of the plain struct.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: `info` is a live proc_bsdinfo of `size` bytes, the flavor's buffer type.
        let written = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        if written != size {
            return None;
        }
        // SAFETY: getsid only reads the process table.
        let session = unsafe { libc::getsid(pid) };
        Some(Process {
            pid: u32::try_from(pid).ok()?,
            parent: info.pbi_ppid,
            session: u32::try_from(session).ok(),
        })
    }

    /// CPU time (with the reaped children's) in Mach time units and resident bytes.
    fn rusage(pid: u32) -> Option<(u64, u64)> {
        // SAFETY: An all-zero rusage_info_v2 is a valid value of the plain struct.
        let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is a live rusage_info_v2, the buffer RUSAGE_INFO_V2 fills.
        let status = unsafe {
            libc::proc_pid_rusage(
                libc::c_int::try_from(pid).ok()?,
                libc::RUSAGE_INFO_V2,
                (&raw mut info).cast(),
            )
        };
        (status == 0).then(|| {
            let cpu = info
                .ri_user_time
                .saturating_add(info.ri_system_time)
                .saturating_add(info.ri_child_user_time)
                .saturating_add(info.ri_child_system_time);
            (cpu, info.ri_resident_size)
        })
    }

    /// The factor from Mach time units to nanoseconds (1 on Intel, 125/3 on Apple silicon).
    #[allow(deprecated)] // libc points to the mach2 crate; this one binding is enough.
    fn timebase() -> (u32, u32) {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: `info` is a live mach_timebase_info the call fills in.
        let status = unsafe { libc::mach_timebase_info(&mut info) };
        if status == libc::KERN_SUCCESS && info.denom != 0 {
            (info.numer, info.denom)
        } else {
            (1, 1)
        }
    }

    pub fn usages(trees: &[std::sync::Arc<ProcTree>]) -> Vec<Option<TreeUsage>> {
        let Some(pids) = all_pids() else {
            return vec![None; trees.len()];
        };
        let processes: Vec<Process> = pids.into_iter().filter_map(process).collect();
        let (numer, denom) = timebase();
        trees
            .iter()
            .map(|tree| {
                let group = u32::try_from(tree.group()).ok()?;
                let members = darwin::members(&processes, group);
                if members.is_empty() {
                    return None;
                }
                let (cpu, memory) = members
                    .iter()
                    .filter_map(|pid| rusage(*pid))
                    .fold((0u64, 0u64), |(cpu, memory), (c, m)| {
                        (cpu.saturating_add(c), memory.saturating_add(m))
                    });
                Some(TreeUsage {
                    cpu: Duration::from_nanos(darwin::nanos(cpu, numer, denom)),
                    memory,
                    processes: members,
                })
            })
            .collect()
    }
}

/// Which processes make up a tree on macOS, and its time unit, as plain logic tested on every
/// platform.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod darwin {
    use std::collections::{HashMap, HashSet};

    /// One process of the process table.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Process {
        pub pid: u32,
        pub parent: u32,
        /// `None` where the session could not be read.
        pub session: Option<u32>,
    }

    /// The processes of the tree whose child is `group`: every process of the child's terminal
    /// session (it is the session leader), as on Linux, and every descendant of the child, which
    /// also finds a program that started a session of its own. Sorted, each once.
    pub fn members(processes: &[Process], group: u32) -> Vec<u32> {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for process in processes {
            children
                .entry(process.parent)
                .or_default()
                .push(process.pid);
        }
        let mut found: HashSet<u32> = processes
            .iter()
            .filter(|process| process.pid == group || process.session == Some(group))
            .map(|process| process.pid)
            .collect();
        let mut queue: Vec<u32> = found.iter().copied().collect();
        while let Some(pid) = queue.pop() {
            for child in children.get(&pid).into_iter().flatten() {
                if found.insert(*child) {
                    queue.push(*child);
                }
            }
        }
        let mut members: Vec<u32> = found.into_iter().collect();
        members.sort_unstable();
        members
    }

    /// Mach time units to nanoseconds with the timebase `numer / denom`, without overflow.
    pub fn nanos(ticks: u64, numer: u32, denom: u32) -> u64 {
        if denom == 0 {
            return ticks;
        }
        let nanos = u128::from(ticks) * u128::from(numer) / u128::from(denom);
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn process(pid: u32, parent: u32, session: Option<u32>) -> Process {
            Process {
                pid,
                parent,
                session,
            }
        }

        #[test]
        fn a_tree_is_the_childs_session_and_descendants() {
            let processes = [
                process(1, 0, Some(1)),
                process(100, 1, Some(100)),   // Hangar
                process(200, 100, Some(200)), // the session's child
                process(201, 200, Some(200)), // its agent
                process(202, 201, Some(300)), // a helper with a session of its own
                process(203, 1, Some(200)),   // reparented, still in the session
                process(300, 1, Some(300)),   // another program
                process(301, 300, None),
            ];
            assert_eq!(members(&processes, 200), [200, 201, 202, 203]);
            assert!(members(&processes, 999).is_empty());
        }

        #[test]
        fn a_tree_without_a_readable_session_still_has_its_descendants() {
            let processes = [process(200, 100, None), process(201, 200, None)];
            assert_eq!(members(&processes, 200), [200, 201]);
        }

        #[test]
        fn mach_time_turns_into_nanoseconds() {
            assert_eq!(nanos(1_000, 1, 1), 1_000);
            // Apple silicon: 24 MHz ticks.
            assert_eq!(nanos(24_000_000, 125, 3), 1_000_000_000);
            assert_eq!(nanos(u64::MAX, 125, 3), u64::MAX);
            assert_eq!(nanos(7, 1, 0), 7);
        }
    }
}

/// The fields of `/proc/<pid>/stat` the resources, the session signals and the GPU reading
/// (`gpu.rs`) need. Plain text parsing, so it is tested on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod stat {
    #[derive(Debug, PartialEq, Eq)]
    pub struct Fields {
        /// Field 4: the parent process.
        pub parent: i64,
        pub session: i64,
        /// utime + stime + cutime + cstime: a child's times move to its parent once it is waited
        /// for, so a tree's total does not drop when one of its processes ends.
        pub cpu_ticks: u64,
        pub rss_pages: u64,
        /// Field 22: when the process started, in clock ticks since boot.
        pub start_ticks: u64,
        /// State `Z`: ended, waiting for its parent; it runs nothing any more.
        pub zombie: bool,
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
            parent: number(4)?,
            session: number(6)?,
            cpu_ticks: ticks(14)?
                .saturating_add(ticks(15)?)
                .saturating_add(ticks(16)?)
                .saturating_add(ticks(17)?),
            rss_pages: ticks(24)?,
            start_ticks: ticks(22)?,
            zombie: *fields.first()? == "Z",
        })
    }

    /// Whether a session with members that started at `starts` is still the one whose leader
    /// was reaped at `reaped_at` (`None`: not reaped yet, the leader itself holds the id). After
    /// the reaping only a member that started strictly before it proves that; a member of the
    /// same clock tick does not, which errs on leaving a process alone.
    pub fn still_ours(reaped_at: Option<u64>, mut starts: impl Iterator<Item = u64>) -> bool {
        match reaped_at {
            None => true,
            Some(reaped) => starts.any(|start| start < reaped),
        }
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
                    parent: 1,
                    session: 4242,
                    cpu_ticks: 215,
                    rss_pages: 2560,
                    start_ticks: 12345,
                    zombie: false,
                })
            );
            let zombie = line.replacen(") S ", ") Z ", 1);
            assert!(parse(&zombie).unwrap().zombie);
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

        #[test]
        fn a_reaped_session_counts_only_with_an_older_member() {
            assert!(still_ours(None, std::iter::empty()));
            assert!(still_ours(None, [500].into_iter()));
            assert!(still_ours(Some(100), [150, 99].into_iter()));
            assert!(!still_ours(Some(100), [100, 150].into_iter()));
            assert!(!still_ours(Some(100), std::iter::empty()));
        }
    }
}
