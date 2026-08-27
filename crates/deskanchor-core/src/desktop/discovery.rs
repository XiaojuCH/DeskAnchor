use std::ffi::c_void;
use std::ptr::NonNull;

use anyhow::{Context, Result};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_UNAWARE, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, IFolderView, IShellBrowser, IShellFolder, IShellWindows, SID_STopLevelBrowser,
    SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::core::Interface;

pub(crate) struct DesktopFolderView {
    pub(crate) view: IFolderView,
    pub(crate) folder: IShellFolder,
}

impl DesktopFolderView {
    pub(crate) fn open() -> Result<Self> {
        let shell_windows: IShellWindows = unsafe {
            // SAFETY: the caller initialized COM on this thread. ShellWindows is a
            // registered system coclass and aggregation is not requested.
            CoCreateInstance(&ShellWindows, None::<&windows::core::IUnknown>, CLSCTX_ALL)
        }
        .context("failed to create ShellWindows")?;
        let location = VARIANT::from(CSIDL_DESKTOP as i32);
        let empty = VARIANT::default();
        let mut desktop_hwnd = 0;
        let dispatch = unsafe {
            // SAFETY: both VARIANT pointers and the HWND output live through the call.
            // SWC_DESKTOP with SWFO_NEEDDISPATCH is the supported desktop-view route.
            shell_windows.FindWindowSW(
                &location,
                &empty,
                SWC_DESKTOP,
                &mut desktop_hwnd,
                SWFO_NEEDDISPATCH,
            )
        }
        .context("ShellWindows could not find the Explorer desktop")?;
        let service_provider: IServiceProvider = dispatch
            .cast()
            .context("desktop dispatch did not expose IServiceProvider")?;
        let browser: IShellBrowser = unsafe {
            // SAFETY: SID_STopLevelBrowser is the documented service exposed by the
            // Explorer desktop dispatch object; the binding requests IShellBrowser.
            service_provider.QueryService(&SID_STopLevelBrowser)
        }
        .context("desktop did not expose the top-level Shell browser")?;
        let shell_view = unsafe {
            // SAFETY: `browser` is live on the current STA.
            browser.QueryActiveShellView()
        }
        .context("desktop Shell browser did not have an active view")?;
        let view: IFolderView = shell_view
            .cast()
            .context("desktop Shell view did not expose IFolderView")?;
        let folder = unsafe {
            // SAFETY: `view` is a live COM interface on this initialized STA, and the
            // generated binding supplies IShellFolder's IID and output slot.
            view.GetFolder()
        }
        .context("desktop view did not expose IShellFolder")?;
        Ok(Self { view, folder })
    }
}

pub(crate) fn on_shell_sta<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    std::thread::Builder::new()
        .name("deskanchor-shell-sta".into())
        .spawn(move || {
            let _dpi_awareness = DpiAwarenessScope::enter_unaware()?;
            let _com = ComApartment::initialize_sta()?;
            operation()
        })
        .context("failed to start the Shell STA thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("the Shell STA thread panicked"))?
}

/// Pins Shell calls to one verified coordinate regime, independent of the host.
struct DpiAwarenessScope(DPI_AWARENESS_CONTEXT);

impl DpiAwarenessScope {
    fn enter_unaware() -> Result<Self> {
        let previous = unsafe {
            // SAFETY: this newly spawned worker owns no windows. The returned
            // previous context is retained and restored on this same thread.
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE)
        };
        if previous.0.is_null() {
            return Err(windows::core::Error::from_thread())
                .context("failed to enter the verified DPI-unaware Shell context");
        }
        Ok(Self(previous))
    }
}

impl Drop for DpiAwarenessScope {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: restores the successful call's prior context on this thread.
            let _ = SetThreadDpiAwarenessContext(self.0);
        }
    }
}

pub(crate) struct OwnedPidl(NonNull<ITEMIDLIST>);

impl OwnedPidl {
    pub(crate) fn new(value: *mut ITEMIDLIST) -> Result<Self> {
        NonNull::new(value)
            .map(Self)
            .context("Explorer returned a null item PIDL")
    }

    pub(crate) fn as_ptr(&self) -> *const ITEMIDLIST {
        self.0.as_ptr()
    }
}

impl Drop for OwnedPidl {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: IFolderView::Item transfers a task-allocator-owned PIDL.
            CoTaskMemFree(Some(self.0.as_ptr().cast::<c_void>()));
        }
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize_sta() -> Result<Self> {
        unsafe {
            // SAFETY: called once on the newly spawned thread before any COM use.
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
        }
        .ok()
        .context("failed to initialize a COM single-threaded apartment")?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: balances the successful CoInitializeEx on this same thread.
            CoUninitialize();
        }
    }
}
