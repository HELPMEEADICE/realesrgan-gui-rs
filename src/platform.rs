//! Platform integration: opening URLs, the Windows taskbar progress bar and
//! completion toasts. Everything degrades to a no-op when unsupported.

/// Opens a URL in the default browser.
pub fn open_url(url: &str) -> bool {
    #[cfg(windows)]
    {
        use std::process::Command;
        // `start` is a cmd builtin; the empty argument keeps it from treating
        // the URL as a window title.
        return Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .is_ok();
    }
    #[cfg(target_os = "macos")]
    {
        return std::process::Command::new("open").arg(url).spawn().is_ok();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .is_ok();
    }
    #[allow(unreachable_code)]
    false
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskbarState {
    NoProgress,
    Normal,
}

/// Windows taskbar progress indicator, the equivalent of the `ITaskbarList3`
/// usage in the original implementation. On other platforms every method is a
/// no-op.
pub struct TaskbarProgress {
    #[cfg(windows)]
    inner: Option<WindowsTaskbar>,
}

impl TaskbarProgress {
    pub fn new(window: &slint::Window) -> Self {
        #[cfg(windows)]
        {
            TaskbarProgress {
                inner: WindowsTaskbar::new(window),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = window;
            TaskbarProgress {}
        }
    }

    pub fn set_state(&self, state: TaskbarState) {
        #[cfg(windows)]
        if let Some(inner) = &self.inner {
            inner.set_state(state);
        }
        #[cfg(not(windows))]
        let _ = state;
    }

    pub fn set_value(&self, completed: u64, total: u64) {
        #[cfg(windows)]
        if let Some(inner) = &self.inner {
            inner.set_value(completed, total);
        }
        #[cfg(not(windows))]
        let _ = (completed, total);
    }
}

/// Returns the native handle of the Slint window, when it has been created.
#[cfg(windows)]
fn window_hwnd(window: &slint::Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;

    let raw = window.window_handle().window_handle().ok()?.as_raw();
    match raw {
        RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut std::ffi::c_void)),
        _ => None,
    }
}

#[cfg(windows)]
struct WindowsTaskbar {
    list: windows::Win32::UI::Shell::ITaskbarList3,
    hwnd: windows::Win32::Foundation::HWND,
}

#[cfg(windows)]
impl WindowsTaskbar {
    fn new(window: &slint::Window) -> Option<Self> {
        use windows::core::GUID;
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
        use windows::Win32::UI::Shell::ITaskbarList3;

        const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);

        let hwnd = window_hwnd(window)?;

        let list: ITaskbarList3 =
            unsafe { CoCreateInstance(&CLSID_TASKBAR_LIST, None, CLSCTX_INPROC_SERVER) }.ok()?;
        unsafe { list.HrInit() }.ok()?;
        Some(WindowsTaskbar { list, hwnd })
    }

    fn set_state(&self, state: TaskbarState) {
        use windows::Win32::UI::Shell::{TBPF_NOPROGRESS, TBPF_NORMAL};
        let flag = match state {
            TaskbarState::NoProgress => TBPF_NOPROGRESS,
            TaskbarState::Normal => TBPF_NORMAL,
        };
        let _ = unsafe { self.list.SetProgressState(self.hwnd, flag) };
    }

    fn set_value(&self, completed: u64, total: u64) {
        let _ = unsafe {
            self.list
                .SetProgressValue(self.hwnd, completed, total.clamp(1, u64::MAX))
        };
    }
}

/// Shows a native notification-area balloon; used when a batch finishes or
/// fails. This avoids the WinRT toast workaround that borrowed PowerShell's
/// AppUserModelID, so the notification is attributed to this application and
/// uses its embedded icon.
pub fn notify(window: &slint::Window, title: &str, message: &str) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
            NIM_MODIFY, NOTIFYICONDATAW,
        };
        use windows::Win32::UI::WindowsAndMessaging::WM_APP;

        let Some(hwnd) = window_hwnd(window) else {
            return;
        };

        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
            uCallbackMessage: WM_APP + 1,
            hIcon: load_app_icon(),
            ..Default::default()
        };
        copy_wide(&mut data.szTip, crate::paths::APP_TITLE);

        // `NIM_ADD` fails when the icon is already registered, which is fine:
        // `NIM_MODIFY` below still updates the existing icon.
        let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };

        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_INFO;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, message);
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }
    #[cfg(not(windows))]
    {
        let _ = (window, title, message);
    }
}

#[cfg(windows)]
fn load_app_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, HICON, IDI_APPLICATION};

    unsafe {
        if let Ok(module) = GetModuleHandleW(None) {
            if let Ok(icon) = LoadIconW(Some(HINSTANCE(module.0)), PCWSTR(1 as _)) {
                return icon;
            }
        }
        LoadIconW(None, IDI_APPLICATION).unwrap_or(HICON(std::ptr::null_mut()))
    }
}

#[cfg(windows)]
fn copy_wide(destination: &mut [u16], text: &str) {
    let mut units = text.encode_utf16();
    for slot in destination.iter_mut() {
        *slot = units.next().unwrap_or(0);
        if *slot == 0 {
            return;
        }
    }
    // The text was too long: keep the buffer NUL-terminated.
    if let Some(last) = destination.last_mut() {
        *last = 0;
    }
}

/// One time per-process platform initialisation.
pub fn init() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Ole::OleInitialize;
        // OLE, not just COM, is required by the drag-and-drop drop target.
        // Failure is fine, the backend may already have initialised the apartment.
        let _ = unsafe { OleInitialize(None) };
    }
}
