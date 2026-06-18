use std::sync::atomic::{AtomicUsize, Ordering};

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::{self, WinitEvent};
use smithay::desktop::PopupManager;
use smithay::desktop::Window as SmithayWindow;
use smithay::input::keyboard::KeyboardHandle;
use smithay::input::pointer::PointerHandle;
use smithay::input::{Seat, SeatState};
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::{Point, Rectangle, Serial, Size, SERIAL_COUNTER};
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use wayland_server::protocol::wl_surface;

use crate::input;

static NEXT_WINDOW_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub window_height: u32,
    pub gaps: u32,
    pub visible_windows: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            window_height: 500,
            gaps: 1,
            visible_windows: 2,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                std::path::PathBuf::from(home).join(".config")
            });
        let path = config_dir.join("tendril").join("config.toml");

        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => {
                    log::info!("loaded config from {:?}", path);
                    config
                }
                Err(e) => {
                    log::warn!("failed to parse config at {:?}: {e}", path);
                    Config::default()
                }
            },
            Err(_) => {
                log::info!("no config file at {:?}, using defaults", path);
                Config::default()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Window {
    pub id: usize,
    pub toplevel: SmithayWindow,
    pub height: u32,
    pub z_index_offset: f32,
    pub z_anim_delay: f64,
}

impl Window {
    pub fn new(toplevel: SmithayWindow, height: u32) -> Self {
        let id = NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed);
        Window {
            id,
            toplevel,
            height,
            z_index_offset: 0.0,
            z_anim_delay: 0.0,
        }
    }

    pub fn y_position(&self, index: usize, config: &Config, scroll_offset: f64) -> f64 {
        window_y_position(index, self.height, config.gaps, scroll_offset)
    }

    #[allow(dead_code)]
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
        let total_h: f64 = self.windows.iter().map(|w| w.height as f64).sum();
        let n = self.windows.len() as f64;
        let g = config.gaps as f64;
        total_h + (n + 1.0) * g
    }

    pub fn max_scroll(&self, config: &Config, viewport_height: i32) -> f64 {
        let content = self.total_content_height(config);
        (content - viewport_height as f64).max(0.0)
    }

    pub fn clamp_scroll(&mut self, config: &Config, viewport_height: i32) {
        let max = self.max_scroll(config, viewport_height);
        self.target_scroll_offset = self.target_scroll_offset.clamp(0.0, max);
    }

    pub fn snap_scroll(&mut self, config: &Config, viewport_height: i32) {
        if self.windows.is_empty() {
            self.target_scroll_offset = 0.0;
            return;
        }
        let h = self.windows[0].height as f64;
        let g = config.gaps as f64;
        let stride = h + g;
        if stride <= 0.0 {
            return;
        }
        let snaps = (self.target_scroll_offset / stride).round();
        self.target_scroll_offset = snaps * stride;
        self.clamp_scroll(config, viewport_height);
    }

    pub fn snap_scroll_directional(&mut self, config: &Config, viewport_height: i32, delta: f64) {
        if self.windows.is_empty() {
            return;
        }
        let h = self.windows[0].height as f64;
        let g = config.gaps as f64;
        let stride = h + g;
        if stride <= 0.0 {
            return;
        }
        let snaps = if delta > 0.0 {
            (self.target_scroll_offset / stride).ceil()
        } else {
            (self.target_scroll_offset / stride).floor()
        };
        self.target_scroll_offset = snaps * stride;
        self.clamp_scroll(config, viewport_height);
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
        let h = self.windows.first().map(|w| w.height).unwrap_or(config.window_height);
        window_y_position(index, h, config.gaps, self.scroll_offset)
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
    gap as f64 + index as f64 * (height as f64 + gap as f64) - scroll_offset
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
    #[allow(dead_code)]
    pub decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    #[allow(dead_code)]
    pub seat: Seat<Self>,
    pub data_device_state: DataDeviceState,
    #[allow(dead_code)]
    pub display_handle: DisplayHandle,
    pub viewport_size: (u32, u32),
    pub cursor_pos: (f64, f64),
    pub mod_pressed: bool,
    pub shift_pressed: bool,
    pub keyboard_handle: Option<KeyboardHandle<Self>>,
    pub pointer_handle: Option<PointerHandle<Self>>,
    pub last_frame_time: Option<std::time::Instant>,
    pub damage_full: bool,
    pub popup_manager: PopupManager,
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
            last_frame_time: None,
            damage_full: true,
            popup_manager: PopupManager::default(),
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

    #[allow(dead_code)]
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
                self.damage_full = true;
            }
            WinitEvent::Redraw => {
                self.needs_redraw = true;
                self.damage_full = true;
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
        let n_visible = self.config.visible_windows.max(1);

        for left in [true, false] {
            let n = self.workspace().column(left).windows.len();
            if n == 0 {
                continue;
            }
            let h = (vp_h - gaps * (n_visible + 1)) / n_visible;

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
        SERIAL_COUNTER.next_serial()
    }

    pub fn focus_window(&mut self, id: usize) -> bool {
        for (ws_idx, ws) in self.workspaces.iter_mut().enumerate() {
            for left in [true, false] {
                let col = ws.column_mut(left);
                if let Some(idx) = col.windows.iter().position(|w| w.id == id) {
                    col.focused_idx = Some(idx);
                    self.active_workspace = ws_idx;
                    ws.active_column = left;
                    self.needs_redraw = true;
                    return true;
                }
            }
        }
        false
    }

    pub fn switch_workspace(&mut self, id: usize) -> bool {
        if id < self.workspaces.len() {
            self.active_workspace = id;
            self.needs_redraw = true;
            self.damage_full = true;
            true
        } else {
            false
        }
    }

    pub fn scroll_column(&mut self, left: bool, delta: f64) -> f64 {
        let config = self.config.clone();
        let vp_h = self.viewport_size.1 as i32;
        let col = if left {
            &mut self.workspace_mut().left_column
        } else {
            &mut self.workspace_mut().right_column
        };
        col.target_scroll_offset += delta;
        col.clamp_scroll(&config, vp_h);
        col.target_scroll_offset
    }

    pub fn set_config(&mut self, config: Config) {
        self.config = config;
        self.reconfigure_windows();
        self.needs_redraw = true;
        self.damage_full = true;
    }

    pub fn idle(&mut self) {
        self.popup_manager.cleanup();

        let now = std::time::Instant::now();
        let dt = self
            .last_frame_time
            .map(|t| (now - t).as_secs_f64())
            .unwrap_or(0.016);
        self.last_frame_time = Some(now);

        // Advance all scroll animations
        for ws in self.workspaces.iter_mut() {
            for col in [&mut ws.left_column, &mut ws.right_column] {
                let diff = (col.target_scroll_offset - col.scroll_offset).abs();
                if diff > 0.5 {
                    col.tick_scroll(10.0, dt);
                } else {
                    col.scroll_offset = col.target_scroll_offset;
                }
            }
        }

        // Tick z-effect animations
        let mut z_animating = false;
        for ws in self.workspaces.iter_mut() {
            for col in [&mut ws.left_column, &mut ws.right_column] {
                for w in col.windows.iter_mut() {
                    if w.z_anim_delay > 0.0 {
                        w.z_anim_delay -= dt;
                        if w.z_anim_delay <= 0.0 {
                            w.z_anim_delay = 0.0;
                        }
                    }
                    if w.z_anim_delay <= 0.0 && w.z_index_offset > 0.0 {
                        let target = 0.0;
                        let diff = target - w.z_index_offset;
                        if diff.abs() > 0.05 {
                            w.z_index_offset += diff * 10.0 * dt as f32;
                        } else {
                            w.z_index_offset = 0.0;
                        }
                        z_animating = true;
                    }
                }
            }
        }

        if !self.needs_redraw {
            let animating = self.workspaces.iter().any(|ws| {
                ws.left_column.scroll_offset != ws.left_column.target_scroll_offset
                    || ws.right_column.scroll_offset != ws.right_column.target_scroll_offset
            });
            if !animating && !z_animating {
                return;
            }
        }
        self.needs_redraw = false;

        let size = self.backend.window_size();
        let damage = if self.damage_full {
            self.damage_full = false;
            vec![Rectangle::from_size(size)]
        } else {
            let col_width = (size.w as u32 - self.config.gaps) / 2;
            let gaps = self.config.gaps as i32;
            vec![
                Rectangle::new(Point::new(0, 0), Size::new(col_width as i32 + gaps, size.h)),
                Rectangle::new(
                    Point::new(col_width as i32, 0),
                    Size::new(col_width as i32 + gaps, size.h),
                ),
            ]
        };

        let col_width = (size.w as u32 - self.config.gaps) / 2;
        let vp_height = size.h;
        let gaps = self.config.gaps;

        let mut all_windows: Vec<wl_surface::WlSurface> = Vec::new();

        let mut entries: Vec<(i32, i32, f64, f32, wl_surface::WlSurface)> = Vec::new();

        for left in [true, false] {
            let ws = self.workspace();
            let col = if left { &ws.left_column } else { &ws.right_column };
            let base_x = if left { gaps as i32 } else { col_width as i32 + gaps as i32 };

            for (i, y) in col.windows_visible(&self.config, vp_height) {
                let z = col.windows[i].z_index_offset;
                let scale = if z > 0.0 { 1.05 } else { 1.0 };
                if let Some(surface) = col.windows[i].toplevel.wl_surface() {
                    let s = wl_surface::WlSurface::clone(&*surface);
                    entries.push((base_x, y as i32, scale, z, s));
                }
            }

            for w in col.windows.iter() {
                if let Some(surface) = w.toplevel.wl_surface() {
                    all_windows.push(wl_surface::WlSurface::clone(&*surface));
                }
            }
        }

        // Add popup surfaces on top of their parent toplevels
        for left in [true, false] {
            let ws = self.workspace();
            let col = if left { &ws.left_column } else { &ws.right_column };
            let base_x = if left { gaps as i32 } else { col_width as i32 + gaps as i32 };

            for (i, y) in col.windows_visible(&self.config, vp_height) {
                if let Some(surface) = col.windows[i].toplevel.wl_surface() {
                    for (popup, offset) in PopupManager::popups_for_surface(&surface) {
                        let popup_surface = popup.wl_surface().clone();
                        let pos = (base_x + offset.x, y as i32 + offset.y);
                        entries.push((pos.0, pos.1, 1.0, 100.0, popup_surface.clone()));
                        all_windows.push(popup_surface);
                    }
                }
            }
        }

        entries.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));

        crate::render::render_frame(&mut self.backend, size, &damage, &entries, &all_windows);
    }
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
        assert_eq!(window_y_position(0, 500, 8, 0.0), 8.0);
        assert_eq!(window_y_position(1, 500, 8, 0.0), 516.0);
        assert_eq!(window_y_position(2, 500, 8, 0.0), 1024.0);
        assert_eq!(window_y_position(0, 500, 8, 200.0), -192.0);
        assert_eq!(window_y_position(1, 500, 8, 200.0), 316.0);
        assert_eq!(window_y_position(2, 500, 8, 200.0), 824.0);
    }

    #[test]
    fn test_total_content_height() {
        let config = Config {
            window_height: 500,
            gaps: 8,
            visible_windows: 2,
        };
        let mut col = Column::new();
        assert_eq!(col.total_content_height(&config), 0.0);

        col.windows.push(Window::new(dummy_window(), 500));
        assert_eq!(col.total_content_height(&config), 516.0);

        col.windows.push(Window::new(dummy_window(), 500));
        assert_eq!(col.total_content_height(&config), 1024.0);

        col.windows.push(Window::new(dummy_window(), 500));
        assert_eq!(col.total_content_height(&config), 1532.0);
    }

    #[test]
    fn test_scroll_clamping() {
        let config = Config {
            window_height: 500,
            gaps: 8,
            visible_windows: 2,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }

        col.target_scroll_offset = 1000.0;
        col.clamp_scroll(&config, 1000);
        assert_eq!(col.target_scroll_offset, 532.0);

        col.target_scroll_offset = -100.0;
        col.clamp_scroll(&config, 1000);
        assert_eq!(col.target_scroll_offset, 0.0);
    }

    #[test]
    fn test_geometry_3_windows_scrolled_200() {
        let config = Config {
            window_height: 500,
            gaps: 8,
            visible_windows: 2,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        col.scroll_offset = 200.0;

        let visible = col.windows_visible(&config, 1000);
        // stride = 508
        // y0 = 8 + 0*508 - 200 = -192, bottom = 308  -> visible
        // y1 = 8 + 1*508 - 200 = 316,  bottom = 816  -> visible
        // y2 = 8 + 2*508 - 200 = 824,  bottom = 1324 -> visible
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn test_exact_positions() {
        let config = Config {
            window_height: 500,
            gaps: 8,
            visible_windows: 2,
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

        assert!((positions[0].1 - (-192.0)).abs() < 0.001);
        assert!((positions[0].2 - 308.0).abs() < 0.001);
        assert!((positions[1].1 - 316.0).abs() < 0.001);
        assert!((positions[1].2 - 816.0).abs() < 0.001);
        assert!((positions[2].1 - 824.0).abs() < 0.001);
        assert!((positions[2].2 - 1324.0).abs() < 0.001);

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
            visible_windows: 2,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        col.scroll_offset = 200.0;

        assert!((col.y_position(0, &config) - (-192.0)).abs() < 0.001);
        assert!((col.y_position(1, &config) - 316.0).abs() < 0.001);
        assert!((col.y_position(2, &config) - 824.0).abs() < 0.001);
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
    fn test_snap_scroll() {
        let config = Config {
            window_height: 500,
            gaps: 8,
            visible_windows: 2,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        // stride = 508; max_scroll for vp=1000: total=1532, max=532
        // snap points: 0, 508
        // 508 exceeds max (532) so 508 is fine; 1016 exceeds max

        col.target_scroll_offset = 300.0;
        col.snap_scroll(&config, 1000);
        assert_eq!(col.target_scroll_offset, 508.0);

        col.target_scroll_offset = 200.0;
        col.snap_scroll(&config, 1000);
        assert_eq!(col.target_scroll_offset, 0.0);

        col.target_scroll_offset = 250.0;
        col.snap_scroll(&config, 1000);
        // 250/508 = 0.492 -> round = 0 -> 0
        assert_eq!(col.target_scroll_offset, 0.0);

        col.target_scroll_offset = 260.0;
        col.snap_scroll(&config, 1000);
        // 260/508 = 0.512 -> round = 1 -> 508
        assert_eq!(col.target_scroll_offset, 508.0);
    }

    #[test]
    fn test_snap_scroll_directional() {
        let config = Config {
            window_height: 500,
            gaps: 8,
            visible_windows: 2,
        };
        let mut col = Column::new();
        for _ in 0..3 {
            col.windows.push(Window::new(dummy_window(), 500));
        }
        // stride = 508; max_scroll = 532
        col.target_scroll_offset = 0.0;
        // positive delta: ceil(0/508) = 0 -> target = 0 (already at snap 0)
        // but we call it after target_scroll_offset is already set, using the delta
        col.target_scroll_offset = 10.0;
        col.snap_scroll_directional(&config, 1000, 10.0);
        // ceil(10/508) = 1 -> target = 508
        assert_eq!(col.target_scroll_offset, 508.0);

        col.target_scroll_offset = 500.0;
        col.snap_scroll_directional(&config, 1000, -10.0);
        // floor(500/508) = 0 -> target = 0
        assert_eq!(col.target_scroll_offset, 0.0);

        // small positive delta: should ceil
        col.target_scroll_offset = 1.0;
        col.snap_scroll_directional(&config, 1000, 1.0);
        assert_eq!(col.target_scroll_offset, 508.0);

        // negative delta from zero: floor stays at 0
        col.target_scroll_offset = 0.0;
        col.snap_scroll_directional(&config, 1000, -1.0);
        assert_eq!(col.target_scroll_offset, 0.0);
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
