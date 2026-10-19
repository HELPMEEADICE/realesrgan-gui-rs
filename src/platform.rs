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

#[cfg(windows)]
struct WindowsTaskbar {
    list: windows::Win32::UI::Shell::ITaskbarList3,
    hwnd: windows::Win32::Foundation::HWND,
}

#[cfg(windows)]
impl WindowsTaskbar {
    fn new(window: &slint::Window) -> Option<Self> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::core::GUID;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{
            CoCreateInstance, CLSCTX_INPROC_SERVER,
        };
        use windows::Win32::UI::Shell::ITaskbarList3;

        const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);

        let raw = window.window_handle().window_handle().ok()?.as_raw();
        let hwnd = match raw {
            RawWindowHandle::Win32(win32) => HWND(win32.hwnd.get() as *mut std::ffi::c_void),
            _ => return None,
        };

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
            self.list.SetProgressValue(
                self.hwnd,
                completed,
                total.clamp(1, u64::MAX),
            )
        };
    }
}

/// Shows a desktop notification; used when a batch finishes or fails.
pub fn notify(title: &str, message: &str) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::{Duration, Toast};
        let _ = Toast::new(Toast::POWERSHELL_APP_ID)
            .title(title)
            .text1(message)
            .duration(Duration::Short)
            .show();
    }
    #[cfg(not(windows))]
    {
        let _ = (title, message);
    }
}

/// One time per-process platform initialisation.
pub fn init() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        // Failure is fine, COM may already be initialised by the backend.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    }
}
