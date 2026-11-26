//! Per-user folders, resolved without ever falling back to a path relative
//! to the current folder.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

use windows::{
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE},
        Storage::Packaging::Appx::GetApplicationUserModelId,
        System::Threading::{PROCESS_NAME_WIN32, QueryFullProcessImageNameW},
    },
    core::PWSTR,
};

/// `%LOCALAPPDATA%`, or its usual place under the profile when the variable
/// is missing (a stripped environment), or the temp folder as a last resort.
#[must_use]
pub fn local_app_data() -> PathBuf {
    known_folder("LOCALAPPDATA", &["AppData", "Local"])
}

/// `%APPDATA%`, with the same fallbacks as [`local_app_data`].
#[must_use]
pub fn roaming_app_data() -> PathBuf {
    known_folder("APPDATA", &["AppData", "Roaming"])
}

fn known_folder(variable: &str, under_profile: &[&str]) -> PathBuf {
    let absolute = |value: OsString| {
        let path = PathBuf::from(value);
        path.is_absolute().then_some(path)
    };
    env::var_os(variable)
        .and_then(absolute)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .and_then(absolute)
                .map(|profile| {
                    under_profile
                        .iter()
                        .fold(profile, |path, part| path.join(part))
                })
        })
        .unwrap_or_else(env::temp_dir)
}

/// `Navalia` under [`local_app_data`], joined with `name`.
#[must_use]
pub fn local_file(name: impl AsRef<Path>) -> PathBuf {
    local_app_data().join("Navalia").join(name)
}

const MAX_WIDE_PATH: usize = 32_768;

/// Reads the full Win32 executable path of `process`, growing past 1024 chars
/// when needed so long paths are never truncated.
pub(crate) fn query_process_image(process: HANDLE) -> Option<String> {
    let mut size = 1024usize;
    loop {
        let mut buffer = vec![0u16; size];
        let mut len = buffer.len() as u32;
        // SAFETY: `buffer` is valid for `len` UTF-16 code units.
        match unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut len,
            )
        } {
            Ok(()) => return Some(String::from_utf16_lossy(&buffer[..len as usize])),
            Err(error)
                if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult()
                    && size < MAX_WIDE_PATH =>
            {
                size = (size * 2).min(MAX_WIDE_PATH);
            }
            Err(_) => return None,
        }
    }
}

/// Reads the Application User Model ID of `process`, growing past 1024 chars
/// when needed.
pub(crate) fn query_process_aumid(process: HANDLE) -> Option<String> {
    let mut size = 1024usize;
    loop {
        let mut buffer = vec![0u16; size];
        let mut len = buffer.len() as u32;
        // SAFETY: `buffer` is valid for `len` UTF-16 code units.
        let status = unsafe {
            GetApplicationUserModelId(process, &mut len, Some(PWSTR(buffer.as_mut_ptr())))
        };
        if status.is_ok() {
            return Some(String::from_utf16_lossy(
                &buffer[..len.saturating_sub(1) as usize],
            ));
        }
        if status == ERROR_INSUFFICIENT_BUFFER && size < MAX_WIDE_PATH {
            size = (len as usize).max(size * 2).min(MAX_WIDE_PATH);
            continue;
        }
        return None;
    }
}
