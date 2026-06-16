#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};

use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::draw_render_elements;
use smithay::backend::renderer::{Color32F, Frame, Renderer};
use smithay::backend::winit::{self, WinitEvent};
use smithay::desktop::Window as SmithayWindow;
use smithay::input::keyboard::KeyboardHandle;
use smithay::input::pointer::PointerHandle;
use smithay::input::{Seat, SeatState};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::{Rectangle, Serial, Transform};
use smithay::wayland::compositor::{
    with_surface_tree_downward, SurfaceAttributes, TraversalAction,
};
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use wayland_server::protocol::wl_surface;

use crate::input;

static NEXT_WINDOW_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct Config {
    pub window_height: u32,
    pub gaps: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window_height: 500,
            gaps: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Window {
    pub id: usize,
    pub toplevel: SmithayWindow,
    pub height: u32,
    pub z_index_offset: f32,
}

impl Window {
    pub fn new(toplevel: SmithayWindow, height: u32) -> Self {
        let id = NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
        Window {
            id,
            toplevel,
            height,
            z_index_offset: 0.0,
        }
    }

    pub fn y_position(&self, index: usize, config: &Config, scroll_offset: f64) -> f64 {
        window_y_position(index, self.height, config.gaps, scroll_offset)
    }

    pub fn is_visible(
        &self,
        index: usize,
        config: &Config,
        scroll_offset: f64,
        viewport_height: i32,
    ) -> bool {
        let y = self.y_position(index, config, scroll_offset);
        let bottom = y + self.height as f64;
        bottom > 0.0 && y < viewport_height as f64
    }
}

#[derive(Debug, Clone)]
pub struct Column {
    pub windows: Vec<Window>,
    pub scroll_offset: f64,
    pub target_scroll_offset: f64,
    pub focused_idx: Option<usize>,
}

impl Column {
    pub fn new() -> Self {
        Column {
            windows: Vec::new(),
            scroll_offset: 0.0,
            target_scroll_offset: 0.0,
            focused_idx: None,
        }
    }

    pub fn total_content_height(&self, config: &Config) -> f64 {
        if self.windows.is_empty() {
            return 0.0;
        }
        let n = self.windows.len() as f64;
        let h = config.window_height as f64;
        let g = config.gaps as f64;
        n * h + (n - 1.0) * g
    }

    pub fn max_scroll(&self, config: &Config, viewport_height: i32) -> f64 {
        let content = self.total_content_height(config);
        (content - viewport_height as f64).max(0.0)
    }

    pub fn clamp_scroll(&mut self, config: &Config, viewport_height: i32) {
        let max = self.max_scroll(config, viewport_height);
        self.target_scroll_offset = self.target_scroll_offset.clamp(0.0, max);
    }

    pub fn tick_scroll(&mut self, speed: f64, dt: f64) {
        let diff = self.target_scroll_offset - self.scroll_offset;
        if diff.abs() > 0.5 {
            self.scroll_offset += diff * speed * dt;
        } else {
            self.scroll_offset = self.target_scroll_offset;
        }
    }

    pub fn y_position(&self, index: usize, config: &Config) -> f64 {
        window_y_position(index, config.window_height, config.gaps, self.scroll_offset)
    }

    pub fn windows_visible(
        &self,
        config: &Config,
        viewport_height: i32,
    ) -> Vec<(usize, f64)> {
        self.windows
            .iter()
            .enumerate()
            .filter_map(|(i, w)| {
                let y = self.y_position(i, config);
                let bottom = y + w.height as f64;
                if bottom > 0.0 && y < viewport_height as f64 {
                    Some((i, y))
                } else {
                    None
                }
            })
            .collect()
    }
}

pub fn window_y_position(index: usize, height: u32, gap: u32, scroll_offset: f64) -> f64 {
    index as f64 * (height as f64 + gap as f64) - scroll_offset
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: u8,
    pub left_column: Column,
    pub right_column: Column,
    pub active_column: bool,
}

impl Workspace {
    pub fn new(id: u8) -> Self {
        Workspace {
            id,
            left_column: Column::new(),
            right_column: Column::new(),
            active_column: true,
        }
    }

    pub fn column_mut(&mut self, left: bool) -> &mut Column {
        if left {
            &mut self.left_column
        } else {
            &mut self.right_column
        }
    }

    pub fn column(&self, left: bool) -> &Column {
        if left {
            &self.left_column
        } else {
            &self.right_column
        }
    }
}

pub struct TendrilState {
    pub backend: winit::WinitGraphicsBackend<GlesRenderer>,
    pub needs_redraw: bool,
    pub workspaces: Vec<Workspace>,
    pub active_workspace: usize,
    pub config: Config,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub seat: Seat<Self>,
    pub data_device_state: DataDeviceState,
    pub display_handle: DisplayHandle,
    pub viewport_size: (u32, u32),
    pub cursor_pos: (f64, f64),
    pub mod_pressed: bool,
    pub shift_pressed: bool,
    pub keyboard_handle: Option<KeyboardHandle<Self>>,
    pub pointer_handle: Option<PointerHandle<Self>>,
}

impl TendrilState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: winit::WinitGraphicsBackend<GlesRenderer>,
        compositor_state: CompositorState,
        xdg_shell_state: XdgShellState,
        decoration_state: XdgDecorationState,
        shm_state: ShmState,
        seat_state: SeatState<Self>,
        seat: Seat<Self>,
        data_device_state: DataDeviceState,
        display_handle: DisplayHandle,
        keyboard_handle: Option<KeyboardHandle<Self>>,
        pointer_handle: Option<PointerHandle<Self>>,
    ) -> Self {
        let size = backend.window_size();
        let workspaces = (0..5).map(Workspace::new).collect();
        TendrilState {
            backend,
            needs_redraw: false,
            workspaces,
            active_workspace: 0,
            config: Config::default(),
            compositor_state,
            xdg_shell_state,
            decoration_state,
            shm_state,
            seat_state,
            seat,
            data_device_state,
            display_handle,
            viewport_size: (size.w as u32, size.h as u32),
            cursor_pos: (0.0, 0.0),
            mod_pressed: false,
            shift_pressed: false,
            keyboard_handle,
            pointer_handle,
        }
    }

    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace]
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace]
    }

    pub fn active_column_mut(&mut self) -> &mut Column {
        let ws = self.workspace_mut();
        if ws.active_column {
            &mut ws.left_column
        } else {
            &mut ws.right_column
        }
    }

    pub fn active_column(&self) -> &Column {
        let ws = self.workspace();
        if ws.active_column {
            &ws.left_column
        } else {
            &ws.right_column
        }
    }

    pub fn handle_event(&mut self, event: WinitEvent) {
        match event {
            WinitEvent::CloseRequested => {
                log::info!("close requested, shutting down");
                std::process::exit(0);
            }
            WinitEvent::Resized { size, .. } => {
                self.viewport_size = (size.w as u32, size.h as u32);
                self.reconfigure_windows();
                self.needs_redraw = true;
            }
            WinitEvent::Redraw => {
                self.needs_redraw = true;
            }
            WinitEvent::Focus(_) => {}
            WinitEvent::Input(event) => {
                input::handle_input(self, event);
            }
        }
    }

    pub fn reconfigure_windows(&mut self) {
        let col_width = (self.viewport_size.0 - self.config.gaps) / 2;
        let vp_h = self.viewport_size.1;
        let gaps = self.config.gaps;

        for left in [true, false] {
            let n = self.workspace().column(left).windows.len();
            if n == 0 {
                continue;
            }
            let h = (vp_h - gaps * (n as u32 + 1)) / n as u32;

            let idxs: Vec<usize> = (0..n).collect();
            let configures: Vec<_> = idxs.into_iter().map(|idx| {
                    let ws = self.workspace_mut();
                    let col = ws.column_mut(left);
                    col.windows[idx].height = h;
                    let toplevel = col.windows[idx].toplevel.toplevel().cloned();
                    (toplevel, h)
                })
                .collect();

            for (tl, h) in configures {
                if let Some(tl) = tl {
                    tl.with_pending_state(|state| {
                        state.size = Some((col_width as i32, h as i32).into());
                    });
                    tl.send_configure();
                }
            }
        }
    }

    pub fn window_at(&self, x: f64, y: f64) -> Option<(bool, usize)> {
        let col_width = (self.viewport_size.0 - self.config.gaps) / 2;
        let gaps = self.config.gaps as f64;
        for left in [true, false] {
            let col = self.workspace().column(left);
            let col_x = if left { gaps } else { col_width as f64 + gaps };
            for (i, w) in col.windows.iter().enumerate() {
                let wy = w.y_position(i, &self.config, col.scroll_offset);
                if x >= col_x
                    && x < col_x + col_width as f64
                    && y >= wy
                    && y < wy + w.height as f64
                {
                    return Some((left, i));
                }
            }
        }
        None
    }

    pub fn serial(&self) -> Serial {
        Serial::from(0)
    }

    pub fn idle(&mut self) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        let size = self.backend.window_size();
        let damage = Rectangle::from_size(size);

        let col_width = (size.w as u32 - self.config.gaps) / 2;
        let vp_height = size.h;
        let gaps = self.config.gaps;

        let left_surfaces: Vec<(i32, i32, wl_surface::WlSurface)> = self
            .workspace()
            .left_column
            .windows_visible(&self.config, vp_height)
            .into_iter()
            .filter_map(|(i, y)| {
                let surface = self.workspace().left_column.windows[i]
                    .toplevel
                    .wl_surface()?;
                let x = gaps as i32;
                Some((x, y as i32, wl_surface::WlSurface::clone(&*surface)))
            })
            .collect();

        let right_surfaces: Vec<(i32, i32, wl_surface::WlSurface)> = self
            .workspace()
            .right_column
            .windows_visible(&self.config, vp_height)
            .into_iter()
            .filter_map(|(i, y)| {
                let surface = self.workspace().right_column.windows[i]
                    .toplevel
                    .wl_surface()?;
                let x = col_width as i32 + gaps as i32;
                Some((x, y as i32, wl_surface::WlSurface::clone(&*surface)))
            })
            .collect();

        let all_windows: Vec<wl_surface::WlSurface> = self
            .workspace()
            .left_column
            .windows
            .iter()
            .chain(self.workspace().right_column.windows.iter())
            .filter_map(|w| {
                let surface = w.toplevel.wl_surface()?;
                Some(wl_surface::WlSurface::clone(&*surface))
            })
            .collect();

        let start = std::time::Instant::now();

        let result = {
            let (renderer, mut framebuffer) = match self.backend.bind() {
                Ok(pair) => pair,
                Err(e) => {
                    log::error!("backend bind failed: {e:?}");
                    return;
                }
            };

            let mut elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
            for (x, y, surface) in left_surfaces {
                elements.extend(render_elements_from_surface_tree(
                    renderer,
                    &surface,
                    (x, y),
                    1.0,
                    1.0,
                    Kind::Unspecified,
                ));
            }
            for (x, y, surface) in right_surfaces {
                elements.extend(render_elements_from_surface_tree(
                    renderer,
                    &surface,
                    (x, y),
                    1.0,
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

            if let Err(e) = frame.clear(Color32F::new(0.1, 0.1, 0.2, 1.0), &[damage]) {
                log::error!("clear failed: {e:?}");
            }

            if let Err(e) = draw_render_elements(&mut frame, 1.0, &elements, &[damage]) {
                log::error!("draw elements failed: {e:?}");
            }

            let time = start.elapsed().as_millis() as u32;

            for surface in &all_windows {
                send_frames_surface_tree(surface, time);
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

pub fn send_frames_surface_tree(surface: &wl_surface::WlSurface, time: u32) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn dummy_window() -> SmithayWindow {
        use std::sync::atomic::AtomicUsize;

        let arc = Arc::new(());
        let data_ptr: *const () = Arc::into_raw(arc);
        let offset = std::mem::size_of::<AtomicUsize>() * 2;
        let inner_ptr = unsafe { (data_ptr as *const u8).sub(offset) as *const () };
        let sw: SmithayWindow = unsafe { std::mem::transmute(inner_ptr) };
        let clone = sw.clone();
        std::mem::forget(sw);
        clone
    }

    #[test]
    fn test_window_y_position_free_func() {
        assert_eq!(window_y_position(0, 500, 8, 0.0), 0.0);
        assert_eq!(window_y_position(1, 500, 8, 0.0), 508.0);
        assert_eq!(window_y_position(2, 500, 8, 0.0), 1016.0);
        assert_eq!(window_y_position(0, 500, 8, 200.0), -200.0);
        assert_eq!(window_y_position(1, 500, 8, 200.0), 308.0);
        assert_eq!(window_y_position(2, 500, 8, 200.0), 816.0);
    }

    #[test]
    fn test_total_content_height() {
        let config = Config {
            window_height: 500,
            gaps: 8,
        };
        let mut col = Column::new();
        assert_eq!(col.total_content_height(&config), 0.0);

        col.windows.push(Window::new(dummy_window(), 500));
        assert_eq!(col.total_content_height(&config), 500.0);

        col.windows.push(Window::new(dummy_window(), 500));
        assert_eq!(col.total_content_height(&config), 1008.0);

        col.windows.push(Window::new(dummy_window(), 500));
        assert_eq!(col.total_content_height(&config), 1516.0);
    }

    #[test]
    fn test_scroll_clamping() {
        let config = Config {
            window_height: 500,
            gaps: 8,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }

        col.target_scroll_offset = 1000.0;
        col.clamp_scroll(&config, 1000);
        assert_eq!(col.target_scroll_offset, 516.0);

        col.target_scroll_offset = -100.0;
        col.clamp_scroll(&config, 1000);
        assert_eq!(col.target_scroll_offset, 0.0);
    }

    #[test]
    fn test_geometry_3_windows_scrolled_200() {
        let config = Config {
            window_height: 500,
            gaps: 8,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        col.scroll_offset = 200.0;

        let visible = col.windows_visible(&config, 1000);
        // stride = 508
        // y0 = 0*508 - 200 = -200, bottom = 300  -> visible
        // y1 = 1*508 - 200 = 308,  bottom = 808  -> visible
        // y2 = 2*508 - 200 = 816,  bottom = 1316 -> visible
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn test_exact_positions() {
        let config = Config {
            window_height: 500,
            gaps: 8,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        col.scroll_offset = 200.0;

        let positions: Vec<(usize, f64, f64)> = col
            .windows
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let y = w.y_position(i, &config, col.scroll_offset);
                let bottom = y + w.height as f64;
                (i, y, bottom)
            })
            .collect();

        assert!((positions[0].1 - (-200.0)).abs() < 0.001);
        assert!((positions[0].2 - 300.0).abs() < 0.001);
        assert!((positions[1].1 - 308.0).abs() < 0.001);
        assert!((positions[1].2 - 808.0).abs() < 0.001);
        assert!((positions[2].1 - 816.0).abs() < 0.001);
        assert!((positions[2].2 - 1316.0).abs() < 0.001);

        let visible: Vec<bool> = col
            .windows
            .iter()
            .enumerate()
            .map(|(i, w)| w.is_visible(i, &config, col.scroll_offset, 1000))
            .collect();
        assert_eq!(visible, vec![true, true, true]);
    }

    #[test]
    fn test_column_y_position_formula() {
        let config = Config {
            window_height: 500,
            gaps: 8,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        col.scroll_offset = 200.0;

        assert!((col.y_position(0, &config) - (-200.0)).abs() < 0.001);
        assert!((col.y_position(1, &config) - 308.0).abs() < 0.001);
        assert!((col.y_position(2, &config) - 816.0).abs() < 0.001);
    }

    #[test]
    fn test_window_ids_monotonic() {
        let w1 = Window::new(dummy_window(), 500);
        let w2 = Window::new(dummy_window(), 500);
        let w3 = Window::new(dummy_window(), 500);
        assert!(w1.id < w2.id);
        assert!(w2.id < w3.id);
    }

    #[test]
    fn test_workspace_columns() {
        let mut ws = Workspace::new(0);
        assert!(ws.active_column);
        ws.column_mut(true)
            .windows
            .push(Window::new(dummy_window(), 500));
        assert_eq!(ws.column(true).windows.len(), 1);
        assert!(ws.column(false).windows.is_empty());
    }
}
