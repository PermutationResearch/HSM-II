//! Team API Handlers for HSM-II SaaS
//!
//! Axum route handlers for the multi-tenant autonomous business team API.
//! Each handler extracts `TenantContext` from the JWT, loads the tenant's
//! `TeamOrchestrator`, and performs the requested operation.

use crate::auth::{Permission, PersistentAuthManager, TenantContext};
use crate::autonomous_team::{
    AudienceSegment, BrandVoice, BusinessRole, ChannelType, MemberStatus,
};
use crate::tenant::{TenantPlan, TenantRegistry};
use crate::usage_tracker::UsageTracker;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info, warn};

// ═══════════════════════════════════════════════════════════════════
// Section 1: Shared Application State
// ═══════════════════════════════════════════════════════════════════

/// Application state shared across all team API handlers.
#[derive(Clone)]
pub struct TeamAppState {
    pub registry: Arc<TenantRegistry>,
    pub auth: Arc<PersistentAuthManager>,
    pub usage: Arc<UsageTracker>,
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: Request/Response Types
// ═══════════════════════════════════════════════════════════════════

/// Request to register a new tenant.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    #[serde(default)]
    pub plan: Option<String>,
}

/// Response after successful tenant registration.
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub tenant_id: String,
    pub name: String,
    pub plan: String,
    pub api_key: String,
    pub message: String,
}

/// Request to exchange API key for JWT.
#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub api_key: String,
}

/// Response with JWT token.
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub token_type: String,
    pub expires_in: u64,
}

/// Submit a task for routing to the best agent.
#[derive(Debug, Deserialize)]
pub struct TaskRequest {
    pub description: String,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
}

/// Response from task routing.
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub task_id: String,
    pub assigned_to: String,
    pub assigned_role: String,
    pub bid_score: f64,
    pub system_prompt: String,
}

/// Request to create a campaign.
#[derive(Debug, Deserialize)]
pub struct CreateCampaignRequest {
    pub name: String,
    pub goal: String,
    #[serde(default)]
    pub channels: Vec<String>,
}

/// Response after campaign creation.
#[derive(Debug, Serialize)]
pub struct CreateCampaignResponse {
    pub campaign_id: String,
    pub name: String,
    pub status: String,
}

/// Request to update brand context (all fields optional for partial update).
#[derive(Debug, Deserialize)]
pub struct UpdateBrandRequest {
    pub name: Option<String>,
    pub positioning: Option<String>,
    pub audiences: Option<Vec<AudienceSegment>>,
    pub voice: Option<BrandVoice>,
    pub forbidden_words: Option<Vec<String>>,
    pub values: Option<Vec<String>>,
    pub differentiators: Option<Vec<String>>,
}

/// Request to update a member's status.
#[derive(Debug, Deserialize)]
pub struct UpdateMemberStatusRequest {
    pub status: String,
}

/// Usage summary response.
#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub api_calls_today: u64,
    pub api_calls_this_month: u64,
    pub llm_tokens_this_month: u64,
    pub publishes_this_month: u64,
    pub plan: String,
    pub daily_limit: u32,
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: Public Auth Handlers (no JWT required)
// ═══════════════════════════════════════════════════════════════════

