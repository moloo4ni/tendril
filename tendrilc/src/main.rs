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
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::ListWindows => cmd_list_windows(),
        Commands::Config => cmd_config(),
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
