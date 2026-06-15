use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::{Color32F, Frame, Renderer};
use smithay::backend::winit::{self, WinitEvent};
use smithay::utils::{Rectangle, Transform};

pub struct TendrilState {
    pub backend: winit::WinitGraphicsBackend<GlesRenderer>,
    pub needs_redraw: bool,
}

impl TendrilState {
    pub fn new(backend: winit::WinitGraphicsBackend<GlesRenderer>) -> Self {
        TendrilState {
            backend,
            needs_redraw: false,
        }
    }

    pub fn handle_event(&mut self, event: WinitEvent) {
        match event {
            WinitEvent::CloseRequested => {
                log::info!("close requested, shutting down");
                std::process::exit(0);
            }
            WinitEvent::Resized { size, scale_factor } => {
                log::debug!("window resized to {size:?} (scale: {scale_factor})");
            }
            WinitEvent::Redraw => {
                self.needs_redraw = true;
            }
            WinitEvent::Focus(_) => {}
            WinitEvent::Input(_) => {}
        }
    }

    pub fn idle(&mut self) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        let size = self.backend.window_size();
        let damage = Rectangle::from_size(size);

        let result = {
            let (renderer, mut framebuffer) = match self.backend.bind() {
                Ok(pair) => pair,
                Err(e) => {
                    log::error!("backend bind failed: {e:?}");
                    return;
                }
            };

            let mut frame = match renderer.render(&mut framebuffer, size, Transform::Flipped180) {
                Ok(f) => f,
                Err(e) => {
                    log::error!("renderer start failed: {e:?}");
                    return;
                }
            };

            if let Err(e) = frame.clear(Color32F::new(0.1, 0.1, 0.2, 1.0), &[damage]) {
                log::error!("clear failed: {e:?}");
            }

            frame.finish()
        };

        if let Err(e) = result {
            log::error!("frame finish failed: {e:?}");
        }

        if let Err(e) = self.backend.submit(Some(&[damage])) {
            log::error!("backend submit failed: {e:?}");
        }
    }
}
