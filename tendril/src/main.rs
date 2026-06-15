mod input;
mod render;
mod shell;
mod state;

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit;
use smithay::reexports::calloop;
use state::TendrilState;

fn main() {
    env_logger::init();

    log::info!("starting tendril compositor (nested mode)");

    let mut event_loop: calloop::EventLoop<TendrilState> =
        calloop::EventLoop::try_new().expect("failed to create event loop");

    let (backend, source) = winit::init::<GlesRenderer>().expect("failed to init winit backend");

    let mut state = TendrilState::new(backend);

    event_loop
        .handle()
        .insert_source(source, |event, _meta, state| {
            state.handle_event(event);
        })
        .expect("failed to register winit source");

    log::info!("tendril compositor started successfully");

    event_loop
        .run(None, &mut state, |state| {
            state.idle();
        })
        .expect("event loop exited with error");
}
