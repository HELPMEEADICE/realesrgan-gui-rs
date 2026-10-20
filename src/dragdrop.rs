//! Native file drag-and-drop.
//!
//! Slint's winit backend does not forward OS-level file drops to `DropArea`
//! yet, so on Windows the window itself is registered as an OLE drop target.
//! winit installs its own drop target and emits file-drop events that Slint
//! ignores, so that registration is taken over first. Dropped paths are
//! forwarded to the UI thread through a channel and picked up by the regular
//! UI timer.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// Owns the platform drop target for as long as the window exists.
pub struct FileDropTarget {
    #[cfg(windows)]
    _inner: windows_target::WindowsDropTarget,
    #[cfg(not(windows))]
    _private: (),
}

impl FileDropTarget {
    /// Returns `None` while the window handle is not available yet, so the
    /// caller can retry from the UI timer. Platforms without a native
    /// implementation return an inert target.
    pub fn new(window: &slint::Window, sender: Sender<Vec<PathBuf>>) -> Option<Self> {
        #[cfg(windows)]
        {
            windows_target::WindowsDropTarget::new(window, sender)
                .map(|inner| Self { _inner: inner })
        }
        #[cfg(not(windows))]
        {
            let _ = (window, sender);
            Some(Self { _private: () })
        }
    }
}

#[cfg(windows)]
mod windows_target {
    use std::cell::Cell;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::{implement, Ref, Result};
    use windows::Win32::Foundation::{HWND, POINTL};
    use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{
        IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
        CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    pub(super) struct WindowsDropTarget {
        hwnd: HWND,
        /// Keeps our interface alive even though COM also holds a reference.
        _target: IDropTarget,
    }

    impl WindowsDropTarget {
        pub(super) fn new(window: &slint::Window, sender: Sender<Vec<PathBuf>>) -> Option<Self> {
            let hwnd = window_hwnd(window)?;
            let target: IDropTarget = DropTargetData {
                sender,
                accepts_files: Cell::new(false),
            }
            .into();
            // winit registers its own drop target for the file-drop events that
            // it emits, but Slint's winit backend does not consume those events.
            // Take the registration over so the drops reach this target instead.
            unsafe {
                let _ = RevokeDragDrop(hwnd);
            }
            unsafe { RegisterDragDrop(hwnd, &target) }.ok()?;
            Some(Self {
                hwnd,
                _target: target,
            })
        }
    }

    impl Drop for WindowsDropTarget {
        fn drop(&mut self) {
            // The window may already be gone when the application exits; a
            // failed revoke is harmless in that case.
            unsafe {
                let _ = RevokeDragDrop(self.hwnd);
            }
        }
    }

    fn window_hwnd(window: &slint::Window) -> Option<HWND> {
        let raw = window.window_handle().window_handle().ok()?.as_raw();
        match raw {
            RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut std::ffi::c_void)),
            _ => None,
        }
    }

    /// The COM object OLE calls into while a drag hovers over the window.
    #[implement(IDropTarget)]
    struct DropTargetData {
        sender: Sender<Vec<PathBuf>>,
        accepts_files: Cell<bool>,
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for DropTargetData_Impl {
        fn DragEnter(
            &self,
            pdataobj: Ref<IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> Result<()> {
            let accepted = pdataobj
                .ok()
                .map(|data| unsafe { can_read_files(data) })
                .unwrap_or(false);
            self.accepts_files.set(accepted);
            unsafe {
                *pdweffect = if accepted {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
            Ok(())
        }

        fn DragOver(
            &self,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> Result<()> {
            unsafe {
                *pdweffect = if self.accepts_files.get() {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
            Ok(())
        }

        fn DragLeave(&self) -> Result<()> {
            self.accepts_files.set(false);
            Ok(())
        }

        fn Drop(
            &self,
            pdataobj: Ref<IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> Result<()> {
            self.accepts_files.set(false);
            let paths = pdataobj
                .ok()
                .map(|data| unsafe { extract_paths(data) })
                .unwrap_or_default();
            if paths.is_empty() {
                unsafe {
                    *pdweffect = DROPEFFECT_NONE;
                }
                return Ok(());
            }
            let _ = self.sender.send(paths);
            unsafe {
                *pdweffect = DROPEFFECT_COPY;
            }
            Ok(())
        }
    }

    fn hdrop_format() -> FORMATETC {
        FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    unsafe fn can_read_files(data: &IDataObject) -> bool {
        let format = hdrop_format();
        unsafe { data.QueryGetData(&format) }.is_ok()
    }

    unsafe fn extract_paths(data: &IDataObject) -> Vec<PathBuf> {
        let format = hdrop_format();
        let Ok(mut medium) = (unsafe { data.GetData(&format) }) else {
            return Vec::new();
        };

        let mut paths = Vec::new();
        if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
            let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
            for index in 0..count {
                let length = unsafe { DragQueryFileW(hdrop, index, None) };
                if length == 0 {
                    continue;
                }
                let mut buffer = vec![0u16; length as usize + 1];
                let written = unsafe { DragQueryFileW(hdrop, index, Some(buffer.as_mut_slice())) };
                if written > 0 {
                    paths.push(PathBuf::from(OsString::from_wide(
                        &buffer[..written as usize],
                    )));
                }
            }
        }

        unsafe { ReleaseStgMedium(&mut medium) };
        paths
    }
}
