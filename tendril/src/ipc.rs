use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;

use calloop::{
    EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory,
};

use tendril_ipc::*;
use crate::state::{Config, TendrilState};

pub struct IpcSource {
    listener: UnixListener,
    state: Rc<RefCell<TendrilState>>,
}

impl IpcSource {
    pub fn bind(state: Rc<RefCell<TendrilState>>) -> Result<Self, String> {
        let runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR not set".to_string())?;
        let socket_path = std::path::Path::new(&runtime_dir).join("tendril.sock");
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("failed to bind IPC socket: {e}"))?;
        log::info!("IPC socket ready at {:?}", socket_path);
        Ok(IpcSource { listener, state })
    }
}

impl EventSource for IpcSource {
    type Event = ();
    type Metadata = ();
    type Ret = ();
    type Error = std::io::Error;

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        _token: Token,
        _callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        if readiness.readable {
            while let Ok((stream, _)) = self.listener.accept() {
                handle_client(stream, &self.state);
            }
        }
        Ok(PostAction::Continue)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        // SAFETY: token is immediately stored in the calloop framework and
        // remains valid for the lifetime of this source
        unsafe {
            poll.register(
                &self.listener,
                Interest::READ,
                Mode::Level,
                token_factory.token(),
            )
        }
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        token_factory: &mut TokenFactory,
    ) -> calloop::Result<()> {
        poll.reregister(
            &self.listener,
            Interest::READ,
            Mode::Level,
            token_factory.token(),
        )
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        poll.unregister(&self.listener)
    }
}

fn handle_client(mut stream: UnixStream, state: &Rc<RefCell<TendrilState>>) {
    let line = {
        let mut reader = BufReader::new(&mut stream);
        let mut buf = String::new();
        match reader.read_line(&mut buf) {
            Ok(0) => return,
            Ok(_) => buf,
            Err(e) => {
                log::error!("IPC read error: {e}");
                return;
            }
        }
    };

    let request: JsonRpcRequest = match serde_json::from_str(line.trim()) {
        Ok(req) => req,
        Err(e) => {
            let err = JsonRpcError::new(-32700, format!("parse error: {e}"));
            let resp = JsonRpcResponse::failure(0, err);
            respond(&mut stream, &resp);
            return;
        }
    };

    let response = dispatch(&request, state);
    respond(&mut stream, &response);
}

fn respond(stream: &mut UnixStream, response: &JsonRpcResponse) {
    let mut bytes = match serde_json::to_vec(response) {
        Ok(b) => b,
        Err(e) => {
            log::error!("IPC serialize error: {e}");
            return;
        }
    };
    bytes.push(b'\n');
    let _ = stream.write_all(&bytes);
}

fn dispatch(request: &JsonRpcRequest, state: &Rc<RefCell<TendrilState>>) -> JsonRpcResponse {
    let id = request.id;
    match request.method.as_str() {
        METHOD_LIST_WINDOWS => handle_list_windows(id, state),
        METHOD_SCROLL => handle_scroll(id, state, request),
        METHOD_FOCUS_WINDOW => handle_focus_window(id, state, request),
        METHOD_GET_CONFIG => handle_get_config(id, state),
        METHOD_SET_CONFIG => handle_set_config(id, state, request),
        METHOD_GET_WORKSPACES => handle_get_workspaces(id, state),
        METHOD_SWITCH_WORKSPACE => handle_switch_workspace(id, state, request),
        _ => JsonRpcResponse::failure(id, JsonRpcError::method_not_found(&request.method)),
    }
}

fn handle_list_windows(id: u64, state: &Rc<RefCell<TendrilState>>) -> JsonRpcResponse {
    let Ok(state) = state.try_borrow() else {
        return JsonRpcResponse::failure(
            id,
            JsonRpcError::internal_error("compositor busy, try again"),
        );
    };

    let mut windows = Vec::new();

    for ws in state.workspaces.iter() {
        for left in [true, false] {
            let col = ws.column(left);
            for (i, w) in col.windows.iter().enumerate() {
                let y = w.y_position(i, &state.config, col.scroll_offset);
                let title = w.toplevel
                    .toplevel()
                    .and_then(|tl| {
                        smithay::wayland::compositor::with_states(tl.wl_surface(), |states| {
                            states
                                .data_map
                                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                                .expect("toplevel surface data")
                                .lock()
                                .unwrap()
                                .title
                                .clone()
                        })
                    })
                    .unwrap_or_default();
                windows.push(WindowInfo {
                    id: w.id,
                    workspace_id: ws.id,
                    column: left,
                    index: i,
                    y_position: y,
                    height: w.height,
                    title,
                });
            }
        }
    }

    let result = ListWindowsResult { windows };
    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
}

