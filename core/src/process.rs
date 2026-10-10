//! Child processes (llama-server, whisper-cli): hidden windows, and never orphaned.
//!
//! On Windows every child joins a job object that kills it when Lariksa exits,
//! even on a crash, so a closed app never leaves a 3 GB model running.

use std::process::{Child, Command};

pub fn command(program: &std::path::Path) -> Command {
    let mut cmd = Command::new(program);
    // Runtimes load their DLLs from their own folder.
    if let Some(dir) = program.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn spawn(mut cmd: Command) -> std::io::Result<Child> {
    let child = cmd.spawn()?;
    #[cfg(windows)]
    job::adopt(&child);
    Ok(child)
}

/// Run a short command and collect stdout + stderr; kill it if it outlives ``timeout``.
pub fn output_within(program: &std::path::Path, args: &[&str], timeout: std::time::Duration) -> std::io::Result<String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut cmd = command(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = spawn(cmd)?;
    // Read both pipes on threads so a chatty child never blocks on a full pipe.
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            stop(&mut child);
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("{} timed out", program.display())));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(format!("{}{}", out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default()))
}

pub fn stop(child: &mut Child) {
    if let Ok(None) = child.try_wait() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn output_and_timeout() {
        let sh = std::path::Path::new("/bin/sh");
        assert_eq!(output_within(sh, &["-c", "echo out; echo err >&2"], Duration::from_secs(5)).unwrap(), "out\nerr\n");
        let t = Instant::now();
        let hung = output_within(sh, &["-c", "sleep 30"], Duration::from_millis(300));
        assert_eq!(hung.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(5)); // killed, not waited for
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    fn job() -> Option<&'static Job> {
        JOB.get_or_init(|| unsafe {
            let handle = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(Job(handle))
        })
        .as_ref()
    }

    pub fn adopt(child: &std::process::Child) {
        if let Some(job) = job() {
            unsafe {
                let _ = AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle()));
            }
        }
    }
}
