#[cfg(windows)]
use std::mem;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

#[cfg(windows)]
pub struct JobObject {
    handle: HANDLE,
}

#[cfg(windows)]
impl JobObject {
    pub fn new() -> Result<Self, String> {
        unsafe {
            // 1. Crear el Job Object
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err("Failed to create Job Object".to_string());
            }

            // 2. Configurar para que mate a los hijos si el padre muere
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let r = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if r == 0 {
                CloseHandle(handle);
                return Err("Failed to set Job Object info".to_string());
            }

            Ok(Self { handle })
        }
    }

    pub fn add_process(&self, process_handle: HANDLE) -> Result<(), String> {
        unsafe {
            let r = AssignProcessToJobObject(self.handle, process_handle);
            if r == 0 {
                return Err("Failed to assign process to job".to_string());
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
