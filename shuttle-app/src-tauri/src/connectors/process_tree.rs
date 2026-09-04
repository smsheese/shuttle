//! Cross-platform child-process lifecycle: die with Shuttle on Linux, macOS, and Windows.

use tokio::process::Command;

/// Call once at startup (before spawning connectors).
pub fn init() {
    #[cfg(windows)]
    init_windows_job();
}

pub fn prepare_connector_command(command: &mut Command) {
    // Do not use kill_on_drop: tokio requires a runtime on drop, but hibernation/quit
    // stops sidecars from the main thread and sleep loop via sync kill_process().

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.as_std_mut().pre_exec(|| {
                let _ = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
}

/// After a connector child is spawned, attach it to the Windows job object (if any).
pub fn on_connector_spawned(pid: u32) {
    #[cfg(windows)]
    assign_windows_job(pid);
    let _ = pid;
}

#[cfg(windows)]
mod windows_job {
    use std::ptr::null_mut;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    static JOB: OnceLock<HANDLE> = OnceLock::new();

    pub fn init() {
        JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(null_mut(), null_mut());
            if job.is_null() {
                tracing::warn!("CreateJobObjectW failed; connector orphans may survive Shuttle exit");
                return job;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *mut _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                tracing::warn!("SetInformationJobObject failed; connector orphans may survive Shuttle exit");
            }
            job
        });
    }

    pub fn assign(pid: u32) {
        let Some(job) = JOB.get() else {
            return;
        };
        if job.is_null() {
            return;
        }
        unsafe {
            let proc = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if proc.is_null() {
                tracing::warn!("OpenProcess failed for connector pid {pid}");
                return;
            }
            if AssignProcessToJobObject(*job, proc) == 0 {
                tracing::warn!("AssignProcessToJobObject failed for connector pid {pid}");
            }
            CloseHandle(proc);
        }
    }
}

#[cfg(windows)]
fn init_windows_job() {
    windows_job::init();
}

#[cfg(windows)]
fn assign_windows_job(pid: u32) {
    windows_job::assign(pid);
}
