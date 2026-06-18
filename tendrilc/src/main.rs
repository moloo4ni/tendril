use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use clap::{Parser, Subcommand};
use serde_json::Value;

use tendril_ipc::*;

#[derive(Parser)]
#[command(name = "tendrilc", about = "Tendril compositor CLI client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all windows managed by the compositor
    ListWindows,
    /// Show current compositor configuration
    Config,
    /// Scroll a column by delta pixels (positive = down, negative = up)
    Scroll {
        /// Delta in pixels (positive scrolls down, negative scrolls up)
        delta: f64,
        /// Scroll left column (omit for active column)
        #[arg(long)]
        left: Option<bool>,
    },
    /// Focus a window by ID
    FocusWindow {
        /// Window ID
        id: usize,
    },
    /// Update compositor configuration
    SetConfig {
        /// Window height
        #[arg(long, default_value_t = 500)]
        window_height: u32,
        /// Gap size between windows
        #[arg(long, default_value_t = 1)]
        gaps: u32,
        /// Number of visible windows per column
        #[arg(long, default_value_t = 2)]
        visible_windows: u32,
    },
    /// List all workspaces with their windows
    GetWorkspaces,
    /// Switch to a workspace
    SwitchWorkspace {
        /// Workspace ID (0-4)
        id: usize,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::ListWindows => cmd_list_windows(),
        Commands::Config => cmd_config(),
        Commands::Scroll { delta, left } => cmd_scroll(delta, left),
        Commands::FocusWindow { id } => cmd_focus_window(id),
        Commands::SetConfig { window_height, gaps, visible_windows } => {
            cmd_set_config(window_height, gaps, visible_windows)
        }
        Commands::GetWorkspaces => cmd_get_workspaces(),
        Commands::SwitchWorkspace { id } => cmd_switch_workspace(id),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn connect() -> Result<UnixStream, String> {
    let runtime_dir =
        std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR not set".to_string())?;
    let socket_path = std::path::Path::new(&runtime_dir).join("tendril.sock");
    UnixStream::connect(&socket_path).map_err(|e| format!("failed to connect to {socket_path:?}: {e}"))
}

fn send_request(method: &str) -> Result<Value, String> {
    let mut stream = connect()?;
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params: None,
    };
    let mut bytes = serde_json::to_vec(&request).map_err(|e| format!("serialize error: {e}"))?;
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .map_err(|e| format!("write error: {e}"))?;

    let response_str = {
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| format!("read error: {e}"))?;
        line
    };

    let resp: JsonRpcResponse =
        serde_json::from_str(response_str.trim()).map_err(|e| format!("parse response error: {e}"))?;

    if let Some(error) = resp.error {
        return Err(format!("IPC error ({}): {}", error.code, error.message));
    }

    resp.result.ok_or_else(|| "no result in response".to_string())
}

fn cmd_list_windows() -> Result<(), String> {
    let result = send_request(METHOD_LIST_WINDOWS)?;
    let data: ListWindowsResult =
        serde_json::from_value(result).map_err(|e| format!("parse result error: {e}"))?;

    if data.windows.is_empty() {
        println!("no windows");
        return Ok(());
    }

    println!("{:<8} {:<5} {:<7} {:<6} {:<10} Title", "ID", "WS", "Column", "Index", "Y");
    println!("{:-<8} {:-<5} {:-<7} {:-<6} {:-<10} {:-<20}", "", "", "", "", "", "");
    for w in &data.windows {
        let col = if w.column { "left" } else { "right" };
        println!(
            "{:<8} {:<5} {:<7} {:<6} {:<10.1} {}",
            w.id, w.workspace_id, col, w.index, w.y_position, w.title
        );
    }
    Ok(())
}

fn cmd_config() -> Result<(), String> {
    let result = send_request(METHOD_GET_CONFIG)?;
    let data: ConfigData =
        serde_json::from_value(result).map_err(|e| format!("parse result error: {e}"))?;
    println!("window_height: {}", data.window_height);
    println!("gaps: {}", data.gaps);
    println!("visible_windows: {}", data.visible_windows);
    Ok(())
}

fn send_request_with_params(method: &str, params: &impl serde::Serialize) -> Result<Value, String> {
    let mut stream = connect()?;
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: method.to_string(),
        params: Some(serde_json::to_value(params).map_err(|e| format!("serialize params error: {e}"))?),
    };
    let mut bytes = serde_json::to_vec(&request).map_err(|e| format!("serialize error: {e}"))?;
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .map_err(|e| format!("write error: {e}"))?;

    let response_str = {
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| format!("read error: {e}"))?;
        line
    };

    let resp: JsonRpcResponse =
        serde_json::from_str(response_str.trim()).map_err(|e| format!("parse response error: {e}"))?;

    if let Some(error) = resp.error {
        return Err(format!("IPC error ({}): {}", error.code, error.message));
    }

    resp.result.ok_or_else(|| "no result in response".to_string())
}

fn cmd_scroll(delta: f64, left: Option<bool>) -> Result<(), String> {
    let params = ScrollParams { delta, left };
    let result = send_request_with_params(METHOD_SCROLL, &params)?;
    let data: ScrollResult =
        serde_json::from_value(result).map_err(|e| format!("parse result error: {e}"))?;
    println!("new_offset: {:.1}", data.new_offset);
    Ok(())
}

fn cmd_focus_window(id: usize) -> Result<(), String> {
    let params = FocusWindowParams { id };
    let result = send_request_with_params(METHOD_FOCUS_WINDOW, &params)?;
    if result.as_bool().unwrap_or(false) {
        println!("focused window {id}");
    } else {
        eprintln!("window {id} not found");
    }
    Ok(())
}

fn cmd_set_config(window_height: u32, gaps: u32, visible_windows: u32) -> Result<(), String> {
    let params = SetConfigParams {
        config: ConfigData {
            window_height,
            gaps,
            visible_windows,
        },
    };
    send_request_with_params(METHOD_SET_CONFIG, &params)?;
    println!("config updated");
    Ok(())
}

fn cmd_get_workspaces() -> Result<(), String> {
    let result = send_request(METHOD_GET_WORKSPACES)?;
    let data: GetWorkspacesResult =
        serde_json::from_value(result).map_err(|e| format!("parse result error: {e}"))?;

    println!("active workspace: {}", data.active_workspace);
    println!();
    for ws in &data.workspaces {
        let active = if ws.active { " (active)" } else { "" };
        println!("workspace {}{}:", ws.id, active);
        if ws.windows.is_empty() {
            println!("  no windows");
            continue;
        }
        for w in &ws.windows {
            let col = if w.column { "left" } else { "right" };
            println!(
                "  [{}] {} col={} y={:.1} h={} \"{}\"",
                w.id, col, w.index, w.y_position, w.height, w.title
            );
        }
    }
    Ok(())
}

fn cmd_switch_workspace(id: usize) -> Result<(), String> {
    let params = SwitchWorkspaceParams { id };
    let result = send_request_with_params(METHOD_SWITCH_WORKSPACE, &params)?;
    if result.as_bool().unwrap_or(false) {
        println!("switched to workspace {id}");
    } else {
        eprintln!("workspace {id} not found");
    }
    Ok(())
}
