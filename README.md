# Tendril

Минималистичный Wayland-композитор с фиксированной двухколоночной раскладкой на Rust + Smithay.

## Статус

- **Stage 1** — инициализация: workspace, winit-бэкенд, цикл событий calloop, минимальный рендер
- **Stage 2** — состояния и геометрия: `Window`, `Column`, `Workspace`, скролл, unit-тесты
- **Stage 3** — Wayland surface handling: `CompositorHandler`, `XdgShellHandler`, сокет, диспетчеризация клиентов, рендер поверхностей, frame callbacks

## Использование

```bash
cargo run --bin tendril
```

Композитор запускается вложенным окном (nested). Wayland-сокет выбирается автоматически (`WAYLAND_DISPLAY` выставляется в логе).

## Архитектура

- `tendril/` — ядро композитора
  - `src/main.rs` — точка входа, `AppState` владеет `Display` + `TendrilState`
  - `src/state.rs` — `Config`, `Window`, `Column`, `Workspace`, `TendrilState`, геометрия, рендер
  - `src/shell.rs` — обработчики протоколов (compositor, xdg-shell, shm, seat), `AppClientState`
  - `src/input.rs` — заглушка (Stage 4)
  - `src/render.rs` — заглушка
- `tendril-ipc/` — IPC-библиотека (Stage 5)
- `tendrilc/` — CLI-клиент (Stage 5)
