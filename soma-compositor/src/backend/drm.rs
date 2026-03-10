/// DRM/KMS bare-metal backend for SomaOS.
///
/// Opens /dev/dri/card0, finds the first connected connector, picks the
/// preferred display mode, allocates a dumb buffer and memory-maps it.
/// Rendering: tiny-skia writes RGBA into a Vec, we convert and blit to
/// the dumb buffer, then call drmModeSetCrtc to flip the frame.
///
/// Input is handled separately via evdev (see ../input/evdev.rs).
use drm::control::{
    connector,
    crtc,
    dumbbuffer::DumbBuffer,
    Device as CtrlDevice,
};
use drm::Device;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};

// ── DRM card wrapper ────────────────────────────────────────────────────────

struct Card(File);

impl AsRawFd for Card {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl CtrlDevice for Card {}

// ── Public struct ───────────────────────────────────────────────────────────

pub struct DrmDisplay {
    card: Card,
    crtc_handle: crtc::Handle,
    connector_handle: connector::Handle,
    mode: drm::control::Mode,
    pub width: u32,
    pub height: u32,
    /// Front buffer (currently displayed)
    front: DumbBuffer,
    /// Back buffer (we render into this)
    back: DumbBuffer,
    /// Framebuffer handles for each dumb buffer
    front_fb: drm::control::framebuffer::Handle,
    back_fb: drm::control::framebuffer::Handle,
    /// Saved CRTC state so we can restore on exit
    saved_crtc: Option<drm::control::crtc::Info>,
}

impl DrmDisplay {
    /// Open the first DRM card and set it up for rendering.
    pub fn open() -> Result<Self, String> {
        // Try card0 first, then card1
        let card_path = if std::path::Path::new("/dev/dri/card0").exists() {
            "/dev/dri/card0"
        } else if std::path::Path::new("/dev/dri/card1").exists() {
            "/dev/dri/card1"
        } else {
            return Err("No DRM card found at /dev/dri/card0 or card1".to_string());
        };

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(card_path)
            .map_err(|e| format!("Cannot open {}: {}", card_path, e))?;

        let card = Card(file);

        // Acquire master rights (needed to set modes)
        card.acquire_master_lock()
            .map_err(|e| format!("Cannot acquire DRM master: {}", e))?;

        let res = card
            .resource_handles()
            .map_err(|e| format!("Cannot get DRM resources: {}", e))?;

        // Find first connected connector
        let connector_info = res
            .connectors()
            .iter()
            .filter_map(|&h| card.get_connector(h, true).ok())
            .find(|c| c.state() == connector::State::Connected)
            .ok_or("No connected display connector found")?;

        let connector_handle = connector_info.handle();

        // Pick preferred mode (first mode, which DRM lists as preferred)
        let mode = *connector_info
            .modes()
            .first()
            .ok_or("Connector has no modes")?;

        let width = mode.size().0 as u32;
        let height = mode.size().1 as u32;

        log::info!("DRM: connector {:?} at {}x{}", connector_handle, width, height);

        // Find encoder → CRTC
        let encoder_handle = connector_info
            .current_encoder()
            .ok_or("Connector has no encoder")?;
        let encoder_info = card
            .get_encoder(encoder_handle)
            .map_err(|e| format!("Cannot get encoder: {}", e))?;
        let crtc_handle = encoder_info
            .crtc()
            .ok_or("Encoder has no CRTC")?;

        // Save current CRTC so we can restore on exit
        let saved_crtc = card.get_crtc(crtc_handle).ok();

        // Create two dumb buffers (double buffering)
        let mut front = card
            .create_dumb_buffer((width, height), drm::buffer::DrmFourcc::Xrgb8888, 32)
            .map_err(|e| format!("Cannot create front dumb buffer: {}", e))?;
        let mut back = card
            .create_dumb_buffer((width, height), drm::buffer::DrmFourcc::Xrgb8888, 32)
            .map_err(|e| format!("Cannot create back dumb buffer: {}", e))?;

        // Create framebuffer objects
        let front_fb = card
            .add_framebuffer(&front, 24, 32)
            .map_err(|e| format!("Cannot add front framebuffer: {}", e))?;
        let back_fb = card
            .add_framebuffer(&back, 24, 32)
            .map_err(|e| format!("Cannot add back framebuffer: {}", e))?;

        // Set the CRTC to display the front buffer at our chosen mode
        card.set_crtc(
            crtc_handle,
            Some(front_fb),
            (0, 0),
            &[connector_handle],
            Some(mode),
        )
        .map_err(|e| format!("Cannot set CRTC: {}", e))?;

        Ok(Self {
            card,
            crtc_handle,
            connector_handle,
            mode,
            width,
            height,
            front,
            back,
            front_fb,
            back_fb,
            saved_crtc,
        })
    }

    /// Write a tiny-skia RGBA pixmap to the back buffer and page-flip.
    /// `rgba_pixels` must be exactly width × height × 4 bytes (RGBA8).
    pub fn present(&mut self, rgba_pixels: &[u8]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let expected = w * h * 4;
        if rgba_pixels.len() != expected {
            log::warn!("DRM present: pixel buf size {} != expected {}", rgba_pixels.len(), expected);
            return;
        }

        // Convert RGBA → XRGB8888 and write into back buffer
        {
            let mut back_map = self
                .card
                .map_dumb_buffer(&mut self.back)
                .map_err(|e| format!("Cannot map back buffer: {}", e))
                .ok();
            let Some(ref mut back_map) = back_map else {
                return;
            };
            let dst = back_map.as_mut();
            for i in 0..(w * h) {
                let r = rgba_pixels[i * 4] as u32;
                let g = rgba_pixels[i * 4 + 1] as u32;
                let b = rgba_pixels[i * 4 + 2] as u32;
                let xrgb = (r << 16) | (g << 8) | b;
                let off = i * 4;
                dst[off] = (xrgb & 0xFF) as u8;
                dst[off + 1] = ((xrgb >> 8) & 0xFF) as u8;
                dst[off + 2] = ((xrgb >> 16) & 0xFF) as u8;
                dst[off + 3] = 0;
            }
        }

        // Flip: set CRTC to display the back buffer
        if let Err(e) = self.card.set_crtc(
            self.crtc_handle,
            Some(self.back_fb),
            (0, 0),
            &[self.connector_handle],
            Some(self.mode),
        ) {
            log::warn!("DRM set_crtc failed: {}", e);
        }

        // Swap handles so next frame writes to the other buffer
        std::mem::swap(&mut self.front, &mut self.back);
        std::mem::swap(&mut self.front_fb, &mut self.back_fb);
    }
}

impl Drop for DrmDisplay {
    fn drop(&mut self) {
        // Restore saved CRTC
        if let Some(ref saved) = self.saved_crtc {
            let _ = self.card.set_crtc(
                self.crtc_handle,
                saved.framebuffer(),
                saved.position(),
                &[self.connector_handle],
                saved.mode(),
            );
        }
        // Clean up framebuffers and dumb buffers
        let _ = self.card.destroy_framebuffer(self.front_fb);
        let _ = self.card.destroy_framebuffer(self.back_fb);
        let _ = self.card.destroy_dumb_buffer(self.front);
        let _ = self.card.destroy_dumb_buffer(self.back);
    }
}
