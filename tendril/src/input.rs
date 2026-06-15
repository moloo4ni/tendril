use smithay::backend::input::{
    AbsolutePositionEvent, ButtonState, Event, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
};
use smithay::backend::input::{Axis, AxisSource};
use smithay::backend::winit::WinitInput;
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::input::keyboard::FilterResult;
use smithay::wayland::seat::WaylandFocus;

use crate::state::TendrilState;

const SCANCODE_LEFTMETA: u32 = 125;
const SCANCODE_RIGHTMETA: u32 = 126;
const SCANCODE_LEFTSHIFT: u32 = 42;
const SCANCODE_RIGHTSHIFT: u32 = 54;

pub fn handle_input(state: &mut TendrilState, event: InputEvent<WinitInput>) {
    match event {
        InputEvent::Keyboard { event } => {
            let code = event.key_code();
            let pressed = event.state() == KeyState::Pressed;

            match code.into() {
                SCANCODE_LEFTMETA | SCANCODE_RIGHTMETA => {
                    state.mod_pressed = pressed;
                }
                SCANCODE_LEFTSHIFT | SCANCODE_RIGHTSHIFT => {
                    state.shift_pressed = pressed;
                }
                _ => {
                    let kb = state.keyboard_handle.clone();
                    if let Some(kb) = kb {
                        kb.input::<(), _>(
                            state,
                            code,
                            event.state(),
                            state.serial(),
                            event.time() as u32,
                            |_, _, _| FilterResult::Forward,
                        );
                    }
                }
            }
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let vp_w = state.viewport_size.0 as i32;
            let vp_h = state.viewport_size.1 as i32;
            let x = event.x_transformed(vp_w);
            let y = event.y_transformed(vp_h);
            state.cursor_pos = (x, y);

            let ptr = state.pointer_handle.clone();
            if let Some(ptr) = ptr {
                ptr.motion(
                    state,
                    None,
                    &MotionEvent {
                        location: (x, y).into(),
                        serial: state.serial(),
                        time: event.time() as u32,
                    },
                );
                ptr.frame(state);
            }
        }
        InputEvent::PointerButton { event } => {
            state.needs_redraw = true;

            let ptr = state.pointer_handle.clone();
            if let Some(ref ptr) = ptr {
                ptr.button(
                    state,
                    &ButtonEvent {
                        time: event.time() as u32,
                        button: event.button_code(),
                        state: event.state(),
                        serial: state.serial(),
                    },
                );
                ptr.frame(state);
            }

            if event.state() == ButtonState::Pressed {
                let pos = state.cursor_pos;
                let surface = state.window_at(pos.0, pos.1).and_then(|(left, idx)| {
                    let ws = state.workspace_mut();
                    ws.active_column = left;
                    let col = ws.column_mut(left);
                    col.focused_idx = Some(idx);
                    col.windows[idx].toplevel.wl_surface().map(|s| (*s).clone())
                });
                if let Some(surface) = surface {
                    let kb = state.keyboard_handle.clone();
                    if let Some(kb) = kb {
                        kb.set_focus(state, Some(surface), state.serial());
                    }
                }
            }
        }
        InputEvent::PointerAxis { event } => {
            if state.mod_pressed && state.shift_pressed {
                // Mod+Shift+Scroll → reorder window (future)
            } else if state.mod_pressed {
                let delta = event
                    .amount_v120(Axis::Vertical)
                    .unwrap_or(0.0)
                    * -0.5;
                let vp = state.viewport_size.1 as i32;
                let config = state.config.clone();
                let col = state.active_column_mut();
                col.target_scroll_offset += delta;
                col.clamp_scroll(&config, vp);
                state.needs_redraw = true;

                let ptr = state.pointer_handle.clone();
                if let Some(ptr) = ptr {
                    ptr.axis(
                        state,
                        AxisFrame::new(event.time() as u32)
                            .source(AxisSource::Wheel)
                            .value(Axis::Vertical, delta),
                    );
                    ptr.frame(state);
                }
            }
        }
        _ => {}
    }
}
