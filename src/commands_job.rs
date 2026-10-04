//! A Windows job object around a running command. Killing the shell alone
//! leaves what it started running and holding the output pipes; terminating
//! the job ends every process in it, but only when asked to.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject,
};

pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel object reference that any thread may use.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// A job used only to end a command's whole tree on Stop. It deliberately
    /// has no kill-on-close limit: the handle closes when the shell exits, and
    /// that must not kill what the command detached (`Start-Process`, GUI
    /// apps), which keeps running as it does on macOS.
    pub fn new() -> io::Result<Job> {
        // SAFETY: both arguments may be null: default security, no name.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Job(handle))
    }

    /// Processes the child starts from now on join the job with it.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: both handles are open: the job is ours and `child` is borrowed.
        let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn terminate(&self) {
        // SAFETY: the handle stays open until drop.
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: closed exactly once, here.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
