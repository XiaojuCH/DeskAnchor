use std::ffi::c_void;
use std::ptr::NonNull;

use anyhow::{Context, Result, bail};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    IShellItem, SHCreateItemWithParent, SIGDN, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    SVGIO_ALLVIEW,
};
use windows::core::PWSTR;

use super::discovery::{DesktopFolderView, OwnedPidl, on_shell_sta};
use super::model::{DesktopIcon, DesktopState, DisplayConfiguration, IconIdentity};
use super::monitors::capture_monitors;

pub(crate) struct LiveIcon {
    pub(crate) model: DesktopIcon,
    pub(crate) pidl: OwnedPidl,
}

pub fn capture_current() -> Result<DesktopState> {
    on_shell_sta(|| {
        let desktop = DesktopFolderView::open()?;
        let icons = enumerate_icons(&desktop)?
            .into_iter()
            .map(|icon| icon.model)
            .collect();
        let display = DisplayConfiguration::new(capture_monitors()?);
        Ok(DesktopState { display, icons })
    })
}

pub(crate) fn enumerate_icons(desktop: &DesktopFolderView) -> Result<Vec<LiveIcon>> {
    let item_count = unsafe {
        // SAFETY: the folder view remains alive and SVGIO_ALLVIEW is a valid flag.
        desktop.view.ItemCount(SVGIO_ALLVIEW)
    }
    .context("failed to count desktop items")?;
    if item_count < 0 {
        bail!("Explorer returned a negative desktop item count: {item_count}");
    }

    let mut icons = Vec::with_capacity(item_count as usize);
    for index in 0..item_count {
        let pidl = OwnedPidl::new(
            unsafe {
                // SAFETY: `index` is within ItemCount's range and the view stays alive.
                desktop.view.Item(index)
            }
            .with_context(|| format!("failed to get desktop item at view index {index}"))?,
        )?;
        let position = unsafe {
            // SAFETY: this child PIDL came from this exact live folder view.
            desktop.view.GetItemPosition(pidl.as_ptr())
        }
        .with_context(|| format!("failed to read desktop item position at index {index}"))?;
        let item: IShellItem = unsafe {
            // SAFETY: `folder` is the parent that produced this child PIDL.
            SHCreateItemWithParent(None, &desktop.folder, pidl.as_ptr())
        }
        .with_context(|| format!("failed to create IShellItem at index {index}"))?;
        let display_name = shell_item_name(&item, SIGDN_NORMALDISPLAY)
            .with_context(|| format!("failed to read display name at index {index}"))?;
        let parsing_name = shell_item_name(&item, SIGDN_DESKTOPABSOLUTEPARSING)
            .with_context(|| format!("failed to read parsing identity at index {index}"))?;
        icons.push(LiveIcon {
            model: DesktopIcon {
                identity: IconIdentity::shell_parsing_name(parsing_name),
                display_name,
                x: position.x,
                y: position.y,
            },
            pidl,
        });
    }
    Ok(icons)
}

fn shell_item_name(item: &IShellItem, kind: SIGDN) -> Result<String> {
    let value = unsafe {
        // SAFETY: `item` is live and `kind` is a valid SIGDN value.
        item.GetDisplayName(kind)
    }
    .context("IShellItem::GetDisplayName failed")?;
    let value = OwnedWideString::new(value)?;
    unsafe {
        // SAFETY: a successful Shell call returns a NUL-terminated PWSTR.
        value.as_pwstr().to_string()
    }
    .context("Shell item name was not valid UTF-16")
}

struct OwnedWideString(NonNull<u16>);

impl OwnedWideString {
    fn new(value: PWSTR) -> Result<Self> {
        NonNull::new(value.0)
            .map(Self)
            .context("Shell returned a null display-name pointer")
    }

    fn as_pwstr(&self) -> PWSTR {
        PWSTR(self.0.as_ptr())
    }
}

impl Drop for OwnedWideString {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: IShellItem::GetDisplayName uses the COM task allocator.
            CoTaskMemFree(Some(self.0.as_ptr().cast::<c_void>()));
        }
    }
}
