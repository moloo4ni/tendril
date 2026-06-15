use std::os::unix::io::OwnedFd;

use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::delegate_compositor;
use smithay::delegate_data_device;
use smithay::delegate_seat;
use smithay::delegate_shm;
use smithay::delegate_xdg_shell;
use smithay::desktop::Window as SmithayWindow;
use smithay::input::{pointer::CursorImageStatus, Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_seat;
use smithay::utils::Serial;
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    CompositorClientState, CompositorHandler, CompositorState,
};
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
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
        self.needs_redraw = true;
    }
}

impl XdgShellHandler for TendrilState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let size = self.backend.window_size();
        let col_width = (size.w as u32 - self.config.gaps) / 2;
        let window_height = self.config.window_height;

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
            state.size = Some((col_width as i32, window_height as i32).into());
        });
        surface.send_configure();

        let smithay_window = SmithayWindow::new_wayland_window(surface);
        let window = Window::new(smithay_window, window_height);
        self.active_column_mut().windows.push(window);
        self.needs_redraw = true;
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        log::warn!("popup surfaces not yet supported");
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
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

delegate_compositor!(TendrilState);
delegate_xdg_shell!(TendrilState);
delegate_shm!(TendrilState);
delegate_seat!(TendrilState);
delegate_data_device!(TendrilState);
