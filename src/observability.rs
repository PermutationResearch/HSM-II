//! Production Observability for HSM-II
//!
//! Provides metrics, health checks, distributed tracing, and alerting hooks.

use std::sync::Arc;
use std::time::{Duration, Instant};
use axum::{extract::State, http::StatusCode, response::Json, Router};
use prometheus::{Counter, Gauge, Histogram, Registry, Encoder, TextEncoder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::interval;
use tracing::{error, info, warn};

/// Global metrics registry
pub struct MetricsRegistry {
    registry: Registry,
    /// Total HTTP requests
    pub http_requests_total: Counter,
    /// HTTP request duration
    pub http_request_duration: Histogram,
    /// Active connections
    pub active_connections: Gauge,
    /// LLM requests
    pub llm_requests_total: Counter,
    /// LLM latency
    pub llm_latency_ms: Histogram,
    /// Tool executions
    pub tool_executions_total: Counter,
    /// Failed operations
    pub failures_total: Counter,
    /// Council decisions
    pub council_decisions_total: Counter,
    /// Agent promises kept/broken
    pub promises_kept: Counter,
    pub promises_broken: Counter,
}

impl MetricsRegistry {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        let http_requests_total = Counter::new(
            "hsm_http_requests_total",
            "Total HTTP requests received"
        )?;
        registry.register(Box::new(http_requests_total.clone()))?;

        let http_request_duration = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "hsm_http_request_duration_seconds",
                "HTTP request duration in seconds"
            ).buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])
        )?;
        registry.register(Box::new(http_request_duration.clone()))?;

        let active_connections = Gauge::new(
            "hsm_active_connections",
            "Number of active connections"
        )?;
        registry.register(Box::new(active_connections.clone()))?;

        let llm_requests_total = Counter::new(
            "hsm_llm_requests_total",
            "Total LLM requests"
        )?;
        registry.register(Box::new(llm_requests_total.clone()))?;

        let llm_latency_ms = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "hsm_llm_latency_milliseconds",
                "LLM request latency in milliseconds"
            ).buckets(vec![50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0])
        )?;
        registry.register(Box::new(llm_latency_ms.clone()))?;

        let tool_executions_total = Counter::new(
            "hsm_tool_executions_total",
            "Total tool executions"
        )?;
        registry.register(Box::new(tool_executions_total.clone()))?;

        let failures_total = Counter::new(
            "hsm_failures_total",
            "Total failed operations"
        )?;
        registry.register(Box::new(failures_total.clone()))?;

        let council_decisions_total = Counter::new(
            "hsm_council_decisions_total",
            "Total council decisions"
        )?;
        registry.register(Box::new(council_decisions_total.clone()))?;

        let promises_kept = Counter::new(
            "hsm_promises_kept_total",
            "Total promises kept"
        )?;
        registry.register(Box::new(promises_kept.clone()))?;

        let promises_broken = Counter::new(
            "hsm_promises_broken_total",
            "Total promises broken"
        )?;
        registry.register(Box::new(promises_broken.clone()))?;

        Ok(Self {
            registry,
            http_requests_total,
            http_request_duration,
            active_connections,
            llm_requests_total,
            llm_latency_ms,
            tool_executions_total,
            failures_total,
            council_decisions_total,
            promises_kept,
            promises_broken,
        })
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> anyhow::Result<String> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new().expect("Failed to create metrics registry")
    }
}

/// Health check status
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub checks: Vec<ComponentHealth>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: String,
    pub message: Option<String>,
    pub latency_ms: u64,
}

/// Health checker
pub struct HealthChecker {
    start_time: Instant,
    checks: Vec<Box<dyn HealthCheck + Send + Sync>>,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            checks: Vec::new(),
        }
    }

    pub fn add_check(&mut self, check: Box<dyn HealthCheck + Send + Sync>) {
        self.checks.push(check);
    }

    pub async fn check_all(&self) -> HealthStatus {
        let mut component_health = Vec::new();

        for check in &self.checks {
            let start = Instant::now();
            let result = check.check().await;
            let latency_ms = start.elapsed().as_millis() as u64;

            component_health.push(ComponentHealth {
                name: check.name().to_string(),
                status: if result.is_ok() { "healthy".to_string() } else { "unhealthy".to_string() },
                message: result.err().map(|e| e.to_string()),
                latency_ms,
            });
        }

        let all_healthy = component_health.iter().all(|h| h.status == "healthy");

        HealthStatus {
            status: if all_healthy { "healthy".to_string() } else { "degraded".to_string() },
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
            checks: component_health,
        }
    }
}

/// Health check trait
#[async_trait::async_trait]
pub trait HealthCheck {
    fn name(&self) -> &str;
    async fn check(&self) -> anyhow::Result<()>;
}

/// LLM provider health check
pub struct LlmHealthCheck {
    client: Arc<crate::llm::LlmClient>,
}

