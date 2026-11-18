//! The blur sits in its own click-through window just beneath the widget:
//! Windows' blur ignores window regions and per-pixel alpha, so separate
//! panels need composition visuals, and putting the widget's drawing on
//! those would load a Direct3D device (about 100 MB on a GPU driver).

use std::sync::Once;

use pulsar_core::layout::Rect;
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{Compositor, ContainerVisual};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_HOSTBACKDROPBRUSH, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DwmSetWindowAttribute,
};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LWA_ALPHA, RegisterClassW, SW_HIDE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, Interface, Result, w};
use windows_numerics::{Vector2, Vector3};

const CLASS: windows::core::PCWSTR = w!("PulsarBackdrop");

pub type Shape = (Rect, f32);

pub struct Backdrop {
    hwnd: HWND,
    compositor: Compositor,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    shapes: Vec<Shape>,
    strength: f32,
    shown: bool,
}

impl Backdrop {
    pub fn create(instance: HINSTANCE) -> Result<Self> {
        dispatcher_queue()?;
        static CLASS_ONCE: Once = Once::new();
        CLASS_ONCE.call_once(|| unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(backdrop_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            });
        });
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_NOREDIRECTIONBITMAP,
                CLASS,
                w!("Pulsar backdrop"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?
        };
        let built = Self::build(hwnd);
        if built.is_err() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        built
    }

    fn build(hwnd: HWND) -> Result<Self> {
        unsafe {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
            let on = BOOL::from(true);
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const on).cast(),
                size_of::<BOOL>() as u32,
            )?;
            let square = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const square).cast(),
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
        }
        crate::overlay::disable_transitions(hwnd);
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, true)? };
        let root = compositor.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        Ok(Self {
            hwnd,
            compositor,
            _target: target,
            root,
            shapes: Vec::new(),
            strength: 1.0,
            shown: false,
        })
    }

    /// `strength`, 0–1, is how much of the blur covers what is behind;
    /// the rest shows through sharp.
    pub fn set_shapes(&mut self, shapes: &[Shape], strength: f32) -> Result<()> {
        if self.shapes == shapes && self.strength == strength {
            return Ok(());
        }
        let children = self.root.Children()?;
        children.RemoveAll()?;
        for &(rect, radius) in shapes {
            let size = Vector2 {
                X: rect.w,
                Y: rect.h,
            };
            let geometry = self.compositor.CreateRoundedRectangleGeometry()?;
            geometry.SetSize(size)?;
            geometry.SetCornerRadius(Vector2 {
                X: radius,
                Y: radius,
            })?;
            let panel = self.compositor.CreateSpriteVisual()?;
            panel.SetOffset(Vector3 {
                X: rect.x,
                Y: rect.y,
                Z: 0.0,
            })?;
            panel.SetSize(size)?;
            panel.SetOpacity(strength)?;
            panel.SetClip(&self.compositor.CreateGeometricClipWithGeometry(&geometry)?)?;
            panel.SetBrush(&self.compositor.CreateHostBackdropBrush()?)?;
            children.InsertAtTop(&panel)?;
        }
        self.shapes = shapes.to_vec();
        self.strength = strength;
        Ok(())
    }

    pub fn place(&mut self, widget: HWND, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(widget),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.shown = true;
    }

    pub fn set_opacity(&self, opacity: f32) {
        let _ = self.root.SetOpacity(opacity);
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn hide(&mut self) {
        if std::mem::take(&mut self.shown) {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn backdrop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Composition needs a dispatcher queue on the window's thread, and only
/// one can exist per thread.
fn dispatcher_queue() -> Result<()> {
    thread_local! {
        static QUEUE: std::cell::OnceCell<DispatcherQueueController> = const { std::cell::OnceCell::new() };
    }
    QUEUE.with(|queue| {
        if queue.get().is_some() {
            return Ok(());
        }
        let controller = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?
        };
        let _ = queue.set(controller);
        Ok(())
    })
}
