// OLE drop target: lets files be dragged onto the island. Windows only hands a
// drag to a window it can hit, so `App::poll` makes the whole panel hit-testable
// while a mouse button is held over it (see `drag_catch`).

use windows::core::{implement, Ref};
use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Com::{
    IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

use crate::app::{with_app, DragKind};

const CF_HDROP: u16 = 15;

#[implement(IDropTarget)]
struct Target;

fn first_path(data: &IDataObject) -> Option<String> {
    unsafe {
        let fmt = FORMATETC {
            cfFormat: CF_HDROP,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut medium: STGMEDIUM = data.GetData(&fmt).ok()?;
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
        let path = if count > 0 {
            let len = DragQueryFileW(hdrop, 0, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = DragQueryFileW(hdrop, 0, Some(&mut buf)) as usize;
            Some(String::from_utf16_lossy(&buf[..n]))
        } else {
            None
        };
        ReleaseStgMedium(&mut medium);
        path
    }
}

fn has_files(data: &IDataObject) -> bool {
    unsafe {
        let fmt = FORMATETC {
            cfFormat: CF_HDROP,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        data.QueryGetData(&fmt).is_ok()
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let ok = data.as_ref().map(has_files).unwrap_or(false);
        unsafe {
            *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        }
        if ok {
            let (x, y) = (pt.x, pt.y);
            with_app(|a| {
                a.on_drag_pos(x, y);
                a.on_drag(DragKind::Over);
            });
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe {
            *effect = DROPEFFECT_COPY;
        }
        let (x, y) = (pt.x, pt.y);
        with_app(|a| {
            a.on_drag_pos(x, y);
            a.on_drag(DragKind::Over);
        });
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        with_app(|a| a.on_drag(DragKind::Leave));
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let path = data.as_ref().and_then(first_path);
        unsafe {
            *effect = if path.is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        }
        let (x, y) = (pt.x, pt.y);
        with_app(|a| {
            a.on_drag_pos(x, y);
            a.on_drag(DragKind::Drop(path));
        });
        Ok(())
    }
}

/// Registers the island window as a file drop target (needs the UI thread).
pub fn register(hwnd: HWND) {
    unsafe {
        let _ = OleInitialize(None);
        let target: IDropTarget = Target.into();
        let _ = RegisterDragDrop(hwnd, &target);
    }
}
