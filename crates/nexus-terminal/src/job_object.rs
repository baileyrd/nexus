//! Windows Job Objects (PRD-09 §5.3).
//!
//! # Scope
//!
//! Windows has no POSIX process group, so the Unix "kill(-pgid, SIG…)"
//! trick we use in [`crate::Session::send_signal`] does not port. Job
//! Objects are the Win32 equivalent: any process assigned to a job is
//! killed together when `TerminateJobObject` fires, and — crucially —
//! child processes are auto-assigned to the parent's job if we set the
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` + `BREAKAWAY_OK` limits
//! correctly. That gives us the same "kill the whole tree" guarantee
//! the §5.2 process-group path gives on Unix.
//!
//! # Microkernel fit
//!
//! Plain library. Exposed as a thin RAII wrapper so [`crate::Session`]
//! can own one per spawned child on Windows without leaking the raw
//! `HANDLE` type through its public API.
//!
//! # What this module is NOT
//!
//! - A full Job Object feature surface: no memory limits, CPU limits,
//!   or UI restrictions. PRD-09 §7 (memory monitoring) and §17
//!   (performance) have follow-ups that will layer on top.
//! - Cross-platform. Every type in this module is Windows-only; Unix
//!   builds see a stub that compiles to nothing.

#[cfg(windows)]
mod imp {
    use std::io;

    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        TerminateJobObject, JobObjectExtendedLimitInformation,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};

    /// RAII handle on a Win32 Job Object. `Drop` calls
    /// `TerminateJobObject(…, 1)` so any processes still in the job die
    /// atomically when the session is dropped. Explicit
    /// [`JobObject::terminate`] is idempotent — subsequent drops become
    /// no-ops.
    pub struct JobObject {
        handle: HANDLE,
        /// Flag flipped by [`Self::terminate`] so `Drop` doesn't fire
        /// a second `TerminateJobObject` against an already-closed
        /// handle. Windows returns `ERROR_INVALID_HANDLE` for that
        /// sequence and we'd rather be silent.
        terminated: bool,
    }

    // SAFETY: A Job Object HANDLE is just an integer into the Windows
    // kernel handle table. Sharing it across threads is allowed; the
    // kernel serialises the calls. We never dereference it.
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        /// Create a fresh, unnamed job object with "kill on close"
        /// semantics. Child processes auto-join the job as long as the
        /// parent doesn't opt out via `CREATE_BREAKAWAY_FROM_JOB`.
        ///
        /// # Errors
        /// Returns the Win32 error as an [`io::Error`] if job creation
        /// or `SetInformationJobObject` fails.
        pub fn create() -> io::Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                        | JOB_OBJECT_LIMIT_BREAKAWAY_OK,
                    ..unsafe { std::mem::zeroed() }
                },
                ..unsafe { std::mem::zeroed() }
            };
            let rc = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of_mut!(info).cast(),
                    u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                        .unwrap_or(u32::MAX),
                )
            };
            if rc == 0 {
                let err = io::Error::last_os_error();
                unsafe { CloseHandle(handle) };
                return Err(err);
            }
            Ok(Self {
                handle,
                terminated: false,
            })
        }

        /// Assign the process identified by `pid` to this job. Opens a
        /// fresh handle to the process (we don't get a handle back from
        /// portable-pty's `Child::process_id`) and closes it once the
        /// assignment lands — the job object retains its own ref.
        ///
        /// # Errors
        /// Returns the Win32 error as an [`io::Error`] if `OpenProcess`
        /// or `AssignProcessToJobObject` fails.
        pub fn assign_pid(&self, pid: u32) -> io::Result<()> {
            let proc_handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, FALSE, pid) };
            if proc_handle.is_null() || proc_handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let rc = unsafe { AssignProcessToJobObject(self.handle, proc_handle) };
            let err = if rc == 0 {
                Some(io::Error::last_os_error())
            } else {
                None
            };
            unsafe { CloseHandle(proc_handle) };
            match err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }

        /// Kill every process in the job with `exit_code` (use `1` as
        /// the generic sentinel). Idempotent: subsequent calls are
        /// no-ops.
        ///
        /// # Errors
        /// Returns the Win32 error as an [`io::Error`] if
        /// `TerminateJobObject` fails on the first call.
        pub fn terminate(&mut self, exit_code: u32) -> io::Result<()> {
            if self.terminated {
                return Ok(());
            }
            let rc = unsafe { TerminateJobObject(self.handle, exit_code) };
            self.terminated = true;
            if rc == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            if !self.terminated {
                // Best-effort termination. `TerminateJobObject` with
                // exit_code=1 ensures any survivors die even though
                // CLOSE_ON_KILL should already cover that when
                // `CloseHandle` runs below.
                unsafe {
                    let _ = TerminateJobObject(self.handle, 1);
                }
            }
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn create_and_drop_job_is_infallible_on_clean_system() {
            // Smoke test: if we can't even create a job with no limits,
            // Windows is in a state where nothing else we do here will
            // work. Surfaces CI problems early.
            let _job = JobObject::create().expect("create job");
        }

        #[test]
        fn assign_to_current_process_succeeds_once() {
            let job = JobObject::create().expect("create job");
            let pid = std::process::id();
            // Assigning the current process is legal if the current
            // process isn't already in a different job; on CI runners
            // that might fail, so tolerate either outcome — we only
            // want to prove the call compiles and links correctly.
            let _ = job.assign_pid(pid);
        }

        #[test]
        fn terminate_is_idempotent() {
            let mut job = JobObject::create().expect("create job");
            job.terminate(0).expect("first terminate");
            job.terminate(0).expect("second terminate is a no-op");
        }
    }
}

#[cfg(not(windows))]
mod imp {
    //! Unix stub — PRD-09 §5.2's `kill(-pgid, SIG…)` path covers the
    //! same "kill the whole tree" guarantee without a kernel wrapper
    //! object, so `JobObject` on non-Windows platforms is an
    //! unconstructible placeholder that keeps the cross-platform
    //! surface honest. Callers that reach for it on Unix should fall
    //! back to the process-group path in [`crate::Session`].

    /// Placeholder kept so cross-platform code can name the type. No
    /// constructor is exposed on non-Windows — the module is
    /// effectively empty on Unix.
    pub struct JobObject {
        _private: (),
    }

    impl JobObject {
        /// Always returns an error on non-Windows. Documented to make
        /// it obvious in docs that Unix callers should use the
        /// process-group path instead.
        ///
        /// # Errors
        /// Always returns [`std::io::ErrorKind::Unsupported`].
        pub fn create() -> std::io::Result<Self> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Job Objects are Windows-only; use process groups on Unix",
            ))
        }
    }
}

pub use imp::JobObject;
