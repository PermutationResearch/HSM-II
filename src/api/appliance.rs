//! Appliance-style HTTP surface: thread workspaces, uploads, artifacts, `.skill` zip install.

use crate::harness::{append_session_event, load_recent_session_events};
use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::Engine as _;
use rand::RngCore;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
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
        .route("/api/mcp/oauth/pkce/start", post(mcp_oauth_pkce_start))
        .route("/api/mcp/oauth/pkce/callback", get(mcp_oauth_pkce_callback))
        .route("/api/mcp/oauth/pkce/exchange", post(mcp_oauth_pkce_exchange))
        .route("/api/mcp/oauth/pkce/state/:state_id", get(mcp_oauth_pkce_state))
        .route(
            "/api/gateways/compatibility-matrix",
            get(gateway_compatibility_matrix),
        )
        .route("/api/session/:thread_id/snapshot", get(session_snapshot))
        .route("/api/session/:thread_id/event", post(session_append_event))
        .route("/api/session/:thread_id/events", get(session_list_events))
}

#[derive(serde::Deserialize)]
struct McpPkceStartBody {
    auth_url: String,
    token_url: String,
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    connector_ref: Option<String>,
    #[serde(default)]
    company_id: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    extra_params: serde_json::Map<String, serde_json::Value>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedPkceState {
    state_id: String,
    state: String,
    auth_url: String,
    token_url: String,
    client_id: String,
    redirect_uri: String,
    scopes: Vec<String>,
    code_verifier: String,
    code_challenge: String,
    created_at_ms: i64,
    expires_at_ms: i64,
    callback_verified: bool,
    callback_code: Option<String>,
    callback_error: Option<String>,
    consumed: bool,
    connector_ref: Option<String>,
    company_id: Option<String>,
    token_response_meta: Option<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct McpPkceExchangeBody {
    state_id: String,
    #[serde(default)]
    code: Option<String>,
}

#[derive(serde::Deserialize)]
struct McpPkceCallbackQuery {
    state: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn pkce_s256(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn pkce_state_dir(home: &StdPath) -> std::path::PathBuf {
    home.join("mcp").join("oauth").join("pkce")
}

fn pkce_state_path(home: &StdPath, state_id: &str) -> std::path::PathBuf {
    pkce_state_dir(home).join(format!("{}.json", crate::harness::sanitize_thread_id(state_id)))
}

async fn save_pkce_state(home: &StdPath, value: &PersistedPkceState) -> Result<(), StatusCode> {
    let dir = pkce_state_dir(home);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let data = serde_json::to_vec_pretty(value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(pkce_state_path(home, &value.state_id), data)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

async fn load_pkce_state(home: &StdPath, state_id: &str) -> Result<PersistedPkceState, StatusCode> {
    let raw = tokio::fs::read(pkce_state_path(home, state_id))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_slice::<PersistedPkceState>(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn mcp_oauth_pkce_start(
    State(state): State<ApiState>,
    Json(body): Json<McpPkceStartBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.auth_url.trim().is_empty()
        || body.client_id.trim().is_empty()
        || body.token_url.trim().is_empty()
        || body.redirect_uri.trim().is_empty()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = chrono::Utc::now().timestamp_millis();
    let ttl_ms = std::env::var("HSM_MCP_PKCE_STATE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(900)
        .clamp(60, 3600)
        * 1000;
    let state_id = random_urlsafe(16);
    let oauth_state = random_urlsafe(18);
    let code_verifier = random_urlsafe(48);
    let code_challenge = pkce_s256(&code_verifier);
    let mut params = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), body.client_id.trim().to_string()),
        ("redirect_uri".to_string(), body.redirect_uri.trim().to_string()),
        ("state".to_string(), oauth_state.clone()),
        ("code_challenge".to_string(), code_challenge.clone()),
        ("code_challenge_method".to_string(), "S256".to_string()),
    ];
    if !body.scopes.is_empty() {
        params.push(("scope".to_string(), body.scopes.join(" ")));
    }
    for (k, v) in body.extra_params {
        if let Some(s) = v.as_str() {
            params.push((k, s.to_string()));
        }
    }
    let encoded = params
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let join = if body.auth_url.contains('?') { "&" } else { "?" };
    let persisted = PersistedPkceState {
        state_id: state_id.clone(),
        state: oauth_state.clone(),
        auth_url: body.auth_url.clone(),
        token_url: body.token_url.clone(),
        client_id: body.client_id.clone(),
        redirect_uri: body.redirect_uri.clone(),
        scopes: body.scopes,
        code_verifier: code_verifier.clone(),
        code_challenge: code_challenge.clone(),
        created_at_ms: now,
        expires_at_ms: now + ttl_ms,
        callback_verified: false,
        callback_code: None,
        callback_error: None,
        consumed: false,
        connector_ref: body.connector_ref.clone(),
        company_id: body.company_id.clone(),
        token_response_meta: None,
    };
    save_pkce_state(&state.appliance_home, &persisted).await?;
    Ok(Json(json!({
        "state_id": state_id,
        "connector_ref": persisted.connector_ref,
        "company_id": persisted.company_id,
        "authorization_url": format!("{}{}{}", body.auth_url, join, encoded),
        "state": oauth_state,
        "code_verifier": code_verifier,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
        "expires_at_ms": now + ttl_ms,
    })))
}

async fn mcp_oauth_pkce_callback(
    State(state): State<ApiState>,
    Query(q): Query<McpPkceCallbackQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if q.state.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let dir = pkce_state_dir(&state.appliance_home);
    let mut rd = tokio::fs::read_dir(&dir).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let now = chrono::Utc::now().timestamp_millis();
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        let Ok(raw) = tokio::fs::read(&path).await else {
            continue;
        };
        let Ok(mut s) = serde_json::from_slice::<PersistedPkceState>(&raw) else {
            continue;
        };
        if s.state == q.state {
            if now > s.expires_at_ms {
                s.callback_error = Some("expired".to_string());
                let _ = save_pkce_state(&state.appliance_home, &s).await;
                return Ok(Json(json!({ "ok": false, "state_id": s.state_id, "error": "expired" })));
            }
            s.callback_verified = true;
            s.callback_code = q.code.clone();
            s.callback_error = q.error.clone();
            save_pkce_state(&state.appliance_home, &s).await?;
            return Ok(Json(json!({
                "ok": q.error.is_none(),
                "state_id": s.state_id,
                "error": q.error,
            })));
        }
    }
    Err(StatusCode::NOT_FOUND)
}

async fn mcp_oauth_pkce_exchange(
    State(state): State<ApiState>,
    Json(body): Json<McpPkceExchangeBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if body.state_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut persisted = load_pkce_state(&state.appliance_home, body.state_id.trim()).await?;
    let now = chrono::Utc::now().timestamp_millis();
    if now > persisted.expires_at_ms {
        return Ok(Json(json!({ "ok": false, "error": "pkce state expired" })));
    }
    if persisted.consumed {
        return Ok(Json(json!({ "ok": false, "error": "pkce state already consumed" })));
    }
    if !persisted.callback_verified {
        return Ok(Json(json!({ "ok": false, "error": "pkce callback not verified yet" })));
    }
    if let Some(err) = persisted.callback_error.clone() {
        return Ok(Json(json!({ "ok": false, "error": err })));
    }
    let code = body
        .code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| persisted.callback_code.clone())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", persisted.client_id.trim()),
        ("redirect_uri", persisted.redirect_uri.trim()),
        ("code", code.trim()),
        ("code_verifier", persisted.code_verifier.trim()),
    ];
    let resp = client
        .post(persisted.token_url.trim())
        .form(&form)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = resp.status();
    let text = resp.text().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !status.is_success() {
        return Ok(Json(json!({
            "ok": false,
            "status": status.as_u16(),
            "error": text,
        })));
    }
    let parsed = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|_| json!({ "raw": text }));
    persisted.consumed = true;
    persisted.token_response_meta = Some(json!({
        "token_type": parsed.get("token_type").cloned(),
        "scope": parsed.get("scope").cloned(),
        "expires_in": parsed.get("expires_in").cloned(),
        "obtained_at_ms": chrono::Utc::now().timestamp_millis(),
    }));
    let _ = save_pkce_state(&state.appliance_home, &persisted).await;
    Ok(Json(json!({
        "ok": true,
        "state_id": persisted.state_id,
        "connector_ref": persisted.connector_ref,
        "token_response": parsed,
    })))
}

async fn mcp_oauth_pkce_state(
    State(state): State<ApiState>,
    Path(state_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let persisted = load_pkce_state(&state.appliance_home, state_id.trim()).await?;
    Ok(Json(json!({
        "state_id": persisted.state_id,
        "connector_ref": persisted.connector_ref,
        "company_id": persisted.company_id,
        "callback_verified": persisted.callback_verified,
        "consumed": persisted.consumed,
        "expires_at_ms": persisted.expires_at_ms,
        "callback_error": persisted.callback_error,
        "token_response_meta": persisted.token_response_meta,
    })))
}

async fn gateway_compatibility_matrix() -> Json<serde_json::Value> {
    Json(json!({
        "matrix": crate::gateways::tier1_compatibility_matrix(),
    }))
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
