use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const ONBOARDING_CONTRACT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnboardingGate {
    pub id: String,
    pub label: String,
    pub required: bool,
    pub evidence_hint: String,
    #[serde(default)]
    pub keyword_any: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnboardingPackContract {
    pub id: String,
    pub vertical: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub kpi_gates: Vec<OnboardingGate>,
    #[serde(default)]
    pub risk_gates: Vec<OnboardingGate>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnboardingGateResult {
    pub id: String,
    pub label: String,
    pub required: bool,
    pub satisfied: bool,
    pub evidence_hint: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OnboardingPackContractFile {
    #[serde(default = "default_contract_schema")]
    schema_version: u32,
    #[serde(flatten)]
    contract: OnboardingPackContract,
}

fn default_contract_schema() -> u32 {
    ONBOARDING_CONTRACT_SCHEMA_VERSION
}

pub fn contracts_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HSM_ONBOARDING_CONTRACTS_DIR") {
        let t = dir.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join("company_os")
        .join("onboarding_contracts")
}

pub fn load_contracts_hot() -> anyhow::Result<Vec<OnboardingPackContract>> {
    let dir = contracts_dir();
    load_contracts_from_dir(&dir)
}

pub fn load_contracts_from_dir(dir: &Path) -> anyhow::Result<Vec<OnboardingPackContract>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("read onboarding contracts dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("yaml") || e.eq_ignore_ascii_case("yml"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    if files.is_empty() {
        anyhow::bail!("no onboarding contract YAML files in {}", dir.display());
    }

    let mut out = Vec::new();
    for f in files {
        let raw = fs::read_to_string(&f).with_context(|| format!("read {}", f.display()))?;
        let parsed: OnboardingPackContractFile =
            serde_yaml::from_str(&raw).with_context(|| format!("parse {}", f.display()))?;
        if parsed.schema_version != ONBOARDING_CONTRACT_SCHEMA_VERSION {
            anyhow::bail!(
                "{}: schema_version must be {} (got {})",
                f.display(),
                ONBOARDING_CONTRACT_SCHEMA_VERSION,
                parsed.schema_version
            );
        }
        out.push(parsed.contract);
    }
    validate_contracts(&out)?;
    Ok(out)
}

pub fn validate_contracts(contracts: &[OnboardingPackContract]) -> anyhow::Result<()> {
    if contracts.is_empty() {
        anyhow::bail!("contract catalog is empty");
    }
    let mut ids = HashSet::new();
    for c in contracts {
        if c.id.trim().is_empty() {
            anyhow::bail!("contract id must be non-empty");
        }
        if c.vertical.trim().is_empty() {
            anyhow::bail!("contract {} vertical must be non-empty", c.id);
        }
        if !ids.insert(c.id.clone()) {
            anyhow::bail!("duplicate contract id {}", c.id);
        }
        let mut gate_ids = BTreeSet::new();
        for g in c.kpi_gates.iter().chain(c.risk_gates.iter()) {
            if g.id.trim().is_empty() {
                anyhow::bail!("contract {} contains gate with empty id", c.id);
            }
            if !gate_ids.insert(g.id.clone()) {
                anyhow::bail!("contract {} has duplicate gate id {}", c.id, g.id);
            }
            if g.required && g.keyword_any.is_empty() {
                anyhow::bail!(
                    "contract {} gate {} is required but has no keyword_any matcher",
                    c.id,
                    g.id
                );
            }
        }
    }
    Ok(())
}

pub fn find_contract(
    contracts: &[OnboardingPackContract],
    pack_contract_id: &str,
    vertical_hint: &str,
) -> OnboardingPackContract {
    let id = pack_contract_id.trim().to_ascii_lowercase();
    if !id.is_empty() {
        if let Some(p) = contracts
            .iter()
            .find(|p| p.id.to_ascii_lowercase() == id)
            .cloned()
        {
            return p;
        }
    }
    let v = vertical_hint.trim().to_ascii_lowercase();
    if let Some(p) = contracts
        .iter()
        .find(|p| p.vertical.to_ascii_lowercase() == v)
        .cloned()
    {
        return p;
    }
    contracts[0].clone()
}

pub fn evaluate_gate_results(transcript: &str, gates: &[OnboardingGate]) -> Vec<OnboardingGateResult> {
    let transcript_lc = transcript.to_ascii_lowercase();
    gates.iter()
        .map(|g| {
            let satisfied = g
                .keyword_any
                .iter()
                .any(|kw| !kw.trim().is_empty() && transcript_lc.contains(&kw.to_ascii_lowercase()));
            OnboardingGateResult {
                id: g.id.clone(),
                label: g.label.clone(),
                required: g.required,
                satisfied,
                evidence_hint: g.evidence_hint.clone(),
            }
        })
        .collect()
}
