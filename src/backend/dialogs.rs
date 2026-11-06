//! Windows file and folder dialogs. They block until the user answers, so call them from a thread.
use std::path::PathBuf;

/// "Save as": returns the chosen path, or None when cancelled.
pub fn save_file(title: &str, filter_name: &str, ext: &str, default_name: &str) -> Option<PathBuf> {
    imp::file(true, title, filter_name, ext, default_name)
}

/// "Open": returns the chosen path, or None when cancelled.
pub fn open_file(title: &str, filter_name: &str, ext: &str) -> Option<PathBuf> {
    imp::file(false, title, filter_name, ext, "")
}

/// "Open" with several file types (extensions without the dot).
pub fn open_file_any(title: &str, filter_name: &str, exts: &[&str]) -> Option<PathBuf> {
    let pattern = exts.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";");
    imp::file_pattern(title, filter_name, &pattern)
}

/// Choose a folder.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    imp::folder(title)
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;

    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    use windows::Win32::UI::Shell::{BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListW};
    use windows::core::{PCWSTR, PWSTR};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn file_pattern(title: &str, filter_name: &str, pattern: &str) -> Option<PathBuf> {
        open_or_save(false, title, filter_name, pattern, "", "")
    }

    pub fn file(save: bool, title: &str, filter_name: &str, ext: &str, default_name: &str) -> Option<PathBuf> {
        open_or_save(save, title, filter_name, &format!("*.{ext}"), ext, default_name)
    }

    fn open_or_save(save: bool, title: &str, filter_name: &str, pattern: &str, ext: &str, default_name: &str) -> Option<PathBuf> {
        // "Name\0*.ext\0\0"
        let mut filter: Vec<u16> = Vec::new();
        for part in [filter_name.to_string(), pattern.to_string()] {
            filter.extend(part.encode_utf16());
            filter.push(0);
        }
        filter.push(0);
        let title = wide(title);
        let ext = wide(ext);
        let mut buf = [0u16; 1024];
        for (i, u) in default_name.encode_utf16().take(buf.len() - 1).enumerate() {
            buf[i] = u;
        }
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrTitle: PCWSTR(title.as_ptr()),
            lpstrDefExt: if save { PCWSTR(ext.as_ptr()) } else { PCWSTR::null() },
            Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST },
            ..Default::default()
        };
        let ok = unsafe { if save { GetSaveFileNameW(&mut ofn).as_bool() } else { GetOpenFileNameW(&mut ofn).as_bool() } };
        if !ok {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }

    pub fn folder(title: &str) -> Option<PathBuf> {
        let title = wide(title);
        let mut name = [0u16; 260];
        let info = BROWSEINFOW { pszDisplayName: PWSTR(name.as_mut_ptr()), lpszTitle: PCWSTR(title.as_ptr()), ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE, ..Default::default() };
        unsafe {
            let id = SHBrowseForFolderW(&info);
            if id.is_null() {
                return None;
            }
            let mut path = [0u16; 260];
            let ok = SHGetPathFromIDListW(id, &mut path).as_bool();
            windows::Win32::System::Com::CoTaskMemFree(Some(id as *const _));
            if !ok {
                return None;
            }
            let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
            Some(PathBuf::from(String::from_utf16_lossy(&path[..len])))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;
    pub fn file(_: bool, _: &str, _: &str, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    pub fn file_pattern(_: &str, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    pub fn folder(_: &str) -> Option<PathBuf> {
        None
    }
}
