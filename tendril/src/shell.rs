use std::os::unix::io::OwnedFd;

use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::delegate_compositor;
use smithay::delegate_data_device;
use smithay::delegate_seat;
use smithay::delegate_shm;
use smithay::delegate_xdg_decoration;
use smithay::delegate_xdg_shell;
use smithay::desktop::{PopupKind, Window as SmithayWindow};
use smithay::input::{pointer::CursorImageStatus, Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;
use smithay::reexports::wayland_server::protocol::wl_seat;
use smithay::utils::{Rectangle, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState,
};
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use wayland_server::protocol::{wl_buffer, wl_surface::WlSurface};
use wayland_server::Client;

use crate::state::{TendrilState, Window};

#[derive(Default)]
pub struct AppClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for AppClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

impl BufferHandler for TendrilState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for TendrilState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<AppClientState>()
            .unwrap()
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.popup_manager.commit(surface);
        self.needs_redraw = true;
        self.damage_full = true;
    }
}

impl XdgShellHandler for TendrilState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let smithay_window = SmithayWindow::new_wayland_window(surface);
        let window = Window::new(smithay_window, self.config.window_height);

        // add to the column with fewest windows (prefer higher index on tie)
        let ws = self.workspace_mut();
        let target = ws.columns
            .iter()
            .enumerate()
            .min_by_key(|(_, col)| col.windows.len())
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        ws.columns[target].windows.push(window);
        self.reconfigure_windows();
        self.needs_redraw = true;
        self.damage_full = true;
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        let output_size = self.backend.window_size();
        let target_rect = Rectangle::<i32, smithay::utils::Logical> {
            loc: (0, 0).into(),
            size: (output_size.w, output_size.h).into(),
        };
        let geometry = positioner.get_unconstrained_geometry(target_rect);

        surface.with_pending_state(|state| {
            state.geometry = geometry;
        });
        if let Err(e) = surface.send_configure() {
            log::warn!("failed to configure popup: {e:?}");
            return;
        }

        let kind = PopupKind::Xdg(surface);
        if let Err(e) = self.popup_manager.track_popup(kind) {
            log::warn!("failed to track popup: {e:?}");
        }

        self.needs_redraw = true;
        self.damage_full = true;
    }

    fn grab(&mut self, surface: PopupSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        let kind = PopupKind::Xdg(surface);
        let Ok(root_surface) = smithay::desktop::find_popup_root_surface(&kind) else { return };

        let Ok(popup_grab) = self.popup_manager.grab_popup::<Self>(
            root_surface,
            kind,
            &self.seat,
            serial,
        ) else {
            return;
        };

        if let Some(keyboard) = self.seat.get_keyboard() {
            use smithay::desktop::PopupKeyboardGrab;
            keyboard.set_grab(self, PopupKeyboardGrab::new(&popup_grab), serial);
        }
        if let Some(pointer) = self.seat.get_pointer() {
            use smithay::desktop::PopupPointerGrab;
            use smithay::input::pointer::Focus;
            pointer.set_grab(self, PopupPointerGrab::new(&popup_grab), serial, Focus::Keep);
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        let output_size = self.backend.window_size();
        let target_rect = Rectangle::<i32, smithay::utils::Logical> {
            loc: (0, 0).into(),
            size: (output_size.w, output_size.h).into(),
        };
        let geometry = positioner.get_unconstrained_geometry(target_rect);

        surface.with_pending_state(|state| {
            state.geometry = geometry;
        });
        surface.send_repositioned(token);
        self.needs_redraw = true;
    }
}

impl ShmHandler for TendrilState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SeatHandler for TendrilState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&Self::KeyboardFocus>) {}
    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {}
}

impl SelectionHandler for TendrilState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for TendrilState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for TendrilState {}
impl ServerDndGrabHandler for TendrilState {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl XdgDecorationHandler for TendrilState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_configure();
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: Mode) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_configure();
    }
}

delegate_compositor!(TendrilState);
delegate_xdg_shell!(TendrilState);
delegate_xdg_decoration!(TendrilState);
delegate_shm!(TendrilState);
delegate_seat!(TendrilState);
delegate_data_device!(TendrilState);
