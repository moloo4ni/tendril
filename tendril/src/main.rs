mod input;
mod ipc;
mod render;
mod shell;
mod state;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit;
use smithay::input::SeatState;
use smithay::reexports::calloop;
use smithay::reexports::wayland_server::Display;
use smithay::wayland::compositor::CompositorState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use state::{Config, TendrilState};

use crate::shell::AppClientState;

struct AppState {
    display: Display<TendrilState>,
    tendril: Rc<RefCell<TendrilState>>,
}

impl AppState {
    fn idle(&mut self) {
        let mut tendril = self.tendril.borrow_mut();
        let _ = self.display.dispatch_clients(&mut *tendril);
        let _ = self.display.flush_clients();
        tendril.idle();
    }
}

fn main() {
    env_logger::init();
    log::info!("starting tendril compositor (nested mode)");

    let config = Config::load();

    let mut event_loop: calloop::EventLoop<AppState> =
        calloop::EventLoop::try_new().expect("failed to create event loop");

    let (backend, source) = winit::init::<GlesRenderer>().expect("failed to init winit backend");

    let display: Display<TendrilState> =
        Display::new().expect("failed to create wayland display");
    let dh = display.handle();

    let compositor_state = CompositorState::new::<TendrilState>(&dh);
    let xdg_shell_state = XdgShellState::new::<TendrilState>(&dh);
    let decoration_state = XdgDecorationState::new::<TendrilState>(&dh);
    let shm_state = ShmState::new::<TendrilState>(&dh, vec![]);
    let data_device_state = DataDeviceState::new::<TendrilState>(&dh);
    let mut seat_state = SeatState::new();
    let mut seat = seat_state.new_wl_seat(&dh, "default");

    let keyboard = seat.add_keyboard(Default::default(), 200, 200).ok();
    let pointer = Some(seat.add_pointer());

    let tendril_inner = TendrilState::new(
        backend,
        compositor_state,
        xdg_shell_state,
        decoration_state,
        shm_state,
        seat_state,
        seat,
        data_device_state,
        dh,
        keyboard,
        pointer,
    );

    let tendril = Rc::new(RefCell::new(tendril_inner));
    // Set config loaded from file
    tendril.borrow_mut().config = config;

    let mut app_state = AppState { display, tendril: tendril.clone() };

    event_loop
        .handle()
        .insert_source(source, |event, _, app_state: &mut AppState| {
            app_state.tendril.borrow_mut().handle_event(event);
        })
        .expect("failed to register winit source");

    let listening_socket =
        ListeningSocketSource::new_auto().expect("failed to create listening socket");
    let socket_name = listening_socket.socket_name().to_os_string();
    std::env::set_var("WAYLAND_DISPLAY", &socket_name);
    log::info!("listening on WAYLAND_DISPLAY={:?}", socket_name);

    event_loop
        .handle()
        .insert_source(listening_socket, |stream, _, app_state: &mut AppState| {
            if let Err(e) = app_state
                .display
                .handle()
                .insert_client(stream, Arc::new(AppClientState::default()))
            {
                log::error!("failed to insert client: {e:?}");
            }
        })
        .expect("failed to register socket source");

    let ipc_source = ipc::IpcSource::bind(app_state.tendril.clone())
        .expect("failed to bind IPC socket");
    event_loop
        .handle()
        .insert_source(ipc_source, |_, _, _| {})
        .expect("failed to register IPC source");

    log::info!("tendril compositor started successfully");

    event_loop
        .run(None, &mut app_state, |app_state| {
            app_state.idle();
        })
        .expect("event loop exited with error");
}
