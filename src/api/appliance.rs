//! Appliance-style HTTP surface: thread workspaces, uploads, artifacts, `.skill` zip install.

use crate::harness::{append_session_event, load_recent_session_events};
use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::json;
use std::io::Cursor;
use std::path::Path as StdPath;
use zip::ZipArchive;

use super::ApiState;
use crate::harness::{ensure_thread_workspace_on_disk, sanitize_thread_id, workspace_dirs};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/api/appliance/workspace/:thread_id",
            get(workspace_info).post(ensure_workspace),
        )
        .route(
            "/api/appliance/workspace/:thread_id/uploads",
            get(list_uploads),
        )
        .route(
            "/api/appliance/workspace/:thread_id/artifacts",
            get(list_artifacts),
        )
        .route(
            "/api/appliance/workspace/:thread_id/upload",
            post(upload_file),
        )
        .route("/api/agent-skills/install", post(install_skill_zip))
        .route("/api/session/:thread_id/snapshot", get(session_snapshot))
        .route("/api/session/:thread_id/event", post(session_append_event))
        .route("/api/session/:thread_id/events", get(session_list_events))
}

#[derive(Serialize)]
struct WorkspaceInfoResponse {
    thread_id: String,
    workspace_root: String,
    uploads_dir: String,
    artifacts_dir: String,
    exists: bool,
}

/// Single JSON view: workspace layout + optional Honcho hybrid memory + world counts (layer 10).
async fn session_snapshot(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (root, uploads, artifacts) = workspace_dirs(&state.appliance_home, &thread_id);
    let exists = tokio::fs::metadata(&root)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false);
    let workspace = serde_json::json!({
        "thread_id": sanitize_thread_id(&thread_id),
        "workspace_root": root.to_string_lossy(),
        "uploads_dir": uploads.to_string_lossy(),
        "artifacts_dir": artifacts.to_string_lossy(),
        "exists": exists,
    });

    let hybrid_memory = if let Some(ref h) = state.honcho {
        let mem = h.hybrid_memory.read().await;
        Some(serde_json::json!({
            "stats": mem.stats,
            "entries_preview": mem.list_entries_preview(32),
        }))
    } else {
        None
    };

    let world = {
        let g = state.inner.read().await;
        g.world.as_ref().map(|w| {
            serde_json::json!({
                "agents": w.agents.len(),
                "beliefs": w.beliefs.len(),
                "tick_count": w.tick_count,
            })
        })
    };

    let session_events = load_recent_session_events(&state.appliance_home, &thread_id, 48).await;

    Ok(Json(serde_json::json!({
        "thread_id": sanitize_thread_id(&thread_id),
        "workspace": workspace,
        "hybrid_memory": hybrid_memory,
        "world": world,
        "session_events": session_events,
    })))
}

async fn session_append_event(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<StatusCode, StatusCode> {
    append_session_event(&state.appliance_home, &thread_id, body)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

async fn session_list_events(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let events = load_recent_session_events(&state.appliance_home, &thread_id, 256).await;
    Ok(Json(serde_json::json!({
        "thread_id": sanitize_thread_id(&thread_id),
        "events": events,
    })))
}

async fn workspace_info(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
) -> Result<Json<WorkspaceInfoResponse>, StatusCode> {
    let (root, uploads, artifacts) = workspace_dirs(&state.appliance_home, &thread_id);
    let exists = tokio::fs::metadata(&root)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false);
    Ok(Json(WorkspaceInfoResponse {
        thread_id: sanitize_thread_id(&thread_id),
        workspace_root: root.to_string_lossy().into_owned(),
        uploads_dir: uploads.to_string_lossy().into_owned(),
        artifacts_dir: artifacts.to_string_lossy().into_owned(),
        exists,
    }))
}

async fn ensure_workspace(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
) -> Result<Json<WorkspaceInfoResponse>, StatusCode> {
    let home = state.appliance_home.clone();
    let root = ensure_thread_workspace_on_disk(&home, &thread_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (_, uploads, artifacts) = workspace_dirs(&home, &thread_id);
    Ok(Json(WorkspaceInfoResponse {
        thread_id: sanitize_thread_id(&thread_id),
        workspace_root: root.to_string_lossy().into_owned(),
        uploads_dir: uploads.to_string_lossy().into_owned(),
        artifacts_dir: artifacts.to_string_lossy().into_owned(),
        exists: true,
    }))
}

#[derive(Serialize)]
struct DirEntryRow {
    name: String,
    is_dir: bool,
    size: u64,
}

async fn list_dir(dir: &StdPath) -> Result<Vec<DirEntryRow>, StatusCode> {
    let mut out = Vec::new();
    let mut rd = tokio::fs::read_dir(dir)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    while let Ok(Some(e)) = rd.next_entry().await {
        let Ok(ft) = e.file_type().await else {
            continue;
        };
        let Ok(meta) = e.metadata().await else {
            continue;
        };
        let name = e.file_name().to_string_lossy().into_owned();
        out.push(DirEntryRow {
            name,
            is_dir: ft.is_dir(),
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

async fn list_uploads(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<DirEntryRow>>, StatusCode> {
    let (_, uploads, _) = workspace_dirs(&state.appliance_home, &thread_id);
    let rows = list_dir(&uploads).await?;
    Ok(Json(rows))
}

async fn list_artifacts(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<DirEntryRow>>, StatusCode> {
    let (_, _, artifacts) = workspace_dirs(&state.appliance_home, &thread_id);
    let rows = list_dir(&artifacts).await?;
    Ok(Json(rows))
}

async fn upload_file(
    State(state): State<ApiState>,
    Path(thread_id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (_, uploads, _) = workspace_dirs(&state.appliance_home, &thread_id);
    tokio::fs::create_dir_all(&uploads)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field
            .file_name()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "upload.bin".to_string());
        let safe_name = sanitize_thread_id(&filename);
        let dest = uploads.join(safe_name);
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        tokio::fs::write(&dest, &data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(json!({
            "path": dest.to_string_lossy(),
            "bytes": data.len(),
        })));
    }
    Err(StatusCode::BAD_REQUEST)
}

fn install_zip_to_skills(bytes: &[u8], dest_root: &StdPath) -> Result<usize, String> {
    std::fs::create_dir_all(dest_root).map_err(|e| e.to_string())?;
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader).map_err(|e| e.to_string())?;
    let mut n = 0usize;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = file.enclosed_name() else {
            continue;
        };
        let out = dest_root.join(rel);
        if out.strip_prefix(dest_root).is_err() {
            return Err("zip path escapes destination".into());
        }
        if (*file.name()).ends_with('/') {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut outf = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outf).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

async fn install_skill_zip(
    State(state): State<ApiState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dest_root = state.appliance_home.join("skills");
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        if field.name() != Some("file") {
            continue;
        }
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        let dest = dest_root.clone();
        let skills_root_display = dest_root.to_string_lossy().into_owned();
        let files_written =
            tokio::task::spawn_blocking(move || install_zip_to_skills(&data[..], &dest))
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .map_err(|_| StatusCode::BAD_REQUEST)?;
        return Ok(Json(json!({
            "skills_root": skills_root_display,
            "files_written": files_written,
        })));
    }
    Err(StatusCode::BAD_REQUEST)
}
