//! Read/write files under a company's Paperclip `hsmii_home` from the console (path-safe).

use axum::{
    extract::{Path as PathParam, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

use crate::console::ConsoleState;

use super::no_db;

const MAX_READ_BYTES: usize = 2 * 1024 * 1024;
const MAX_WRITE_BYTES: usize = 2 * 1024 * 1024;
const MAX_LIST_ENTRIES: usize = 800;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/workspace/list",
            get(list_workspace),
        )
        .route(
            "/api/company/companies/:company_id/workspace/mkdir",
            post(post_mkdir_workspace),
        )
        // POST delete: works through Next.js proxy and avoids 405 when an older binary omitted DELETE.
        .route(
            "/api/company/companies/:company_id/workspace/file/delete",
            post(post_delete_workspace_file),
        )
        .route(
            "/api/company/companies/:company_id/workspace/file",
            get(get_workspace_file)
                .put(put_workspace_file)
                .delete(delete_workspace_file),
        )
}

/// `Ok(None)` = company row missing; `Ok(Some(None|""))` = no usable home.
async fn fetch_hsmii_home_cell(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<Option<Option<String>>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<String>>("SELECT hsmii_home FROM companies WHERE id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await
}

/// Relative path: only normal components, no `..`, not absolute.
fn parse_rel_path(raw: &str) -> Result<PathBuf, &'static str> {
    let s = raw.trim().replace('\\', "/");
    if s.is_empty() {
        return Ok(PathBuf::new());
    }
    let p = Path::new(&s);
    if p.is_absolute() {
        return Err("path must be relative");
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err("invalid path component");
            }
        }
    }
    Ok(out)
}

fn canonical_home(home: &Path) -> Result<PathBuf, std::io::Error> {
    std::fs::canonicalize(home)
}

/// Resolve `home` + `rel` and ensure the result stays under `home_canon` (symlink-safe when path exists).
fn resolve_under_home(home_canon: &Path, rel: &Path) -> Result<PathBuf, String> {
    let full = home_canon.join(rel);
    if full.exists() {
        let c = std::fs::canonicalize(&full).map_err(|e| e.to_string())?;
        if !c.starts_with(home_canon) {
            return Err("path escapes workspace root".into());
        }
        return Ok(c);
    }
    let mut anc = full.clone();
    loop {
        if anc.exists() {
            let ac = std::fs::canonicalize(&anc).map_err(|e| e.to_string())?;
            if !ac.starts_with(home_canon) {
                return Err("path escapes workspace root".into());
            }
            return Ok(full);
        }
        let Some(p) = anc.parent() else {
            return Err("invalid path".into());
        };
        if p == home_canon || anc.as_os_str().is_empty() {
            return Ok(full);
        }
        anc = p.to_path_buf();
    }
}

fn rel_display(home_canon: &Path, abs: &Path) -> String {
    abs.strip_prefix(home_canon)
        .ok()
        .and_then(|p| {
            let s = p.to_string_lossy();
            if s.is_empty() {
                None
            } else {
                Some(s.replace('\\', "/"))
            }
        })
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    path: String,
}

