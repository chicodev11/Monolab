//! Ties server processes to the app. Stopping the app normally stops servers politely (RunEvent::Exit);
//! this covers a crash or Task Manager: the job closes with the app, and Windows ends every process in it.

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// The job handle as a number (0 when Windows refused to make one); it stays open until the app ends.
    static JOB: OnceLock<usize> = OnceLock::new();

    fn job() -> Option<HANDLE> {
        let raw = *JOB.get_or_init(|| {
            // SAFETY: plain Win32 calls on a handle this function owns; info lives across the call.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return 0;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if set == 0 {
                    CloseHandle(job);
                    return 0;
                }
                job as usize
            }
        });
        (raw != 0).then_some(raw as HANDLE)
    }

    pub fn bind(child: &std::process::Child) {
        if let Some(job) = job() {
            // SAFETY: both handles are valid; the child's handle is owned by `child`, which outlives the call.
            unsafe {
                AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
            }
        }
    }
}

/// Ends `child` (and what it starts afterwards) when the app ends, however it ends. Best effort.
pub fn bind(child: &std::process::Child) {
    #[cfg(windows)]
    imp::bind(child);
    #[cfg(not(windows))]
    let _ = child;
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn a_child_can_join_the_job() {
        let mut child = crate::hidden_command("cmd").args(["/C", "exit 0"]).spawn().expect("cmd runs");
        super::bind(&child);
        assert!(child.wait().expect("cmd ends").success());
    }
}
