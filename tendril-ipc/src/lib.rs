#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── JSON-RPC 2.0 envelope ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        JsonRpcError {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(-32602, msg)
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::new(-32601, format!("method not found: {}", method.into()))
    }

    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self::new(-32603, msg)
    }
}

impl JsonRpcResponse {
    pub fn success(id: u64, result: Value) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, error: JsonRpcError) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

// ── Method names ──

pub const METHOD_LIST_WINDOWS: &str = "list-windows";
pub const METHOD_SCROLL: &str = "scroll";
pub const METHOD_FOCUS_WINDOW: &str = "focus-window";
pub const METHOD_GET_CONFIG: &str = "get-config";
pub const METHOD_SET_CONFIG: &str = "set-config";
pub const METHOD_GET_WORKSPACES: &str = "get-workspaces";
pub const METHOD_SWITCH_WORKSPACE: &str = "switch-workspace";

// ── list-windows ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: usize,
    pub workspace_id: u8,
    pub column: usize,
    pub index: usize,
    pub y_position: f64,
    pub height: u32,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListWindowsResult {
    pub windows: Vec<WindowInfo>,
}

// ── scroll ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollParams {
    pub delta: f64,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollResult {
    pub new_offset: f64,
}

// ── focus-window ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusWindowParams {
    pub id: usize,
}

// ── get-config / set-config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigData {
    pub window_height: u32,
    pub gaps: u32,
    pub visible_windows: u32,
    #[serde(default = "default_column_count")]
    pub column_count: u32,
}

fn default_column_count() -> u32 { 2 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetConfigParams {
    pub config: ConfigData,
}

// ── get-workspaces ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: u8,
    pub active: bool,
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorkspacesResult {
    pub workspaces: Vec<WorkspaceInfo>,
    pub active_workspace: usize,
}

// ── switch-workspace ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchWorkspaceParams {
    pub id: usize,
}