/// POST /api/v1/auth/register — Register a new tenant.
pub async fn register_tenant(
    State(state): State<TeamAppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, StatusCode> {
    let plan = match req.plan.as_deref() {
        Some("starter") => TenantPlan::Starter,
        Some("pro") => TenantPlan::Pro,
        Some("enterprise") => TenantPlan::Enterprise,
        _ => TenantPlan::Free,
    };

    // Create tenant
    let tenant = state
        .registry
        .create_tenant(&req.name, plan)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create tenant");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Create an admin API key for the tenant
    let (api_key, _key_id) = state
        .auth
        .create_tenant_key(
            &tenant.id,
            format!("{} admin key", req.name),
            vec![Permission::Read, Permission::Write, Permission::Admin],
            None,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create API key");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!(tenant_id = %tenant.id, name = %tenant.name, "Tenant registered via API");

    Ok(Json(RegisterResponse {
        tenant_id: tenant.id,
        name: tenant.name,
        plan: plan.label().to_string(),
        api_key,
        message: "Save your API key — it won't be shown again.".to_string(),
    }))
}

/// POST /api/v1/auth/token — Exchange API key for JWT.
pub async fn get_token(
    State(state): State<TeamAppState>,
    Json(req): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, StatusCode> {
    let token = state
        .auth
        .validate_key(&req.api_key)
        .await
        .map_err(|e| {
            warn!(error = %e, "Token exchange failed");
            StatusCode::UNAUTHORIZED
        })?;

    Ok(Json(TokenResponse {
        token,
        token_type: "Bearer".to_string(),
        expires_in: 86400,
    }))
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Team Management Handlers (JWT required)
// ═══════════════════════════════════════════════════════════════════

/// GET /api/v1/team — List all team members.
pub async fn list_team(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
) -> Result<impl IntoResponse, StatusCode> {
    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;

    let members: Vec<serde_json::Value> = orch
        .members
        .iter()
        .map(|m| {
            json!({
                "role": format!("{:?}", m.role),
                "label": m.role.label(),
                "tag": m.role.tag(),
                "status": format!("{:?}", m.status),
                "tasks_completed": m.tasks_completed,
                "tasks_failed": m.tasks_failed,
                "reliability": m.reliability(),
                "proactivity": m.role.default_proactivity(),
            })
        })
        .collect();

    Ok(Json(json!({
        "tenant_id": ctx.tenant_id,
        "member_count": members.len(),
        "members": members,
    })))
}

/// GET /api/v1/team/:role — Get a specific team member.
pub async fn get_member(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Path(role_name): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    state.usage.record_api_call(&ctx.tenant_id).await;

    let role = parse_role(&role_name).ok_or(StatusCode::NOT_FOUND)?;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;
    let member = orch.member(role).ok_or(StatusCode::NOT_FOUND)?;

    let prompt = orch.system_prompt_for(role);

    Ok(Json(json!({
        "role": format!("{:?}", member.role),
        "label": member.role.label(),
        "status": format!("{:?}", member.status),
        "persona_name": member.persona.name,
        "tasks_completed": member.tasks_completed,
        "tasks_failed": member.tasks_failed,
        "reliability": member.reliability(),
        "domain_history": member.domain_history,
        "activation_keywords": member.role.activation_keywords(),
        "system_prompt_preview": if prompt.len() > 500 {
            format!("{}...", &prompt[..500])
        } else {
            prompt
        },
    })))
}

/// PUT /api/v1/team/:role/status — Enable/disable a team member.
pub async fn update_member_status(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Path(role_name): Path<String>,
    Json(req): Json<UpdateMemberStatusRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !ctx.permissions.contains(&Permission::Admin) && !ctx.permissions.contains(&Permission::Write)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    state.usage.record_api_call(&ctx.tenant_id).await;

    let role = parse_role(&role_name).ok_or(StatusCode::NOT_FOUND)?;
    let new_status = parse_member_status(&req.status).ok_or(StatusCode::BAD_REQUEST)?;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    {
        let mut orch = orch.write().await;
        let member = orch.member_mut(role).ok_or(StatusCode::NOT_FOUND)?;
        member.status = new_status;
    }

    // Persist
    state
        .registry
        .save_tenant_state(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "role": format!("{:?}", role),
        "status": req.status,
        "message": "Member status updated",
    })))
}

// ═══════════════════════════════════════════════════════════════════
// Section 5: Brand Context Handlers
// ═══════════════════════════════════════════════════════════════════

/// GET /api/v1/brand — Get the tenant's brand context.
pub async fn get_brand(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
) -> Result<impl IntoResponse, StatusCode> {
    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;
    let brand = &orch.brand;

    Ok(Json(json!({
        "name": brand.name,
        "positioning": brand.positioning,
        "audiences": brand.audiences,
        "voice": brand.voice,
        "forbidden_words": brand.forbidden_words,
        "values": brand.values,
        "differentiators": brand.differentiators,
    })))
}

/// PUT /api/v1/brand — Update the tenant's brand context.
pub async fn update_brand(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Json(req): Json<UpdateBrandRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !ctx.permissions.contains(&Permission::Admin) && !ctx.permissions.contains(&Permission::Write)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    {
        let mut orch = orch.write().await;
        if let Some(name) = req.name {
            orch.brand.name = name;
        }
        if let Some(positioning) = req.positioning {
            orch.brand.positioning = positioning;
        }
        if let Some(audiences) = req.audiences {
            orch.brand.audiences = audiences;
        }
        if let Some(voice) = req.voice {
            orch.brand.voice = voice;
        }
        if let Some(words) = req.forbidden_words {
            orch.brand.forbidden_words = words;
        }
        if let Some(values) = req.values {
            orch.brand.values = values;
        }
        if let Some(diffs) = req.differentiators {
            orch.brand.differentiators = diffs;
        }
    }

    // Persist
    state
        .registry
        .save_tenant_state(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(tenant_id = %ctx.tenant_id, "Brand context updated");

    Ok(Json(json!({
        "message": "Brand context updated",
    })))
}

// ═══════════════════════════════════════════════════════════════════
// Section 6: Task Routing Handlers
// ═══════════════════════════════════════════════════════════════════

/// POST /api/v1/tasks — Submit a task for routing.
pub async fn submit_task(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Json(req): Json<TaskRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !ctx.permissions.contains(&Permission::Write) && !ctx.permissions.contains(&Permission::Admin)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;

    let member = orch
        .route_task(&req.description)
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let bid_score = member.bid(&req.description);
    let system_prompt = orch.system_prompt_for(member.role);

    let task_id = uuid::Uuid::new_v4().to_string();

    info!(
        tenant_id = %ctx.tenant_id,
        task_id = %task_id,
        assigned_to = %member.role.label(),
        bid = %bid_score,
        "Task routed"
    );

    Ok(Json(TaskResponse {
        task_id,
        assigned_to: member.role.label().to_string(),
        assigned_role: format!("{:?}", member.role),
        bid_score,
        system_prompt,
    }))
}

// ═══════════════════════════════════════════════════════════════════
// Section 7: Campaign Handlers
// ═══════════════════════════════════════════════════════════════════

/// POST /api/v1/campaigns — Create a new campaign.
pub async fn create_campaign(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Json(req): Json<CreateCampaignRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if !ctx.permissions.contains(&Permission::Write) && !ctx.permissions.contains(&Permission::Admin)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let channels: Vec<ChannelType> = req
        .channels
        .iter()
        .filter_map(|c| parse_channel_type(c))
        .collect();

    let campaign_id = {
        let mut orch = orch.write().await;
        let campaign = orch.campaign_store
            .create_campaign(&req.name, &req.goal, channels);
        campaign.id.clone()
    };

    // Persist
    state
        .registry
        .save_tenant_state(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(tenant_id = %ctx.tenant_id, campaign_id = %campaign_id, "Campaign created");

    Ok((
        StatusCode::CREATED,
        Json(CreateCampaignResponse {
            campaign_id,
            name: req.name,
            status: "Draft".to_string(),
        }),
    ))
}

/// GET /api/v1/campaigns — List all campaigns.
pub async fn list_campaigns(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
) -> Result<impl IntoResponse, StatusCode> {
    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;

    let campaigns: Vec<serde_json::Value> = orch
        .campaign_store
        .campaigns
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "goal": c.goal,
                "status": format!("{:?}", c.status),
                "channels": c.channels.iter().map(|ch| format!("{:?}", ch)).collect::<Vec<_>>(),
                "started_at": c.started_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "tenant_id": ctx.tenant_id,
        "campaign_count": campaigns.len(),
        "campaigns": campaigns,
    })))
}