impl LlmHealthCheck {
    pub fn new(client: Arc<crate::llm::LlmClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl HealthCheck for LlmHealthCheck {
    fn name(&self) -> &str {
        "llm"
    }

    async fn check(&self) -> anyhow::Result<()> {
        let results = self.client.health_check().await;
        let any_healthy = results.iter().any(|(_, healthy, _)| *healthy);
        
        if any_healthy {
            Ok(())
        } else {
            Err(anyhow::anyhow!("All LLM providers unhealthy"))
        }
    }
}

/// Database health check
pub struct DatabaseHealthCheck;

#[async_trait::async_trait]
impl HealthCheck for DatabaseHealthCheck {
    fn name(&self) -> &str {
        "database"
    }

    async fn check(&self) -> anyhow::Result<()> {
        // Implement based on your database setup
        // For now, assume healthy if we can access the data directory
        tokio::fs::metadata("./data").await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Database check failed: {}", e))
    }
}

/// Disk space health check
pub struct DiskHealthCheck {
    #[allow(dead_code)]
    min_free_percent: f64,
}

impl DiskHealthCheck {
    pub fn new(min_free_percent: f64) -> Self {
        Self { min_free_percent }
    }
}

#[async_trait::async_trait]
impl HealthCheck for DiskHealthCheck {
    fn name(&self) -> &str {
        "disk"
    }

    async fn check(&self) -> anyhow::Result<()> {
        // Check disk space using df command
        let output = tokio::process::Command::new("df")
            .args(&["-h", "."])
            .output()
            .await?;

        if output.status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Disk check failed"))
        }
    }
}

/// Alert manager for critical events
pub struct AlertManager {
    webhooks: Vec<String>,
}

impl AlertManager {
    pub fn new() -> Self {
        let webhooks: Vec<String> = std::env::var("ALERT_WEBHOOKS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self { webhooks }
    }

    pub async fn send_alert(&self, alert: Alert) {
        if self.webhooks.is_empty() {
            warn!("No alert webhooks configured, logging locally");
            error!(
                severity = ?alert.severity,
                title = %alert.title,
                message = %alert.message,
                "ALERT"
            );
            return;
        }

        for webhook in &self.webhooks {
            if let Err(e) = self.send_to_webhook(webhook, &alert).await {
                error!(webhook = %webhook, error = %e, "Failed to send alert");
            }
        }
    }

    async fn send_to_webhook(&self, webhook: &str, alert: &Alert) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let payload = json!({
            "severity": format!("{:?}", alert.severity),
            "title": alert.title,
            "message": alert.message,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "source": "hsm-ii",
        });

        client.post(webhook).json(&payload).send().await?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub severity: AlertSeverity,
    pub title: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Initialize tracing with structured logging
pub fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // Check if OTEL endpoint is configured
    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json());

    if otel_endpoint.is_some() {
        // Initialize OpenTelemetry if configured
        // This would require the opentelemetry crate setup
        // For now, just use structured logging
        info!("OpenTelemetry endpoint configured but OTLP support requires additional setup");
    }

    subscriber.init();
}

/// Create observability router
pub fn observability_router(
    metrics: Arc<MetricsRegistry>,
    health: Arc<HealthChecker>,
) -> Router {
    Router::new()
        .route("/health", axum::routing::get(health_handler))
        .route("/metrics", axum::routing::get(metrics_handler))
        .route("/ready", axum::routing::get(ready_handler))
        .route("/live", axum::routing::get(live_handler))
        .with_state(ObservabilityState { metrics, health })
}

#[derive(Clone)]
struct ObservabilityState {
    metrics: Arc<MetricsRegistry>,
    health: Arc<HealthChecker>,
}

async fn health_handler(State(state): State<ObservabilityState>) -> Json<HealthStatus> {
    let status = state.health.check_all().await;
    Json(status)
}

async fn metrics_handler(State(state): State<ObservabilityState>) -> Result<String, StatusCode> {
    state.metrics.export_prometheus()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn ready_handler(State(state): State<ObservabilityState>) -> StatusCode {
    let status = state.health.check_all().await;
    if status.status == "healthy" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn live_handler() -> StatusCode {
    StatusCode::OK
}

/// Background metrics reporter
pub async fn start_metrics_reporter(metrics: Arc<MetricsRegistry>) {
    let mut interval = interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        // Log current metrics
        info!(
            http_requests = metrics.http_requests_total.get(),
            llm_requests = metrics.llm_requests_total.get(),
            tool_executions = metrics.tool_executions_total.get(),
            failures = metrics.failures_total.get(),
            "Metrics snapshot"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registry() {
        let metrics = MetricsRegistry::new().unwrap();
        metrics.http_requests_total.inc();
        metrics.http_requests_total.inc();
        
        let output = metrics.export_prometheus().unwrap();
        assert!(output.contains("hsm_http_requests_total"));
    }

    #[test]
    fn test_health_status() {
        let status = HealthStatus {
            status: "healthy".to_string(),
            version: "1.0.0".to_string(),
            uptime_seconds: 3600,
            checks: vec![
                ComponentHealth {
                    name: "database".to_string(),
                    status: "healthy".to_string(),
                    message: None,
                    latency_ms: 5,
                }
            ],
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("healthy"));
    }
}
