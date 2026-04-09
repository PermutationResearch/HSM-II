//! Plugin lifecycle primitives: manifest validation, enable/disable, registry wiring.

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::warn;

use super::schemas::{ToolProviderKind, ToolProviderMetadata, ToolRegistry, ToolSchema, ValidationError};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub provider: ToolProviderMetadata,
    #[serde(default)]
    pub tools: Vec<ToolSchema>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub checksum_sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginStateIndex {
    pub enabled: HashMap<String, bool>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum VulnSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl VulnSeverity {
    fn score(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct VulnFinding {
    finding_id: String,
    severity: VulnSeverity,
    kind: String,
    message: String,
    evidence: String,
    recommendation: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum VulnScanMode {
    Off,
    Audit,
    Enforce,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct McpVulnScanReport {
    plugin_id: String,
    provider_id: String,
    endpoint: Option<String>,
    mode: VulnScanMode,
    scanned_at_ms: u128,
    blocked: bool,
    findings: Vec<VulnFinding>,
}

pub struct PluginManager {
    manifest_dir: PathBuf,
    state_path: PathBuf,
    allow_unsigned: bool,
}

impl PluginManager {
    pub fn new(manifest_dir: PathBuf, state_path: PathBuf, allow_unsigned: bool) -> Self {
        Self {
            manifest_dir,
            state_path,
            allow_unsigned,
        }
    }

    pub fn from_env() -> Self {
        let cfg = crate::harness::RuntimeConfig::from_env();
        Self::new(
            cfg.plugins.manifest_dir,
            cfg.state_dir.join("plugin_state.json"),
            cfg.plugins.allow_unsigned,
        )
    }

    fn load_state(&self) -> Result<PluginStateIndex> {
        if !self.state_path.exists() {
            return Ok(PluginStateIndex::default());
        }
        Ok(serde_json::from_slice(&fs::read(&self.state_path)?)?)
    }

    fn save_state(&self, state: &PluginStateIndex) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.state_path, serde_json::to_vec_pretty(state)?)?;
        Ok(())
    }

    pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<()> {
        let mut state = self.load_state()?;
        state.enabled.insert(plugin_id.to_string(), enabled);
        self.save_state(&state)
    }

    pub fn list_manifests(&self) -> Result<Vec<PluginManifest>> {
        if !self.manifest_dir.exists() {
            return Ok(Vec::new());
        }
        let state = self.load_state()?;
        let mut manifests = Vec::new();
        for entry in fs::read_dir(&self.manifest_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read(&path)?;
            let mut manifest: PluginManifest = serde_json::from_slice(&raw)
                .map_err(|e| anyhow!("invalid manifest {}: {}", path.display(), e))?;
            self.verify_checksum(&path, &raw, &manifest)?;
            self.scan_manifest_security(&manifest, &path)?;
            if let Some(v) = state.enabled.get(&manifest.id) {
                manifest.enabled = *v;
            }
            manifests.push(manifest);
        }
        manifests.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(manifests)
    }

    fn verify_checksum(&self, path: &Path, raw: &[u8], manifest: &PluginManifest) -> Result<()> {
        let Some(expected) = manifest.checksum_sha256.as_deref() else {
            if self.allow_unsigned {
                return Ok(());
            }
            return Err(anyhow!(
                "plugin {} missing checksum_sha256: {}",
                manifest.id,
                path.display()
            ));
        };
        let got = format!("{:x}", Sha256::digest(raw));
        if got != expected {
            return Err(anyhow!(
                "plugin {} checksum mismatch: expected {}, got {}",
                manifest.id,
                expected,
                got
            ));
        }
        Ok(())
    }

    fn env_truthy(name: &str, default: bool) -> bool {
        std::env::var(name)
            .map(|v| {
                let s = v.trim();
                s == "1"
                    || s.eq_ignore_ascii_case("true")
                    || s.eq_ignore_ascii_case("yes")
                    || s.eq_ignore_ascii_case("on")
            })
            .unwrap_or(default)
    }

    fn scan_mode() -> VulnScanMode {
        if !Self::env_truthy("HSM_MCP_VULN_SCAN", true) {
            return VulnScanMode::Off;
        }
        match std::env::var("HSM_MCP_VULN_SCAN_MODE")
            .ok()
            .unwrap_or_else(|| "enforce".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "off" => VulnScanMode::Off,
            "audit" | "warn" => VulnScanMode::Audit,
            _ => VulnScanMode::Enforce,
        }
    }

    fn report_dir(&self) -> PathBuf {
        self.state_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("mcp_vuln_reports")
    }

    fn write_report(&self, report: &McpVulnScanReport) {
        let dir = self.report_dir();
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let filename = format!("{}.json", sanitize_filename(&report.plugin_id));
        let path = dir.join(filename);
        if let Ok(payload) = serde_json::to_vec_pretty(report) {
            let _ = fs::write(path, payload);
        }
    }

    fn push_finding(
        findings: &mut Vec<VulnFinding>,
        severity: VulnSeverity,
        id: &str,
        kind: &str,
        msg: impl Into<String>,
        evidence: impl Into<String>,
        recommendation: impl Into<String>,
    ) {
        findings.push(VulnFinding {
            finding_id: id.to_string(),
            severity,
            kind: kind.to_string(),
            message: msg.into(),
            evidence: evidence.into(),
            recommendation: recommendation.into(),
        });
    }

    fn blocked_host(host: &str) -> bool {
        let h = host.trim().to_ascii_lowercase();
        if h.is_empty()
            || h == "localhost"
            || h == "::1"
            || h == "0.0.0.0"
            || h.ends_with(".local")
            || h.ends_with(".internal")
        {
            return true;
        }
        if let Ok(ip) = h.parse::<IpAddr>() {
            match ip {
                IpAddr::V4(v4) => {
                    return v4.is_private()
                        || v4.is_loopback()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                        || v4.is_multicast();
                }
                IpAddr::V6(v6) => {
                    return v6.is_loopback()
                        || v6.is_unique_local()
                        || v6.is_unspecified()
                        || v6.is_multicast();
                }
            }
        }
        false
    }

    fn scan_manifest_security(&self, manifest: &PluginManifest, path: &Path) -> Result<()> {
        let mode = Self::scan_mode();
        if mode == VulnScanMode::Off {
            return Ok(());
        }
        if manifest.provider.kind != ToolProviderKind::Mcp {
            return Ok(());
        }
        let allow_insecure = Self::env_truthy("HSM_MCP_ALLOW_INSECURE", false);

        let mut findings = Vec::new();
        let endpoint = manifest.provider.endpoint.clone();
        let endpoint_trimmed = endpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let Some(endpoint) = endpoint_trimmed else {
            Self::push_finding(
                &mut findings,
                VulnSeverity::High,
                "mcp_missing_endpoint",
                "configuration",
                "MCP provider endpoint is missing",
                path.display().to_string(),
                "Set provider.endpoint to a trusted HTTPS URL",
            );
            let report = McpVulnScanReport {
                plugin_id: manifest.id.clone(),
                provider_id: manifest.provider.id.clone(),
                endpoint: None,
                mode,
                scanned_at_ms: unix_now_ms(),
                blocked: mode == VulnScanMode::Enforce,
                findings,
            };
            self.write_report(&report);
            if report.blocked {
                return Err(anyhow!("MCP provider `{}` blocked: endpoint missing", manifest.id));
            }
            warn!(target: "hsm.mcp.scan", plugin = %manifest.id, "MCP endpoint missing (audit-only mode)");
            return Ok(());
        };

        match reqwest::Url::parse(&endpoint) {
            Err(_) => Self::push_finding(
                &mut findings,
                VulnSeverity::Critical,
                "mcp_invalid_endpoint_url",
                "url",
                "MCP endpoint is not a valid URL",
                endpoint.clone(),
                "Fix endpoint format; use a fully-qualified HTTPS URL",
            ),
            Ok(url) => {
                let scheme = url.scheme().to_ascii_lowercase();
                let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
                if !allow_insecure && scheme != "https" {
                    Self::push_finding(
                        &mut findings,
                        VulnSeverity::High,
                        "mcp_non_https",
                        "transport",
                        "MCP endpoint uses non-HTTPS transport",
                        endpoint.clone(),
                        "Require HTTPS endpoints for MCP providers",
                    );
                }
                if url.username().len() > 0 || url.password().is_some() {
                    Self::push_finding(
                        &mut findings,
                        VulnSeverity::Critical,
                        "mcp_url_embedded_credentials",
                        "secrets",
                        "MCP endpoint contains embedded credentials",
                        endpoint.clone(),
                        "Remove credentials from URL and use secure headers/secrets storage",
                    );
                }
                if !allow_insecure && Self::blocked_host(&host) {
                    Self::push_finding(
                        &mut findings,
                        VulnSeverity::Critical,
                        "mcp_private_or_local_host",
                        "network",
                        "MCP endpoint host is private/local/loopback",
                        host.clone(),
                        "Use externally reachable trusted host or set HSM_MCP_ALLOW_INSECURE=1 for dev only",
                    );
                }
                if host.contains("pastebin")
                    || host.contains("ngrok")
                    || host.contains("trycloudflare")
                    || host.contains("webhook.site")
                {
                    Self::push_finding(
                        &mut findings,
                        VulnSeverity::High,
                        "mcp_tunnel_or_transient_host",
                        "network",
                        "MCP endpoint host is transient/high-risk",
                        host,
                        "Pin MCP endpoints to stable trusted domains",
                    );
                }
                if let Some(port) = url.port() {
                    if !matches!(port, 443 | 8443) {
                        Self::push_finding(
                            &mut findings,
                            VulnSeverity::Medium,
                            "mcp_unusual_port",
                            "network",
                            "MCP endpoint uses unusual port",
                            format!("{}:{}", url.host_str().unwrap_or_default(), port),
                            "Confirm this is intentional and restrict egress to the endpoint",
                        );
                    }
                }
            }
        }

        let mut names = std::collections::HashSet::new();
        for t in &manifest.tools {
            if !names.insert(t.name.to_ascii_lowercase()) {
                Self::push_finding(
                    &mut findings,
                    VulnSeverity::Medium,
                    "mcp_duplicate_tool_name",
                    "schema",
                    "Duplicate MCP tool names in manifest",
                    t.name.clone(),
                    "Ensure each tool.name is unique per provider",
                );
            }
            if t.name.contains('/') || t.name.contains("..") || t.name.contains('\\') {
                Self::push_finding(
                    &mut findings,
                    VulnSeverity::High,
                    "mcp_suspicious_tool_name",
                    "schema",
                    "Tool name includes suspicious path-like characters",
                    t.name.clone(),
                    "Use stable alphanumeric tool names without path separators",
                );
            }
        }
        if manifest.tools.is_empty() {
            Self::push_finding(
                &mut findings,
                VulnSeverity::Low,
                "mcp_no_static_tools",
                "schema",
                "Manifest has no static tools; runtime discovery required",
                manifest.id.clone(),
                "Either define tools statically or ensure tools/list discovery is pinned and monitored",
            );
        }

        let max_severity = findings
            .iter()
            .map(|f| f.severity.score())
            .max()
            .unwrap_or(0);
        let blocked = mode == VulnScanMode::Enforce && max_severity >= VulnSeverity::High.score();
        let report = McpVulnScanReport {
            plugin_id: manifest.id.clone(),
            provider_id: manifest.provider.id.clone(),
            endpoint: Some(endpoint.clone()),
            mode,
            scanned_at_ms: unix_now_ms(),
            blocked,
            findings: findings.clone(),
        };
        self.write_report(&report);
        if blocked {
            let summary = findings
                .iter()
                .filter(|f| f.severity.score() >= VulnSeverity::High.score())
                .map(|f| format!("{}({})", f.finding_id, f.severity.score()))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(anyhow!(
                "MCP provider `{}` blocked by vulnerability scan: {}",
                manifest.id,
                summary
            ));
        }
        if !findings.is_empty() && mode == VulnScanMode::Audit {
            warn!(
                target: "hsm.mcp.scan",
                plugin = %manifest.id,
                findings = findings.len(),
                "MCP vulnerability scan findings (audit mode)"
            );
        }
        Ok(())
    }

    pub fn register_enabled_into_registry(&self, registry: &mut ToolRegistry) -> Result<()> {
        for manifest in self.list_manifests()? {
            if !manifest.enabled {
                continue;
            }
            registry.register_provider(manifest.provider.clone());
            for tool in manifest.tools {
                registry
                    .register_external_tool(tool, &manifest.provider.id)
                    .map_err(|e| map_validation(e, &manifest.id))?;
            }
        }
        Ok(())
    }
}

fn map_validation(err: ValidationError, plugin_id: &str) -> anyhow::Error {
    anyhow!("plugin {} registration failed: {}", plugin_id, err)
}

fn unix_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn sanitize_filename(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars().take(120) {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}