/// GET /api/v1/campaigns/:id — Get campaign details + snapshot.
pub async fn get_campaign(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Path(campaign_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;

    let campaign = orch
        .campaign_store
        .campaigns
        .iter()
        .find(|c| c.id == campaign_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let snapshot = orch.campaign_store.campaign_snapshot(&campaign_id);

    Ok(Json(json!({
        "id": campaign.id,
        "name": campaign.name,
        "goal": campaign.goal,
        "status": format!("{:?}", campaign.status),
        "channels": campaign.channels.iter().map(|ch| format!("{:?}", ch)).collect::<Vec<_>>(),
        "started_at": campaign.started_at,
        "snapshot": snapshot.map(|s| json!({
            "total_views": s.total_views,
            "total_clicks": s.total_clicks,
            "total_engagements": s.total_engagements,
            "total_conversions": s.total_conversions,
            "cost_usd": s.cost_usd,
            "avg_sentiment": s.avg_sentiment,
            "ctr": s.ctr(),
            "cac": s.cac(),
        })),
    })))
}

/// GET /api/v1/campaigns/:id/patterns — Extract dream patterns from campaign.
pub async fn get_dream_patterns(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Path(_campaign_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    state.usage.record_api_call(&ctx.tenant_id).await;

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let orch = orch.read().await;

    // Dream patterns are extracted from all campaign metrics
    let patterns = orch.campaign_store.extract_dream_patterns();

    let pattern_list: Vec<serde_json::Value> = patterns
        .into_iter()
        .map(|(domain, text, valence)| {
            json!({
                "domain": domain,
                "text": text,
                "valence": valence,
            })
        })
        .collect();

    Ok(Json(json!({
        "tenant_id": ctx.tenant_id,
        "pattern_count": pattern_list.len(),
        "patterns": pattern_list,
    })))
}

// ═══════════════════════════════════════════════════════════════════
// Section 8: Task Outcome Handler (Dream Feedback Loop)
// ═══════════════════════════════════════════════════════════════════

/// Request body for recording a task outcome.
#[derive(Debug, Deserialize)]
pub struct TaskOutcomeRequest {
    /// Domain key for the task (e.g. "campaign:blog_launch", "feature:auth").
    pub domain: String,
    /// Whether the task was completed successfully.
    pub success: bool,
    /// Quality score [0.0, 1.0].
    pub quality: f64,
    /// Which role executed the task (e.g. "writer", "cmo").
    pub role: String,
}

/// POST /api/v1/tasks/:id/outcome — Record a task outcome and refresh dream routing.
///
/// This closes the feedback loop: outcome → campaign patterns → DreamAdvisor
/// → enhanced routing for future tasks.
pub async fn record_task_outcome(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
    Path(_task_id): Path<String>,
    Json(body): Json<TaskOutcomeRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let role = parse_role(&body.role).ok_or(StatusCode::BAD_REQUEST)?;

    let quality = body.quality.clamp(0.0, 1.0);

    let orch = state
        .registry
        .get_orchestrator(&ctx.tenant_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let generation = {
        let mut orch = orch.write().await;

        // 1. Record outcome on the member
        if let Some(member) = orch.member_mut(role) {
            member.record_outcome(&body.domain, body.success, quality);
        } else {
            return Err(StatusCode::NOT_FOUND);
        }

        // 2. Refresh dream advisor from campaign patterns
        orch.refresh_dream_advisor();

        // 3. Persist state
        let _ = orch.save();

        orch.dream_advisor.generation
    };

    // Track usage
    state.usage.record_api_call(&ctx.tenant_id).await;

    Ok(Json(json!({
        "status": "recorded",
        "role": role.label(),
        "domain": body.domain,
        "success": body.success,
        "quality": quality,
        "dream_advisor_generation": generation,
    })))
}

// ═══════════════════════════════════════════════════════════════════
// Section 9: Usage Handler
// ═══════════════════════════════════════════════════════════════════

/// GET /api/v1/usage — Get tenant's usage and billing data.
pub async fn get_usage(
    State(state): State<TeamAppState>,
    Extension(ctx): Extension<TenantContext>,
) -> Result<impl IntoResponse, StatusCode> {
    let usage = state.usage.get_usage(&ctx.tenant_id).await;

    let tenant = state
        .registry
        .get_tenant(&ctx.tenant_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(UsageResponse {
        api_calls_today: usage.api_calls_today(),
        api_calls_this_month: usage.api_calls_this_month(),
        llm_tokens_this_month: usage.llm_tokens_this_month(),
        publishes_this_month: usage.publishes_this_month(),
        plan: tenant.plan.label().to_string(),
        daily_limit: tenant.settings.max_api_calls_per_day,
    }))
}

/// GET /api/v1/health — Health check.
pub async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "teamd",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ═══════════════════════════════════════════════════════════════════
// Section 9: Parsing helpers
// ═══════════════════════════════════════════════════════════════════

fn parse_role(s: &str) -> Option<BusinessRole> {
    match s.to_lowercase().as_str() {
        "ceo" => Some(BusinessRole::Ceo),
        "cto" => Some(BusinessRole::Cto),
        "cfo" => Some(BusinessRole::Cfo),
        "cmo" => Some(BusinessRole::Cmo),
        "coo" => Some(BusinessRole::Coo),
        "developer" | "dev" => Some(BusinessRole::Developer),
        "designer" => Some(BusinessRole::Designer),
        "marketer" | "marketing" => Some(BusinessRole::Marketer),
        "analyst" => Some(BusinessRole::Analyst),
        "writer" => Some(BusinessRole::Writer),
        "support" => Some(BusinessRole::Support),
        "hr" => Some(BusinessRole::Hr),
        "sales" => Some(BusinessRole::Sales),
        "legal" => Some(BusinessRole::Legal),
        _ => None,
    }
}

fn parse_member_status(s: &str) -> Option<MemberStatus> {
    match s.to_lowercase().as_str() {
        "active" => Some(MemberStatus::Active),
        "idle" => Some(MemberStatus::Idle),
        "busy" => Some(MemberStatus::Busy),
        "disabled" => Some(MemberStatus::Disabled),
        _ => None,
    }
}

fn parse_channel_type(s: &str) -> Option<ChannelType> {
    match s.to_lowercase().as_str() {
        "blog" => Some(ChannelType::Blog),
        "twitter" | "x" => Some(ChannelType::Twitter),
        "reddit" => Some(ChannelType::Reddit),
        "hackernews" | "hn" => Some(ChannelType::HackerNews),
        "email" => Some(ChannelType::Email),
        "linkedin" => Some(ChannelType::LinkedIn),
        "producthunt" | "product_hunt" => Some(ChannelType::ProductHunt),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 10: Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role() {
        assert_eq!(parse_role("ceo"), Some(BusinessRole::Ceo));
        assert_eq!(parse_role("CEO"), Some(BusinessRole::Ceo));
        assert_eq!(parse_role("developer"), Some(BusinessRole::Developer));
        assert_eq!(parse_role("dev"), Some(BusinessRole::Developer));
        assert_eq!(parse_role("marketing"), Some(BusinessRole::Marketer));
        assert_eq!(parse_role("unknown"), None);
    }

    #[test]
    fn test_parse_member_status() {
        assert_eq!(parse_member_status("active"), Some(MemberStatus::Active));
        assert_eq!(parse_member_status("DISABLED"), Some(MemberStatus::Disabled));
        assert_eq!(parse_member_status("invalid"), None);
    }

    #[test]
    fn test_parse_channel_type() {
        assert_eq!(parse_channel_type("blog"), Some(ChannelType::Blog));
        assert_eq!(parse_channel_type("x"), Some(ChannelType::Twitter));
        assert_eq!(parse_channel_type("hn"), Some(ChannelType::HackerNews));
        assert_eq!(parse_channel_type("unknown"), None);
    }

    #[tokio::test]
    async fn test_task_routing_via_api_types() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("API Test", TenantPlan::Free)
            .await
            .unwrap();

        let orch = registry.get_orchestrator(&tenant.id).await.unwrap();
        let orch = orch.read().await;

        // Route a marketing task
        let member = orch.route_task("write a blog post about our new product launch");
        assert!(member.is_some());
        let member = member.unwrap();
        // Writer, Marketer, or CMO should win this bid
        assert!(
            member.role == BusinessRole::Writer
                || member.role == BusinessRole::Marketer
                || member.role == BusinessRole::Cmo,
            "Expected Writer, Marketer, or Cmo, got {:?}",
            member.role
        );
    }

    #[tokio::test]
    async fn test_brand_context_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("Brand Test", TenantPlan::Pro)
            .await
            .unwrap();

        let orch = registry.get_orchestrator(&tenant.id).await.unwrap();

        // Update brand
        {
            let mut orch = orch.write().await;
            orch.brand.name = "TestBrand".to_string();
            orch.brand.forbidden_words = vec!["spam".to_string()];
        }

        // Save state
        registry.save_tenant_state(&tenant.id).await.unwrap();

        // Evict and reload
        registry.evict(&tenant.id).await;
        let orch2 = registry.get_orchestrator(&tenant.id).await.unwrap();
        let orch2 = orch2.read().await;

        assert_eq!(orch2.brand.name, "TestBrand");
        assert_eq!(orch2.brand.forbidden_words, vec!["spam".to_string()]);
    }

    #[tokio::test]
    async fn test_campaign_creation_via_store() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("Campaign Test", TenantPlan::Starter)
            .await
            .unwrap();

        let orch = registry.get_orchestrator(&tenant.id).await.unwrap();

        let campaign_id = {
            let mut orch = orch.write().await;
            let campaign = orch.campaign_store.create_campaign(
                "Launch Campaign",
                "Increase awareness",
                vec![ChannelType::Blog, ChannelType::Twitter],
            );
            campaign.id.clone()
        };

        assert!(!campaign_id.is_empty());

        let orch = orch.read().await;
        let snapshot = orch.campaign_store.campaign_snapshot(&campaign_id);
        assert!(snapshot.is_some());
    }
}
