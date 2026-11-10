//! Windows Graphics Capture: delivers the window's frames as GPU textures.

use super::{Ctx, Shared};
use crate::{Result, Settings};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use windows::core::{factory, IInspectable, Interface};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11Multithread, ID3D11Texture2D, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// One D3D11 device shared by capture and the encoder so frames never leave the GPU.
pub(crate) fn create_device() -> Result<ID3D11Device> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .ctx("create D3D11 device")?;
    }
    let device = device.expect("D3D11CreateDevice succeeded without a device");
    // The capture callback, the encoder and the audio thread all touch the device.
    let mt: ID3D11Multithread = device.cast().ctx("enable D3D multithread protection")?;
    unsafe {
        let _ = mt.SetMultithreadProtected(true);
    }
    Ok(device)
}

/// The capture device handle as used from the frame callback. Direct3D
/// devices are free-threaded; the wrapper only tells the compiler so.
struct SharedDevice(IDirect3DDevice);
unsafe impl Send for SharedDevice {}
unsafe impl Sync for SharedDevice {}
impl SharedDevice {
    fn get(&self) -> &IDirect3DDevice {
        &self.0
    }
}

/// A capturable window, opened but not yet started.
pub(crate) struct Source {
    item: GraphicsCaptureItem,
    winrt_device: IDirect3DDevice,
    size: SizeInt32,
}

impl Source {
    pub fn open(device: &ID3D11Device, hwnd: HWND) -> Result<Self> {
        let dxgi: IDXGIDevice = device.cast().ctx("get DXGI device")?;
        let inspectable: IInspectable =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.ctx("wrap D3D device for capture")?;
        let winrt_device: IDirect3DDevice = inspectable.cast().ctx("wrap D3D device for capture")?;

        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ctx("capture interop")?;
        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForWindow(hwnd) }.ctx("create capture item for window")?;
        let size = item.Size().ctx("read window size")?;
        Ok(Source { item, winrt_device, size })
    }

    /// Window size rounded down to even numbers, as H.264 requires.
    pub fn even_size(&self) -> (u32, u32) {
        ((self.size.Width.max(2) as u32) & !1, (self.size.Height.max(2) as u32) & !1)
    }

    pub fn start(self, shared: Arc<Shared>, settings: &Settings) -> Result<Capture> {
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &self.winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            self.size,
        )
        .ctx("create capture frame pool")?;
        let session = pool.CreateCaptureSession(&self.item).ctx("create capture session")?;

        // Optional features that depend on the Windows version; failures are fine.
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(settings.capture_cursor);
        let interval = 10_000_000 / settings.fps.max(1) as i64;
        let _ = session.SetMinUpdateInterval(TimeSpan { Duration: interval });

        let pacing = Mutex::new(Pacing { interval, next_due: 0, pool_size: self.size });
        let winrt_device = SharedDevice(self.winrt_device.clone());
        let frame_shared = shared.clone();
        pool.FrameArrived(&TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
            move |pool, _| {
                if let Some(pool) = pool.as_ref() {
                    on_frame(pool, winrt_device.get(), &frame_shared, &pacing);
                }
                Ok(())
            },
        ))
        .ctx("subscribe to frames")?;

        let closed_shared = shared.clone();
        self.item
            .Closed(&TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                closed_shared.window_closed.store(true, Ordering::SeqCst);
                Ok(())
            }))
            .ctx("subscribe to window close")?;

        session.StartCapture().ctx("start capture")?;
        Ok(Capture { pool, session, _item: self.item })
    }
}

struct Pacing {
    /// Target frame interval, 100 ns units.
    interval: i64,
    /// Earliest timestamp the next frame should have.
    next_due: i64,
    pool_size: SizeInt32,
}

fn on_frame(
    pool: &Direct3D11CaptureFramePool,
    device: &IDirect3DDevice,
    shared: &Shared,
    pacing: &Mutex<Pacing>,
) {
    // Always take the frame so the pool does not fill up.
    let Ok(frame) = pool.TryGetNextFrame() else { return };
    if shared.stop.load(Ordering::Relaxed) {
        return;
    }
    let Ok(time) = frame.SystemRelativeTime() else { return };
    let pts = time.Duration - shared.start;
    if pts < 0 {
        return;
    }

    let content = frame.ContentSize().unwrap_or_default();
    {
        let mut p = pacing.lock().unwrap();
        // The window was resized: make the pool match so frames are not cropped by it.
        let resized = content.Width > 0
            && content.Height > 0
            && (content.Width != p.pool_size.Width || content.Height != p.pool_size.Height);
        if resized && pool.Recreate(device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, content).is_ok() {
            p.pool_size = content;
        }
        // Drop frames that arrive faster than the output frame rate.
        if pts < p.next_due - p.interval / 4 {
            shared.frames_paced_out.fetch_add(1, Ordering::Relaxed);
            return;
        }
        p.next_due += p.interval;
        if p.next_due < pts {
            p.next_due = pts + p.interval;
        }
    }

    let texture: windows::core::Result<ID3D11Texture2D> = frame
        .Surface()
        .and_then(|s| s.cast::<IDirect3DDxgiInterfaceAccess>())
        .and_then(|a| unsafe { a.GetInterface() });
    let Ok(texture) = texture else { return };

    let interval = pacing.lock().unwrap().interval;
    match shared.encoder.write_video(&texture, content.Width.max(0) as u32, content.Height.max(0) as u32, pts, interval) {
        Ok(true) => {
            shared.frames_written.fetch_add(1, Ordering::Relaxed);
        }
        _ => {
            shared.frames_dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// A running capture session.
pub(crate) struct Capture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    _item: GraphicsCaptureItem,
}

impl Capture {
    pub fn close(&self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
