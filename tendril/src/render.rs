use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::draw_render_elements;
use smithay::backend::renderer::{Color32F, Frame, Renderer};
use smithay::backend::winit;
use smithay::utils::{Physical, Rectangle, Transform};
use wayland_server::protocol::wl_surface;

use smithay::wayland::compositor::{
    with_surface_tree_downward, SurfaceAttributes, TraversalAction,
};

pub fn render_frame(
    backend: &mut winit::WinitGraphicsBackend<GlesRenderer>,
    size: smithay::utils::Size<i32, Physical>,
    damage: &[Rectangle<i32, Physical>],
    entries: &[(i32, i32, f64, f32, wl_surface::WlSurface)],
    all_windows: &[wl_surface::WlSurface],
) {
    let start = std::time::Instant::now();

    let result = {
        let (renderer, mut framebuffer) = match backend.bind() {
            Ok(pair) => pair,
            Err(e) => {
                log::error!("backend bind failed: {e:?}");
                return;
            }
        };

        let mut elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
        for (x, y, scale, _, surface) in entries {
            elements.extend(render_elements_from_surface_tree(
                renderer,
                surface,
                (*x, *y),
                *scale,
                1.0,
                Kind::Unspecified,
            ));
        }

        let mut frame = match renderer.render(&mut framebuffer, size, Transform::Flipped180) {
            Ok(f) => f,
            Err(e) => {
                log::error!("renderer start failed: {e:?}");
                return;
            }
        };

        if let Err(e) = frame.clear(Color32F::new(0.1, 0.1, 0.2, 1.0), damage) {
            log::error!("clear failed: {e:?}");
        }

        if let Err(e) = draw_render_elements(&mut frame, 1.0, &elements, damage) {
            log::error!("draw elements failed: {e:?}");
        }

        let time = start.elapsed().as_millis() as u32;

        for surface in all_windows {
            send_frames_surface_tree(surface, time);
        }

        frame.finish()
    };

    if let Err(e) = result {
        log::error!("frame finish failed: {e:?}");
    }

    if let Err(e) = backend.submit(Some(damage)) {
        log::error!("backend submit failed: {e:?}");
    }
}

fn send_frames_surface_tree(surface: &wl_surface::WlSurface, time: u32) {
    with_surface_tree_downward(
        surface,
        (),
        |_, _, &()| TraversalAction::DoChildren(()),
        |_surf, states, &()| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time);
            }
        },
        |_, _, &()| true,
    );
}
