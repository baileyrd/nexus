//! Canvas-domain handlers: `canvas_read`, `canvas_write`,
//! `canvas_patch`, `canvas_nodes`, `canvas_edges`.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::StorageEngine;

use super::shared::{exec_err, path_arg, to_value};

pub(crate) fn read(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "canvas_read")?;
    let canvas_file = engine
        .read_canvas(&path)
        .map_err(|e| exec_err(format!("canvas_read: {e}")))?;
    to_value(&canvas_file, "canvas_read")
}

pub(crate) fn write(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "canvas_write")?;
    let canvas_file: crate::CanvasFile = args
        .get("canvas")
        .ok_or_else(|| exec_err("canvas_write: missing 'canvas'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("canvas_write: canvas decode: {e}")))
        })?;
    let meta = engine
        .write_canvas(&path, &canvas_file)
        .map_err(|e| exec_err(format!("canvas_write: {e}")))?;
    to_value(&meta, "canvas_write")
}

pub(crate) fn patch(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "canvas_patch")?;
    let ops: Vec<crate::CanvasPatchOp> = args
        .get("ops")
        .ok_or_else(|| exec_err("canvas_patch: missing 'ops'".to_string()))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| exec_err(format!("canvas_patch: ops decode: {e}")))
        })?;
    let meta = engine
        .patch_canvas(&path, &ops)
        .map_err(|e| exec_err(format!("canvas_patch: {e}")))?;
    to_value(&meta, "canvas_patch")
}

pub(crate) fn nodes(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "canvas_nodes")?;
    let nodes = engine
        .canvas_nodes_by_path(&path)
        .map_err(|e| exec_err(format!("canvas_nodes: {e}")))?;
    to_value(&nodes, "canvas_nodes")
}

pub(crate) fn edges(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "canvas_edges")?;
    let edges = engine
        .canvas_edges_by_path(&path)
        .map_err(|e| exec_err(format!("canvas_edges: {e}")))?;
    to_value(&edges, "canvas_edges")
}
