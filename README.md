# Tendril

A minimal scroll-driven Wayland compositor with a fixed two-column layout, written in Rust with Smithay.

Windows are arranged in vertical strips per column. Strip scroll is the primary interaction — no floating windows, no stacking, no mouse resize.

## Status

- **Stage 1** — initialization: workspace structure, winit backend, calloop event loop, minimal rendering
- **Stage 2** — core state & geometry: `Window`, `Column`, `Workspace`, scroll math, unit tests
- **Stage 3** — Wayland surface handling: `CompositorHandler`, `XdgShellHandler`, listening socket, client dispatch, surface rendering, frame callbacks

## Usage

```bash
cargo run --bin tendril
```

Runs as a nested compositor window. The Wayland socket is auto-selected (`WAYLAND_DISPLAY` is printed in the log).

## Architecture

- `tendril/` — compositor core
  - `src/main.rs` — entry point, `AppState` owns `Display` + `TendrilState`
  - `src/state.rs` — `Config`, `Window`, `Column`, `Workspace`, `TendrilState`, geometry, rendering
  - `src/shell.rs` — protocol handlers (compositor, xdg-shell, shm, seat), `AppClientState`
  - `src/input.rs` — placeholder (Stage 4)
  - `src/render.rs` — placeholder
- `tendril-ipc/` — IPC library (Stage 5)
- `tendrilc/` — CLI client (Stage 5)

## License

MPL 2.0 — see [LICENSE](LICENSE).
