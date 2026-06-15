# Tendril

A minimal scroll-driven Wayland compositor with a fixed two-column layout, written in Rust with Smithay.

## Status

| Stage | What | Done |
|---|---|---|
| 1 | Workspace structure, winit backend, calloop event loop, minimal rendering | ✓ |
| 2 | Core state & geometry: `Window`, `Column`, `Workspace`, scroll math, unit tests | ✓ |
| 3 | Wayland surface handling: `CompositorHandler`, `XdgShellHandler`, listening socket, client dispatch, surface rendering, frame callbacks | ✓ |
| 4 | Input handling: Mod/Super tracking, keyboard forwarding, pointer motion/button/axis, scroll, focus-on-click | ✓ |
| 5 | IPC — Unix socket, JSON-RPC protocol, `tendrilc` CLI client | |
| 6 | Rendering & animations — damage tracking, smooth LERP scroll, Z-axis effect | |

## Known Issues

- **Scroll not working** — winit backend on Wayland delivers `PixelDelta` (trackpad-style) with `amount_v120 = None`. The `amount()` fix is in place but needs verification.
- **Window decorations** — kitty draws a title bar (`xdg-decoration` protocol not yet implemented in the compositor).
- **Mod key** intercepted by Sway when running nested. One solution: toggle mod with a keypress instead of hold.

## Usage

```bash
cd tendril
RUST_LOG=info ./run.sh       # starts compositor + 3 kitty windows
# or manually:
RUST_LOG=info cargo run --bin tendril
```

Runs as a nested compositor window. The Wayland socket is auto-selected (`WAYLAND_DISPLAY` is printed in the log). Run apps inside with:

```bash
WAYLAND_DISPLAY=wayland-2 kitty
```

## Architecture

```mermaid
graph LR
    Client[Wayland Client]
    Socket[Wayland Socket]
    Dispatch[Protocol Dispatch]
    Shell[shell.rs]
    Input[input.rs]
    State[TendrilState]
    Backend[winit Backend]
    Render[Render Loop]

    Client --> Socket
    Socket --> Dispatch
    Dispatch --> Shell
    Shell --> State
    Input --> State
    State --> Render
    Backend --> Render
```

- `tendril/` — compositor core
  - `src/main.rs` — entry point, `AppState` owns `Display` + `TendrilState`, calloop event loop with winit + socket sources
  - `src/state.rs` — `Config`, `Window`, `Column`, `Workspace`, `TendrilState`, geometry math (scroll, visibility, layout), rendering pipeline (GLES, frame callbacks), unit tests
  - `src/shell.rs` — protocol handlers (compositor, xdg-shell, shm, seat, data-device), `AppClientState`, delegate macros
  - `src/input.rs` — input event processing: Mod/Shift tracking, keyboard forwarding, pointer motion/button/axis, scroll clamping, focus-on-click
  - `src/render.rs` — placeholder (Stage 6)
- `tendril-ipc/` — IPC types (Stage 5)
- `tendrilc/` — CLI client (Stage 5)

## Controls

| Input | Action |
|---|---|
| Mod + Scroll | Scroll active column (needs fix) |
| Mod + Shift + Scroll | Reorder windows (future) |
| Pointer click | Focus window under cursor |

Mod = left or right Super key (Windows/Command key).

## License

MPL 2.0 — see [LICENSE](LICENSE).