async fn list_workspace(
    State(st): State<ConsoleState>,
    PathParam(company_id): PathParam<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let home_cell = fetch_hsmii_home_cell(pool, company_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(home_opt) = home_cell else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    };
    let Some(home_str) = home_opt.filter(|s| !s.trim().is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home; set it on the company record" })),
        ));
    };
    let home = Path::new(home_str.trim());
    if !home.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "hsmii_home is not a directory", "hsmii_home": home_str })),
        ));
    }
    let home_canon = canonical_home(home).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let rel = parse_rel_path(&q.path)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    let dir_abs = resolve_under_home(&home_canon, &rel)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    if !dir_abs.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "not a directory", "path": q.path })),
        ));
    }
    let read_dir = std::fs::read_dir(&dir_abs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let mut entries: Vec<Value> = Vec::new();
    for item in read_dir {
        let item = item.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        if entries.len() >= MAX_LIST_ENTRIES {
            break;
        }
        let name = item.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let meta = item.metadata().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        let kind = if meta.is_dir() { "dir" } else { "file" };
        let child_abs = dir_abs.join(&name);
        let path_disp = rel_display(&home_canon, &child_abs);
        let size_bytes = if meta.is_dir() {
            Value::Null
        } else {
            json!(meta.len())
        };
        let modified_at = meta
            .modified()
            .ok()
            .map(|st| DateTime::<Utc>::from(st).to_rfc3339());
        entries.push(json!({
            "name": name,
            "path": path_disp,
            "kind": kind,
            "size_bytes": size_bytes,
            "modified_at": modified_at,
        }));
    }
    entries.sort_by(|a, b| {
        let an = a["name"].as_str().unwrap_or("");
        let bn = b["name"].as_str().unwrap_or("");
        let ad = a["kind"].as_str() == Some("dir");
        let bd = b["kind"].as_str() == Some("dir");
        ad.cmp(&bd)
            .then_with(|| an.to_lowercase().cmp(&bn.to_lowercase()))
    });
    Ok(Json(json!({
        "hsmii_home": home_str,
        "path": rel_display(&home_canon, &dir_abs),
        "entries": entries,
    })))
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

async fn get_workspace_file(
    State(st): State<ConsoleState>,
    PathParam(company_id): PathParam<Uuid>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if q.path.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "query parameter path is required" })),
        ));
    }
    let home_cell = fetch_hsmii_home_cell(pool, company_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(home_opt) = home_cell else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    };
    let Some(home_str) = home_opt.filter(|s| !s.trim().is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home" })),
        ));
    };
    let home = Path::new(home_str.trim());
    let home_canon = canonical_home(home).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let rel = parse_rel_path(&q.path)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    let file_abs = resolve_under_home(&home_canon, &rel)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    if !file_abs.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not a file", "path": q.path })),
        ));
    }
    let bytes = std::fs::read(&file_abs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if bytes.len() > MAX_READ_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": format!("file larger than {MAX_READ_BYTES} bytes") })),
        ));
    }
    let content = String::from_utf8(bytes).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "file is not valid UTF-8" })),
        )
    })?;
    Ok(Json(json!({
        "path": rel_display(&home_canon, &file_abs),
        "content": content,
    })))
}

async fn delete_workspace_file_by_rel_path(
    pool: &PgPool,
    company_id: Uuid,
    rel_path: &str,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if rel_path.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path is required" })),
        ));
    }
    let home_cell = fetch_hsmii_home_cell(pool, company_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(home_opt) = home_cell else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    };
    let Some(home_str) = home_opt.filter(|s| !s.trim().is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home" })),
        ));
    };
    let home = Path::new(home_str.trim());
    let home_canon = canonical_home(home).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let rel = parse_rel_path(rel_path)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    let file_abs = resolve_under_home(&home_canon, &rel)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    if !file_abs.is_file() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "not a file (only regular files can be deleted)",
                "path": rel_path,
            })),
        ));
    }
    std::fs::remove_file(&file_abs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
        "path": rel_display(&home_canon, &file_abs),
    })))
}

async fn delete_workspace_file(
    State(st): State<ConsoleState>,
    PathParam(company_id): PathParam<Uuid>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    delete_workspace_file_by_rel_path(pool, company_id, &q.path).await
}

#[derive(Deserialize)]
struct DeleteWorkspaceFileBody {
    path: String,
}

async fn post_delete_workspace_file(
    State(st): State<ConsoleState>,
    PathParam(company_id): PathParam<Uuid>,
    Json(body): Json<DeleteWorkspaceFileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    delete_workspace_file_by_rel_path(pool, company_id, &body.path).await
}

#[derive(Deserialize)]
struct MkdirBody {
    path: String,
}

/// Create a directory (and parents) under `hsmii_home`, like `mkdir -p`.
async fn post_mkdir_workspace(
    State(st): State<ConsoleState>,
    PathParam(company_id): PathParam<Uuid>,
    Json(body): Json<MkdirBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.path.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path is required" })),
        ));
    }
    let home_cell = fetch_hsmii_home_cell(pool, company_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(home_opt) = home_cell else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    };
    let Some(home_str) = home_opt.filter(|s| !s.trim().is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home" })),
        ));
    };
    let home = Path::new(home_str.trim());
    let home_canon = canonical_home(home).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let rel = parse_rel_path(&body.path)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    let dir_abs = resolve_under_home(&home_canon, &rel)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    if dir_abs.exists() {
        if dir_abs.is_dir() {
            let canon = std::fs::canonicalize(&dir_abs).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
            if !canon.starts_with(&home_canon) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "path escapes workspace root" })),
                ));
            }
            return Ok(Json(json!({
                "ok": true,
                "path": rel_display(&home_canon, &canon),
                "created": false,
            })));
        }
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path exists but is not a directory", "path": body.path })),
        ));
    }
    std::fs::create_dir_all(&dir_abs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let canon = std::fs::canonicalize(&dir_abs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if !canon.starts_with(&home_canon) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path escapes workspace root" })),
        ));
    }
    Ok(Json(json!({
        "ok": true,
        "path": rel_display(&home_canon, &canon),
        "created": true,
    })))
}

