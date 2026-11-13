// SPDX-License-Identifier: Apache-2.0
//! Check the file DACL itself, without the Windows parent-listing attribute grant.
use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_TOKEN,
};
use windows_sys::Win32::Security::{
    AccessCheck, DACL_SECURITY_INFORMATION, DuplicateToken, GENERIC_MAPPING,
    GROUP_SECURITY_INFORMATION, GetKernelObjectSecurity, OWNER_SECURITY_INFORMATION, PRIVILEGE_SET,
    SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_READ_ATTRIBUTES,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

fn effective_token() -> io::Result<OwnedHandle> {
    let mut raw = null_mut();
    // SAFETY: the pseudo handle is valid and `raw` is a writable handle slot.
    let opened = unsafe {
        OpenThreadToken(
            GetCurrentThread(),
            TOKEN_QUERY | TOKEN_DUPLICATE,
            1,
            &mut raw,
        )
    };
    if opened == 0 {
        let error = io::Error::last_os_error();
        // Honor impersonation. Only an absent thread token permits falling back
        // to the process identity; an inaccessible thread token must fail closed.
        if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
            return Err(error);
        }
        // SAFETY: the pseudo handle is valid and `raw` is writable.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut raw) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: a successful token open transfers one owned handle to us.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut duplicate = null_mut();
    // AccessCheck requires an impersonation token, including for a process caller.
    // SAFETY: `token` remains alive; `duplicate` is a writable handle slot.
    if unsafe { DuplicateToken(token.as_raw_handle(), SecurityImpersonation, &mut duplicate) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateToken returned a new owned handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(duplicate) })
}

pub(super) fn check_read_attributes(file: &File) -> io::Result<()> {
    let information =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut needed = 0;
    // Query the same no-follow handle as the metadata, never a second path lookup.
    // Owner and group are required by AccessCheck even when only the DACL matters.
    // SAFETY: the handle is borrowed; a null, zero-length buffer queries its size.
    let found = unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            information,
            null_mut(),
            0,
            &mut needed,
        )
    };
    if found == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(error);
        }
    }
    // DWORD alignment suffices for a self-relative SECURITY_DESCRIPTOR. If the
    // ACL grows between queries, return the error rather than trusting cached data.
    let mut descriptor = vec![0_u32; (needed as usize).div_ceil(size_of::<u32>())];
    // SAFETY: the aligned allocation holds at least `needed` writable bytes.
    if unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            information,
            descriptor.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let token = effective_token()?;
    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    };
    let mut privileges = PRIVILEGE_SET::default();
    let mut privilege_size = size_of::<PRIVILEGE_SET>() as u32;
    let mut granted = 0;
    let mut allowed = 0;
    // READ_ATTRIBUTES needs no privilege grant. Any check failure (including an
    // insufficient privilege buffer) remains an error, never an enumeration fallback.
    // SAFETY: the descriptor came from Windows, the token is owned, and all output
    // pointers reference live, correctly sized and aligned local values.
    if unsafe {
        AccessCheck(
            descriptor.as_mut_ptr().cast(),
            token.as_raw_handle(),
            FILE_READ_ATTRIBUTES,
            &mapping,
            &mut privileges,
            &mut privilege_size,
            &mut granted,
            &mut allowed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if allowed == 0 {
        return Err(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32));
    }
    Ok(())
}
