//! Kills a child process together with all its descendants, so no zombie agents remain.
//!
//! Windows: the process goes into a Job Object with `KILL_ON_JOB_CLOSE`. When the handle is
//! closed (session over, app exited, app crashed), Windows terminates every process in the job.
//! Unix: portable-pty starts the child as session leader; its PID is the process group.

#[cfg(windows)]
pub use windows::ProcTree;

#[cfg(unix)]
pub use unix::ProcTree;

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::io;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

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
}

#[cfg(unix)]
mod unix {
    use std::io;

    #[derive(Debug)]
    pub struct ProcTree {
        group: libc::pid_t,
    }

    impl ProcTree {
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
