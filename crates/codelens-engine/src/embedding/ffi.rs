use anyhow::Result;

pub fn register_sqlite_vec() -> Result<()> {
    let rc = unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )))
    };
    if rc != rusqlite::ffi::SQLITE_OK {
        anyhow::bail!("failed to register sqlite-vec extension (SQLite error code: {rc})");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn sysctl_usize(name: &[u8]) -> Option<usize> {
    let mut value: libc::c_uint = 0;
    let mut size = std::mem::size_of::<libc::c_uint>();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr().cast(),
            (&mut value as *mut libc::c_uint).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && size == std::mem::size_of::<libc::c_uint>()).then_some(value as usize)
}
