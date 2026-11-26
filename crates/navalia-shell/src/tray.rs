//! Enumerates and invokes Windows 11/10 notification-area (system tray) icons
//! hosted inside `Shell_TrayWnd` and overflow flyouts via UI Automation,
//! enriched with executable paths from `HKCU\Control Panel\NotifyIconSettings`
//! and a running-process fallback when the overflow flyout is closed.

use std::{collections::HashSet, mem::size_of, path::Path};

use windows::{
    Win32::{
        Foundation::CloseHandle,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Registry::{
                HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
                RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
            },
            Threading::{
                OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
            Variant::VARIANT,
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationInvokePattern,
                IUIAutomationLegacyIAccessiblePattern, TreeScope_Descendants,
                UIA_ButtonControlTypeId, UIA_ControlTypePropertyId, UIA_InvokePatternId,
                UIA_LegacyIAccessiblePatternId,
            },
            Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath},
            WindowsAndMessaging::FindWindowW,
        },
    },
    core::{GUID, Interface, PCWSTR, PWSTR, w},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub id: String,
    pub name: String,
    pub exe_path: Option<String>,
}

struct ComGuard(bool);

impl ComGuard {
    fn init() -> Self {
        // SAFETY: Apartment-threaded COM init for UIA on the calling worker thread.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self(hr.is_ok())
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: Matches a successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

struct NotifySetting {
    key: String,
    tooltip: String,
    tooltip_lower: String,
    exe_path: String,
    exe_lower: String,
    stem: String,
    is_promoted: bool,
}

fn read_reg_sz(key: HKEY, name: PCWSTR) -> Option<String> {
    let mut bytes = 0u32;
    // SAFETY: Queries string byte length first, then reads into a sized UTF-16 buffer.
    unsafe {
        RegGetValueW(key, None, name, RRF_RT_REG_SZ, None, None, Some(&mut bytes))
            .ok()
            .ok()?;
        let mut buf = vec![0u16; (bytes as usize / 2).max(1)];
        RegGetValueW(
            key,
            None,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        (!text.is_empty()).then_some(text)
    }
}

fn read_reg_dword(key: HKEY, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    // SAFETY: Queries a 4-byte DWORD value into `value`.
    unsafe {
        RegGetValueW(
            key,
            None,
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
    }
    Some(value)
}

fn parse_guid(text: &str) -> Option<GUID> {
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    let parts: Vec<&str> = inner.split('-').collect();
    if parts.len() != 5
        || parts[0].len() != 8
        || parts[1].len() != 4
        || parts[2].len() != 4
        || parts[3].len() != 4
        || parts[4].len() != 12
    {
        return None;
    }
    let hex: String = parts.concat();
    let val = u128::from_str_radix(&hex, 16).ok()?;
    Some(GUID::from_u128(val))
}

fn resolve_known_folder_path(trimmed: &str) -> Option<String> {
    let brace_end = trimmed.find('}')?;
    if !trimmed.starts_with('{') || trimmed.as_bytes().get(brace_end + 1) != Some(&b'\\') {
        return None;
    }
    let guid = parse_guid(&trimmed[..=brace_end])?;
    let rest = &trimmed[brace_end + 2..];
    // SAFETY: `SHGetKnownFolderPath` allocates a PWSTR freed with `CoTaskMemFree`.
    let folder = unsafe {
        let ptr = SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None).ok()?;
        let s = ptr.to_string().ok();
        CoTaskMemFree(Some(ptr.0.cast()));
        s?
    };
    let joined = Path::new(&folder).join(rest);
    joined
        .exists()
        .then(|| joined.to_string_lossy().into_owned())
}

fn normalize_exe_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().strip_prefix(r"\??\").unwrap_or(raw.trim());
    if trimmed.starts_with('{') {
        return resolve_known_folder_path(trimmed);
    }
    if Path::new(trimmed).exists() {
        return Some(trimmed.to_owned());
    }
    None
}

fn load_notify_settings() -> Vec<NotifySetting> {
    let mut settings = Vec::new();
    let mut root = HKEY::default();
    // SAFETY: Read-only registry enumeration under HKCU\Control Panel\NotifyIconSettings.
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Control Panel\NotifyIconSettings"),
            Some(0),
            KEY_READ,
            &mut root,
        )
        .is_err()
        {
            return settings;
        }
        let mut index = 0u32;
        let mut name_buf = [0u16; 256];
        loop {
            let mut len = name_buf.len() as u32;
            if RegEnumKeyExW(
                root,
                index,
                Some(PWSTR(name_buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
            .is_err()
            {
                break;
            }
            index += 1;
            let key_name = String::from_utf16_lossy(&name_buf[..len as usize]);
            let mut sub = HKEY::default();
            if RegOpenKeyExW(root, PCWSTR(name_buf.as_ptr()), Some(0), KEY_READ, &mut sub).is_ok() {
                if let Some(raw_exe) = read_reg_sz(sub, w!("ExecutablePath"))
                    && let Some(exe_path) = normalize_exe_path(&raw_exe)
                {
                    let tooltip = read_reg_sz(sub, w!("InitialTooltip"))
                        .map(|t| t.trim().to_owned())
                        .unwrap_or_default();
                    let tooltip_lower = tooltip.to_lowercase();
                    let exe_lower = exe_path.to_lowercase();
                    let stem = Path::new(&exe_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_lowercase();
                    let is_promoted = read_reg_dword(sub, w!("IsPromoted")) == Some(1);
                    settings.push(NotifySetting {
                        key: key_name,
                        tooltip,
                        tooltip_lower,
                        exe_path,
                        exe_lower,
                        stem,
                        is_promoted,
                    });
                }
                let _ = RegCloseKey(sub);
            }
        }
        let _ = RegCloseKey(root);
    }
    settings
}

fn match_exe_path(aid: &str, name: &str, settings: &[NotifySetting]) -> Option<String> {
    if !aid.is_empty()
        && let Some(hit) = settings
            .iter()
            .find(|s| !s.key.is_empty() && aid.contains(&s.key))
    {
        return Some(hit.exe_path.clone());
    }
    let lower = name.to_lowercase();
    if let Some(hit) = settings.iter().find(|s| {
        !s.tooltip_lower.is_empty()
            && (lower == s.tooltip_lower
                || lower.contains(&s.tooltip_lower)
                || s.tooltip_lower.contains(&lower))
    }) {
        return Some(hit.exe_path.clone());
    }
    settings
        .iter()
        .find(|s| !s.stem.is_empty() && lower.contains(&s.stem))
        .map(|s| s.exe_path.clone())
}

fn make_tray_id(aid: &str, name: &str) -> String {
    if aid.is_empty() || aid == "NotifyItemIcon" {
        name.to_owned()
    } else {
        format!("{aid}|{name}")
    }
}

fn is_system_tray_control(aid: &str) -> bool {
    matches!(
        aid,
        "SystemTrayIcon"
            | "ShowHiddenIcons"
            | "ClockButton"
            | "StartButton"
            | "SearchButton"
            | "TaskViewButton"
            | "WidgetsButton"
            | "InputIndicatorButton"
            | "NotificationCenterButton"
            | "ControlCenterButton"
            | "ShowDesktopButton"
    ) || aid.starts_with("Appid:")
}

fn is_tray_button_in_taskbar(aid: &str, class_name: &str, name: &str) -> bool {
    if name.trim().is_empty() || is_system_tray_control(aid) {
        return false;
    }
    aid.starts_with("NotifyItemIcon") || class_name.contains("NotifyItemIcon")
}

fn is_tray_button_in_overflow(aid: &str, name: &str) -> bool {
    !(name.trim().is_empty() || is_system_tray_control(aid))
}

fn collect_window_tray_buttons(
    automation: &IUIAutomation,
    class_name: PCWSTR,
    is_overflow: bool,
    settings: &[NotifySetting],
    items: &mut Vec<TrayItem>,
) -> usize {
    let mut found = 0usize;
    // SAFETY: Standard UIA queries on Explorer's taskbar/overflow windows.
    unsafe {
        let Ok(hwnd) = FindWindowW(class_name, None) else {
            return 0;
        };
        let Ok(root) = automation.ElementFromHandle(hwnd) else {
            return 0;
        };
        let Ok(cond) = automation.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_ButtonControlTypeId.0),
        ) else {
            return 0;
        };
        let Ok(buttons) = root.FindAll(TreeScope_Descendants, &cond) else {
            return 0;
        };
        let count = buttons.Length().unwrap_or(0);
        for index in 0..count {
            let Ok(button) = buttons.GetElement(index) else {
                continue;
            };
            let aid = button
                .CurrentAutomationId()
                .map(|b| b.to_string())
                .unwrap_or_default();
            let class = button
                .CurrentClassName()
                .map(|b| b.to_string())
                .unwrap_or_default();
            let name = button
                .CurrentName()
                .map(|b| b.to_string())
                .unwrap_or_default();
            let accepted = if is_overflow {
                is_tray_button_in_overflow(&aid, &name)
            } else {
                is_tray_button_in_taskbar(&aid, &class, &name)
            };
            if !accepted {
                continue;
            }
            found += 1;
            let id = make_tray_id(&aid, &name);
            let exe_path = match_exe_path(&aid, &name, settings);
            let name_lower = name.to_lowercase();
            let exe_lower = exe_path.as_deref().map(str::to_lowercase);
            let duplicate = items.iter().any(|item| {
                item.id == id
                    || (exe_lower.is_some()
                        && item.name.to_lowercase() == name_lower
                        && item.exe_path.as_deref().map(str::to_lowercase) == exe_lower)
            });
            if duplicate {
                continue;
            }
            items.push(TrayItem { id, name, exe_path });
        }
    }
    found
}

fn running_process_exes() -> HashSet<String> {
    let mut exes = HashSet::new();
    // SAFETY: Read-only process snapshot and full image path query.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return exes;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &raw mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                if pid != 0
                    && let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
                {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        proc,
                        PROCESS_NAME_FORMAT(0),
                        PWSTR(buf.as_mut_ptr()),
                        &mut len,
                    )
                    .is_ok()
                    {
                        let path = String::from_utf16_lossy(&buf[..len as usize]);
                        exes.insert(path.to_lowercase());
                    }
                    let _ = CloseHandle(proc);
                }
                if Process32NextW(snap, &raw mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    exes
}

fn is_excluded_shell_binary(exe_lower: &str) -> bool {
    let file_name = Path::new(exe_lower)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    matches!(
        file_name,
        "explorer.exe"
            | "monotificationux.exe"
            | "shellexperiencehost.exe"
            | "startmenuexperiencehost.exe"
            | "searchhost.exe"
            | "navalia.exe"
            | "navalia-settings.exe"
    )
}

/// Lists visible and overflow third-party system tray icons, falling back to
/// running apps registered in `HKCU\Control Panel\NotifyIconSettings` when the
/// Windows overflow flyout is closed.
#[must_use]
pub fn list_tray_icons() -> Vec<TrayItem> {
    let _com = ComGuard::init();
    let settings = load_notify_settings();
    let mut items: Vec<TrayItem> = Vec::new();
    let mut overflow_count = 0usize;

    // SAFETY: Standard UIA initialization on the current apartment-threaded worker.
    unsafe {
        if let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        {
            collect_window_tray_buttons(
                &automation,
                w!("Shell_TrayWnd"),
                false,
                &settings,
                &mut items,
            );
            overflow_count += collect_window_tray_buttons(
                &automation,
                w!("TopLevelWindowForOverflowXamlIsland"),
                true,
                &settings,
                &mut items,
            );
            overflow_count += collect_window_tray_buttons(
                &automation,
                w!("NotifyIconOverflowWindow"),
                true,
                &settings,
                &mut items,
            );
        }
    }

    if overflow_count == 0 && !settings.is_empty() {
        let running = running_process_exes();
        let mut fallback: Vec<TrayItem> = Vec::new();
        for setting in &settings {
            if setting.is_promoted
                || is_excluded_shell_binary(&setting.exe_lower)
                || !running.contains(&setting.exe_lower)
            {
                continue;
            }
            let already_listed = items.iter().chain(fallback.iter()).any(|item| {
                item.exe_path.as_deref().map(str::to_lowercase).as_deref()
                    == Some(setting.exe_lower.as_str())
            });
            if already_listed {
                continue;
            }
            let display_name = if !setting.tooltip.is_empty() {
                setting.tooltip.clone()
            } else {
                Path::new(&setting.exe_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("App")
                    .to_owned()
            };
            fallback.push(TrayItem {
                id: format!("reg:{}", setting.exe_lower),
                name: display_name,
                exe_path: Some(setting.exe_path.clone()),
            });
        }
        fallback.sort_by_key(|a| a.name.to_lowercase());
        items.extend(fallback);
    }

    items
}

fn try_invoke_in_window(
    automation: &IUIAutomation,
    class_name: PCWSTR,
    id: &str,
    name: &str,
) -> bool {
    // SAFETY: Standard UIA queries and Invoke/DoDefaultAction on Explorer windows.
    unsafe {
        let Ok(hwnd) = FindWindowW(class_name, None) else {
            return false;
        };
        let Ok(root) = automation.ElementFromHandle(hwnd) else {
            return false;
        };
        let Ok(cond) = automation.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_ButtonControlTypeId.0),
        ) else {
            return false;
        };
        let Ok(buttons) = root.FindAll(TreeScope_Descendants, &cond) else {
            return false;
        };
        let count = buttons.Length().unwrap_or(0);
        for index in 0..count {
            let Ok(button) = buttons.GetElement(index) else {
                continue;
            };
            let aid = button
                .CurrentAutomationId()
                .map(|b| b.to_string())
                .unwrap_or_default();
            let btn_name = button
                .CurrentName()
                .map(|b| b.to_string())
                .unwrap_or_default();
            if make_tray_id(&aid, &btn_name) == id || (!name.is_empty() && btn_name == name) {
                if let Ok(pattern) = button.GetCurrentPattern(UIA_InvokePatternId)
                    && let Ok(invoke) = pattern.cast::<IUIAutomationInvokePattern>()
                    && invoke.Invoke().is_ok()
                {
                    return true;
                }
                if let Ok(pattern) = button.GetCurrentPattern(UIA_LegacyIAccessiblePatternId)
                    && let Ok(legacy) = pattern.cast::<IUIAutomationLegacyIAccessiblePattern>()
                    && legacy.DoDefaultAction().is_ok()
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Invokes a Windows 11/10 system tray icon by its `id` (or `name`),
/// falling back to launching `exe_path` if UIA invocation is unavailable.
pub fn invoke_tray_icon(id: &str, name: &str, exe_path: Option<&str>) -> bool {
    let _com = ComGuard::init();
    // SAFETY: Standard UIA invocation across taskbar and overflow host windows.
    let invoked = unsafe {
        CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            .ok()
            .is_some_and(|automation| {
                try_invoke_in_window(&automation, w!("Shell_TrayWnd"), id, name)
                    || try_invoke_in_window(
                        &automation,
                        w!("TopLevelWindowForOverflowXamlIsland"),
                        id,
                        name,
                    )
                    || try_invoke_in_window(&automation, w!("NotifyIconOverflowWindow"), id, name)
            })
    };
    if invoked {
        return true;
    }
    if let Some(path) = exe_path {
        return crate::launcher::open(path).is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_known_folder_guid() {
        let parsed = parse_guid("{6D809377-6AF0-444B-8957-A3773F02200E}");
        assert!(parsed.is_some());
        assert!(parse_guid("not-a-guid").is_none());
        assert!(parse_guid("{6D809377-6AF0-444B-8957-A3773F02200}").is_none());
    }

    #[test]
    fn make_tray_id_avoids_shared_notify_item_collision() {
        assert_eq!(make_tray_id("NotifyItemIcon", "Steam"), "Steam");
        assert_eq!(make_tray_id("NotifyItemIcon", "Discord"), "Discord");
        assert_eq!(make_tray_id("", "Verbum"), "Verbum");
        assert_eq!(
            make_tray_id("NotifyItemIcon_1234", "Oculus"),
            "NotifyItemIcon_1234|Oculus"
        );
    }

    #[test]
    fn filters_system_buttons_from_taskbar_and_overflow() {
        assert!(!is_tray_button_in_taskbar(
            "ShowHiddenIcons",
            "SystemTray.NormalChevronButton",
            "Show hidden icons"
        ));
        assert!(!is_tray_button_in_taskbar(
            "SystemTrayIcon",
            "SystemTray.OmniButton",
            "Network"
        ));
        assert!(!is_tray_button_in_taskbar(
            "Appid:Microsoft.Windows.Explorer",
            "Taskbar.TaskListButton",
            "File Explorer"
        ));
        assert!(is_tray_button_in_taskbar(
            "NotifyItemIcon",
            "SystemTray.NotifyIconView",
            "Steam"
        ));
        assert!(is_tray_button_in_overflow("NotifyItemIcon", "Discord"));
        assert!(!is_tray_button_in_overflow("ShowHiddenIcons", "Overflow"));
    }

    #[test]
    fn match_exe_path_prefers_subkey_then_tooltip_then_stem() {
        let settings = vec![
            NotifySetting {
                key: "16572215701068998714".to_owned(),
                tooltip: "Oculus".to_owned(),
                tooltip_lower: "oculus".to_owned(),
                exe_path: r"C:\Apps\Oculus\oculus.exe".to_owned(),
                exe_lower: r"c:\apps\oculus\oculus.exe".to_owned(),
                stem: "oculus".to_owned(),
                is_promoted: false,
            },
            NotifySetting {
                key: "8122504644871931265".to_owned(),
                tooltip: "Steam - synchronizing".to_owned(),
                tooltip_lower: "steam - synchronizing".to_owned(),
                exe_path: r"C:\Program Files (x86)\Steam\steam.exe".to_owned(),
                exe_lower: r"c:\program files (x86)\steam\steam.exe".to_owned(),
                stem: "steam".to_owned(),
                is_promoted: true,
            },
        ];

        assert_eq!(
            match_exe_path(
                "NotifyItemIcon_16572215701068998714",
                "Something",
                &settings
            )
            .as_deref(),
            Some(r"C:\Apps\Oculus\oculus.exe")
        );
        assert_eq!(
            match_exe_path("NotifyItemIcon", "Steam", &settings).as_deref(),
            Some(r"C:\Program Files (x86)\Steam\steam.exe")
        );
        assert_eq!(
            match_exe_path("NotifyItemIcon", "My Steam Client", &settings).as_deref(),
            Some(r"C:\Program Files (x86)\Steam\steam.exe")
        );
    }
}
