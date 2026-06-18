# Tendril Development Roadmap

## Stage 1 — Workspace & Backend ✓

Winit backend, calloop event loop, minimal rendering.

- [x] Workspace structure with `tendril`, `tendril-ipc`, `tendrilc` crates
- [x] Smithay winit backend with GlesRenderer
- [x] calloop event loop with winit + listening socket sources
- [x] CI: GitHub Actions (build, clippy, test)

## Stage 2 — Core State & Geometry ✓

Window, Column, Workspace types, scroll math, unit tests.

- [x] `Config`, `Window`, `Column`, `Workspace` structs
- [x] Scroll offset with clamping and snapping (directional + proximity)
- [x] Visibility culling, window Y-position formula
- [x] 10 unit tests covering geometry and scroll

## Stage 3 — Wayland Surface Handling ✓

Protocol handlers, client dispatch, surface rendering.

- [x] `CompositorHandler`, `XdgShellHandler`, `SeatHandler`, etc.
- [x] Delegate macros for all protocols
- [x] Client-side decoration via xdg-decoration
- [x] Surface commit → `needs_redraw = true`
- [x] Frame callbacks via `send_frames_surface_tree`

## Stage 4 — Input Handling ✓

Keyboard and pointer input, focus management, window reorder.

- [x] Mod/Super and Shift tracking by scancode
- [x] Keyboard navigation (hjkl + arrows) with focus-follows-mouse
- [x] Scroll gating: Mod → compositor scroll, no-Mod → app scroll
- [x] Window reorder with Z-index bump and scale=1.05

## Stage 5 — IPC & CLI ✓

Unix socket JSON-RPC 2.0 protocol and CLI client.

- [x] `tendril-ipc` crate: JSON-RPC 2.0 types and method constants
- [x] `ipc.rs`: UnixListener as calloop EventSource, request dispatch
- [x] `tendrilc` CLI with clap: `list-windows`, `config` subcommands
- [x] TOML config loading from `~/.config/tendril/config.toml`
- [x] `Rc<RefCell<TendrilState>>` for shared state between event sources

**IPC methods provided:**
- `list-windows` — list all windows with id, workspace, column, position
- `get-config` — show current compositor configuration
- `scroll` — scroll a column by delta
- `focus-window` — focus a window by ID
- `set-config` — update compositor configuration at runtime
- `get-workspaces` — list all workspaces with their windows
- `switch-workspace` — switch to a workspace

## Stage 6 — Rendering & Animations

Smooth LERP scroll, damage tracking, persistent Z-effect.

- [x] Wire `Column::tick_scroll()` into the idle loop for smooth scrolling
- [x] Track damage regions instead of full-viewport redraw
- [x] Persistent Z-effect animation (fade scale back to 1.0 after 500ms delay)
- [x] Move rendering pipeline from `state.rs` into `render.rs`
- [x] Remove `#![allow(dead_code)]` from state.rs and render.rs

## Known Issues

- **Mod+Scroll (mouse wheel) broken under Sway** — the host compositor captures
  the Mod key before Tendril sees it. Trackpad scroll under Mod may work with
  larger gestures. Workaround: configure Sway with a different `$mod`.

## Future Ideas

- KMS/DRM backend (native, non-nested)
- Multi-monitor support
- Layer-shell protocol support (bars, overlays)
