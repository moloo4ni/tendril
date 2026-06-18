use smithay::backend::input::{
    AbsolutePositionEvent, Event, InputEvent, KeyState,
    KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
};
use smithay::backend::input::{Axis, AxisSource, Keycode};
use smithay::backend::winit::WinitInput;
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::input::keyboard::FilterResult;
use smithay::wayland::seat::WaylandFocus;

use crate::state::TendrilState;

// xkbcommon keycodes = evdev scancode + 8
const SCANCODE_LEFTMETA: u32 = 133;
const SCANCODE_RIGHTMETA: u32 = 134;
const SCANCODE_LEFTSHIFT: u32 = 50;
const SCANCODE_RIGHTSHIFT: u32 = 62;

const SCANCODE_H: u32 = 43;
const SCANCODE_J: u32 = 44;
const SCANCODE_K: u32 = 45;
const SCANCODE_L: u32 = 46;
const SCANCODE_UP: u32 = 111;
const SCANCODE_DOWN: u32 = 116;
const SCANCODE_LEFT: u32 = 113;
const SCANCODE_RIGHT: u32 = 114;

pub fn handle_input(state: &mut TendrilState, event: InputEvent<WinitInput>) {
    match event {
        InputEvent::Keyboard { event } => {
            let code = event.key_code();
            let pressed = event.state() == KeyState::Pressed;
            let raw: u32 = code.into();

            match raw {
                SCANCODE_LEFTMETA | SCANCODE_RIGHTMETA => {
                    state.mod_pressed = pressed;
                    if !pressed {
                        state.shift_pressed = false;
                    }
                }
                SCANCODE_LEFTSHIFT | SCANCODE_RIGHTSHIFT => {
                    state.shift_pressed = pressed;
                }
                SCANCODE_H | SCANCODE_LEFT => {
                    if pressed && state.mod_pressed && !state.shift_pressed {
                        focus_prev_column(state);
                    } else if pressed && state.mod_pressed && state.shift_pressed {
                        keyboard_reorder(state, -1.0);
                    } else {
                        forward_key(state, code, event);
                    }
                }
                SCANCODE_L | SCANCODE_RIGHT => {
                    if pressed && state.mod_pressed && !state.shift_pressed {
                        focus_next_column(state);
                    } else if pressed && state.mod_pressed && state.shift_pressed {
                        keyboard_reorder(state, 1.0);
                    } else {
                        forward_key(state, code, event);
                    }
                }
                SCANCODE_J | SCANCODE_DOWN => {
                    if pressed && state.mod_pressed && !state.shift_pressed {
                        focus_next(state, 1);
                    } else if pressed && state.mod_pressed && state.shift_pressed {
                        keyboard_reorder(state, 1.0);
                    } else {
                        forward_key(state, code, event);
                    }
                }
                SCANCODE_K | SCANCODE_UP => {
                    if pressed && state.mod_pressed && !state.shift_pressed {
                        focus_next(state, -1);
                    } else if pressed && state.mod_pressed && state.shift_pressed {
                        keyboard_reorder(state, -1.0);
                    } else {
                        forward_key(state, code, event);
                    }
                }
                _ => {
                    forward_key(state, code, event);
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
            if let Some(ref ptr) = ptr {
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

            let surface = state.window_at(x, y).and_then(|(left, idx)| {
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
        }
        InputEvent::PointerAxis { event } => {
            if state.mod_pressed {
                let amount_v120 = event.amount_v120(Axis::Vertical);
                let amount = event.amount(Axis::Vertical);

                if let Some(v120) = amount_v120 {
                    if v120 == 0.0 { return; }
                    if state.shift_pressed {
                        reorder_window(state, v120);
                    } else {
                        scroll_strip(state, v120, true);
                    }
                } else if let Some(amount) = amount {
                    if amount == 0.0 { return; }
                    if state.shift_pressed {
                        reorder_window(state, amount);
                    } else {
                        scroll_strip_smooth(state, amount);
                    }
                }
            } else {
                let ptr = state.pointer_handle.clone();
                if let Some(ptr) = ptr {
                    let amount = event.amount(Axis::Vertical);
                    let amount_v120 = event.amount_v120(Axis::Vertical);
                    let (delta, is_wheel) = match (amount, amount_v120) {
                        (Some(d), None) => (d, false),
                        (None, Some(v)) => (v * 0.5, true),
                        (Some(d), Some(_)) => (d, false),
                        (None, None) => (0.0, false),
                    };
                    if delta != 0.0 {
                        log::info!("forwarding axis to app: delta={}, is_wheel={}", delta, is_wheel);
                        ptr.axis(
                            state,
                            AxisFrame::new(event.time() as u32)
                                .source(if is_wheel { AxisSource::Wheel } else { AxisSource::Finger })
                                .value(Axis::Vertical, delta),
                        );
                        ptr.frame(state);
                    }
                }
            }
        }
        _ => {}
    }
}

fn forward_key(state: &mut TendrilState, code: impl Into<Keycode>, event: impl KeyboardKeyEvent<WinitInput>) {
    let kb = state.keyboard_handle.clone();
    if let Some(kb) = kb {
        kb.input::<(), _>(
            state,
            code.into(),
            event.state(),
            state.serial(),
            event.time() as u32,
            |_, _, _| FilterResult::Forward,
        );
    }
}

fn focus_prev_column(state: &mut TendrilState) {
    let n_cols = state.workspace().n_cols();
    if n_cols < 2 { return; }
    let cur = state.workspace().active_column;
    let next = (cur + n_cols - 1) % n_cols;
    focus_column_index(state, next);
}

fn focus_next_column(state: &mut TendrilState) {
    let n_cols = state.workspace().n_cols();
    if n_cols < 2 { return; }
    let cur = state.workspace().active_column;
    let next = (cur + 1) % n_cols;
    focus_column_index(state, next);
}

fn focus_column_index(state: &mut TendrilState, col_idx: usize) {
    let surface = {
        let ws = state.workspace_mut();
        ws.active_column = col_idx;
        let col = &mut ws.columns[col_idx];
        if col.windows.is_empty() { return; }
        let idx = col.focused_idx.unwrap_or(0).min(col.windows.len() - 1);
        col.focused_idx = Some(idx);
        col.windows[idx].toplevel.wl_surface().map(|s| (*s).clone())
    };
    if let Some(surface) = surface {
        let kb = state.keyboard_handle.clone();
        if let Some(kb) = kb {
            kb.set_focus(state, Some(surface), state.serial());
        }
    }
    state.needs_redraw = true;
}

fn focus_next(state: &mut TendrilState, direction: i32) {
    let surface = {
        let col = state.active_column_mut();
        let n = col.windows.len();
        if n == 0 { return; }
        let current = col.focused_idx.unwrap_or(0) as i32;
        let new = (current + direction).clamp(0, n as i32 - 1) as usize;
        if new == col.focused_idx.unwrap_or(0) { return; }
        col.focused_idx = Some(new);
        col.windows[new].toplevel.wl_surface().map(|s| (*s).clone())
    };
    if let Some(surface) = surface {
        let kb = state.keyboard_handle.clone();
        if let Some(kb) = kb {
            kb.set_focus(state, Some(surface), state.serial());
        }
    }
    state.needs_redraw = true;
}

fn scroll_strip(state: &mut TendrilState, delta: f64, is_wheel: bool) {
    log::info!("scroll_strip: delta={}, is_wheel={}", delta, is_wheel);
    let vp = state.viewport_size.1 as i32;
    let config = state.config.clone();
    let col = state.active_column_mut();
    if col.windows.is_empty() { return; }
    col.target_scroll_offset += delta;
    col.clamp_scroll(&config, vp);
    if is_wheel {
        col.snap_scroll_directional(&config, vp, delta);
    } else {
        col.snap_scroll(&config, vp);
    }
    log::info!("scroll_strip: target_offset={}", col.target_scroll_offset);
    state.needs_redraw = true;
    state.damage_full = true;
}

fn scroll_strip_smooth(state: &mut TendrilState, amount: f64) {
    log::info!("scroll_strip_smooth: amount={}", amount);
    let vp = state.viewport_size.1 as i32;
    let config = state.config.clone();
    let col = state.active_column_mut();
    if col.windows.is_empty() { return; }
    col.target_scroll_offset += amount;
    col.clamp_scroll(&config, vp);
    col.snap_scroll(&config, vp);
    log::info!("scroll_strip_smooth: target_offset={}", col.target_scroll_offset);
    state.needs_redraw = true;
}

fn keyboard_reorder(state: &mut TendrilState, direction: f64) {
    let col = state.active_column_mut();
    reorder_impl(col, direction);
    state.needs_redraw = true;
}

fn reorder_window(state: &mut TendrilState, delta: f64) {
    let col = state.active_column_mut();
    reorder_impl(col, delta);
    state.needs_redraw = true;
}

fn reorder_impl(col: &mut crate::state::Column, direction: f64) {
    let Some(idx) = col.focused_idx else { return };
    for w in col.windows.iter_mut() {
        w.z_index_offset = 0.0;
        w.z_anim_delay = 0.0;
    }
    if direction > 0.0 && idx < col.windows.len() - 1 {
        col.windows.swap(idx, idx + 1);
        col.focused_idx = Some(idx + 1);
        col.windows[idx + 1].z_index_offset = 10.0;
        col.windows[idx + 1].z_anim_delay = 0.5;
    } else if direction < 0.0 && idx > 0 {
        col.windows.swap(idx, idx - 1);
        col.focused_idx = Some(idx - 1);
        col.windows[idx - 1].z_index_offset = 10.0;
        col.windows[idx - 1].z_anim_delay = 0.5;
    }
}