#[derive(Deserialize)]
struct PutFileBody {
    path: String,
    content: String,
}

async fn put_workspace_file(
    State(st): State<ConsoleState>,
    PathParam(company_id): PathParam<Uuid>,
    Json(body): Json<PutFileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.path.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path is required" })),
        ));
    }
    if body.content.len() > MAX_WRITE_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": format!("content larger than {MAX_WRITE_BYTES} bytes") })),
        ));
    }
    let home_cell = fetch_hsmii_home_cell(pool, company_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(home_opt) = home_cell else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    };
    let Some(home_str) = home_opt.filter(|s| !s.trim().is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home" })),
        ));
    };
    let home = Path::new(home_str.trim());
    let home_canon = canonical_home(home).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let rel = parse_rel_path(&body.path)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    let file_abs = resolve_under_home(&home_canon, &rel)
        .map_err(|m| (StatusCode::BAD_REQUEST, Json(json!({ "error": m }))))?;
    let parent = file_abs.parent().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid file path" })),
        )
    })?;
    if !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    }
    let parent_canon = std::fs::canonicalize(parent).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if !parent_canon.starts_with(&home_canon) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path escapes workspace root" })),
        ));
    }
    crate::fs_atomic::write_atomic(&file_abs, body.content.as_bytes()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
        "path": rel_display(&home_canon, &file_abs),
    })))
}

/// One Markdown instruction file under an agent pack folder (`agents/<name>/…`).
#[derive(Debug, Serialize)]
pub struct AgentInstructionFile {
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

/// Walk `agents/<agent_folder_name>/` for `.md` files up to `max_depth` levels below that folder
/// (depth 0 = files in the agent root only; depth 4 = common nested layouts).
pub fn list_agent_markdown_instructions(
    hsmii_home_trimmed: &str,
    agent_folder_name: &str,
    max_depth: u32,
    max_files: usize,
) -> Result<Vec<AgentInstructionFile>, String> {
    let name = agent_folder_name.trim();
    if name.is_empty() {
        return Ok(Vec::new());
    }
    let home = Path::new(hsmii_home_trimmed.trim());
    if !home.is_dir() {
        return Err("hsmii_home is not a directory".into());
    }
    let home_canon = canonical_home(home).map_err(|e| e.to_string())?;
    let rel = parse_rel_path(&format!("agents/{name}")).map_err(|m| m.to_string())?;
    let agent_root = resolve_under_home(&home_canon, &rel)?;
    if !agent_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    walk_agent_markdown(&agent_root, &home_canon, max_depth, max_files, &mut out)?;
    out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    Ok(out)
}

fn walk_agent_markdown(
    dir: &Path,
    home_canon: &Path,
    remaining_depth: u32,
    max_files: usize,
    out: &mut Vec<AgentInstructionFile>,
) -> Result<(), String> {
    if out.len() >= max_files {
        return Ok(());
    }
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for item in rd {
        let item = item.map_err(|e| e.to_string())?;
        let fname = item.file_name().to_string_lossy().to_string();
        if fname.starts_with('.') {
            continue;
        }
        let p = item.path();
        let meta = item.metadata().map_err(|e| e.to_string())?;
        if meta.is_file() {
            if !fname.to_lowercase().ends_with(".md") {
                continue;
            }
            let canon = std::fs::canonicalize(&p).map_err(|e| e.to_string())?;
            if !canon.starts_with(home_canon) {
                continue;
            }
            let path = rel_display(home_canon, &canon);
            let modified_at = meta
                .modified()
                .ok()
                .map(|st| DateTime::<Utc>::from(st).to_rfc3339());
            out.push(AgentInstructionFile {
                path,
                name: fname,
                size_bytes: meta.len(),
                modified_at,
            });
        } else if meta.is_dir() {
            subdirs.push(p);
        }
    }
    if remaining_depth > 0 {
        for sub in subdirs {
            walk_agent_markdown(&sub, home_canon, remaining_depth - 1, max_files, out)?;
            if out.len() >= max_files {
                break;
            }
        }
    }
    Ok(())
}