fn handle_get_config(id: u64, state: &Rc<RefCell<TendrilState>>) -> JsonRpcResponse {
    let Ok(state) = state.try_borrow() else {
        return JsonRpcResponse::failure(
            id,
            JsonRpcError::internal_error("compositor busy, try again"),
        );
    };

    let config = ConfigData {
        window_height: state.config.window_height,
        gaps: state.config.gaps,
        visible_windows: state.config.visible_windows,
    };
    JsonRpcResponse::success(id, serde_json::to_value(config).unwrap())
}

fn handle_scroll(id: u64, state: &Rc<RefCell<TendrilState>>, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params: ScrollParams = match parse_params(request) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let Ok(mut state) = state.try_borrow_mut() else {
        return JsonRpcResponse::failure(id, JsonRpcError::internal_error("compositor busy, try again"));
    };

    let left = params.left.unwrap_or_else(|| state.workspace().active_column);
    let new_offset = state.scroll_column(left, params.delta);
    state.needs_redraw = true;

    let result = ScrollResult { new_offset };
    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
}

fn handle_focus_window(id: u64, state: &Rc<RefCell<TendrilState>>, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params: FocusWindowParams = match parse_params(request) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let Ok(mut state) = state.try_borrow_mut() else {
        return JsonRpcResponse::failure(id, JsonRpcError::internal_error("compositor busy, try again"));
    };

    if state.focus_window(params.id) {
        JsonRpcResponse::success(id, serde_json::to_value(true).unwrap())
    } else {
        JsonRpcResponse::failure(id, JsonRpcError::new(-1, format!("window {} not found", params.id)))
    }
}

fn handle_set_config(id: u64, state: &Rc<RefCell<TendrilState>>, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params: SetConfigParams = match parse_params(request) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let Ok(mut state) = state.try_borrow_mut() else {
        return JsonRpcResponse::failure(id, JsonRpcError::internal_error("compositor busy, try again"));
    };

    let config = Config {
        window_height: params.config.window_height,
        gaps: params.config.gaps,
        visible_windows: params.config.visible_windows,
    };
    state.set_config(config);

    JsonRpcResponse::success(id, serde_json::to_value(true).unwrap())
}

fn handle_get_workspaces(id: u64, state: &Rc<RefCell<TendrilState>>) -> JsonRpcResponse {
    let Ok(state) = state.try_borrow() else {
        return JsonRpcResponse::failure(
            id,
            JsonRpcError::internal_error("compositor busy, try again"),
        );
    };

    let mut workspaces = Vec::new();
    for ws in state.workspaces.iter() {
        let mut windows = Vec::new();
        for left in [true, false] {
            let col = ws.column(left);
            for (i, w) in col.windows.iter().enumerate() {
                let y = w.y_position(i, &state.config, col.scroll_offset);
                let title = w.toplevel
                    .toplevel()
                    .and_then(|tl| {
                        smithay::wayland::compositor::with_states(tl.wl_surface(), |states| {
                            states
                                .data_map
                                .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
                                .expect("toplevel surface data")
                                .lock()
                                .unwrap()
                                .title
                                .clone()
                        })
                    })
                    .unwrap_or_default();
                windows.push(WindowInfo {
                    id: w.id,
                    workspace_id: ws.id,
                    column: left,
                    index: i,
                    y_position: y,
                    height: w.height,
                    title,
                });
            }
        }
        workspaces.push(WorkspaceInfo {
            id: ws.id,
            active: ws.id as usize == state.active_workspace,
            windows,
        });
    }

    let result = GetWorkspacesResult {
        workspaces,
        active_workspace: state.active_workspace,
    };
    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap())
}

fn handle_switch_workspace(id: u64, state: &Rc<RefCell<TendrilState>>, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params: SwitchWorkspaceParams = match parse_params(request) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let Ok(mut state) = state.try_borrow_mut() else {
        return JsonRpcResponse::failure(id, JsonRpcError::internal_error("compositor busy, try again"));
    };

    if state.switch_workspace(params.id) {
        JsonRpcResponse::success(id, serde_json::to_value(true).unwrap())
    } else {
        JsonRpcResponse::failure(
            id,
            JsonRpcError::new(-1, format!("workspace {} not found", params.id)),
        )
    }
}

#[allow(clippy::result_large_err)]
fn parse_params<T: serde::de::DeserializeOwned>(request: &JsonRpcRequest) -> Result<T, JsonRpcResponse> {
    let params = request
        .params
        .as_ref()
        .ok_or_else(|| JsonRpcResponse::failure(request.id, JsonRpcError::invalid_params("missing params")))?;
    serde_json::from_value(params.clone()).map_err(|e| {
        JsonRpcResponse::failure(request.id, JsonRpcError::invalid_params(format!("invalid params: {e}")))
    })
}
