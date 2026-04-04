//! Machine-readable HSM-II architecture: layers, data flows, entry points.
//! Canonical document: `architecture/hsm-ii-blueprint.ron` (embedded + parsed at runtime).

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;

/// Full blueprint matching `architecture/hsm-ii-blueprint.ron`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureBlueprint {
    pub schema_version: u32,
    pub title: String,
    pub summary: String,
    pub layers: Vec<LayerSpec>,
    pub entry_points: Vec<String>,
    pub data_flows: Vec<DataFlowSpec>,
    pub shared_abstractions: Vec<String>,
    /// Markdown body: Company OS (Postgres) vs Paperclip IntelligenceLayer (in-process).
    pub dual_company_layers: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSpec {
    pub id: String,
    pub name: String,
    pub responsibility: String,
    pub key_abstraction: String,
    pub lives_inside: String,
    pub code_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlowSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
}

/// Live runtime stats from a running `HyperStigmergicMorphogenesis`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldArchitectureRuntime {
    pub beliefs: usize,
    pub experiences: usize,
    pub hyper_edges: usize,
    pub tick_count: u64,
    pub prev_coherence: f64,
    pub skill_bank_roots: usize,
}

pub fn default_blueprint_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("architecture/hsm-ii-blueprint.ron")
}

/// Load blueprint from a `.ron` file.
pub fn load_blueprint_from_path(path: &Path) -> anyhow::Result<ArchitectureBlueprint> {
    let raw = std::fs::read_to_string(path)?;
    let v: ArchitectureBlueprint = ron::de::from_str(&raw)?;
    Ok(v)
}

fn parse_embedded() -> anyhow::Result<ArchitectureBlueprint> {
    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/architecture/hsm-ii-blueprint.ron"
    ));
    Ok(ron::de::from_str(raw)?)
}

/// Parsed once at runtime (cheap to clone).
pub fn embedded_blueprint() -> ArchitectureBlueprint {
    static CELL: OnceLock<ArchitectureBlueprint> = OnceLock::new();
    CELL.get_or_init(|| {
        parse_embedded().expect("embedded architecture/hsm-ii-blueprint.ron is invalid")
    })
    .clone()
}

/// Human-readable Markdown report (with Mermaid diagram).
pub fn blueprint_markdown(bp: &ArchitectureBlueprint) -> String {
    let mut o = String::new();
    o.push_str(&format!("# {}\n\n", bp.title));
    o.push_str(&bp.summary);
    o.push_str("\n\n## Five Living Layers\n\n");
    o.push_str("| Layer | Responsibility | Key Abstraction | Lives Inside | Code Modules |\n");
    o.push_str("|-------|----------------|-----------------|--------------|--------------|\n");
    for l in &bp.layers {
        o.push_str(&format!(
            "| **{}** | {} | `{}` | {} | `{}` |\n",
            l.name,
            l.responsibility,
            l.key_abstraction,
            l.lives_inside,
            l.code_modules.join(", ")
        ));
    }

    o.push_str("\n## Data Flows (the only 5 paths that matter)\n\n");
    for f in &bp.data_flows {
        o.push_str(&format!("### {}\n\n{}\n\n", f.name, f.description));
        for (i, step) in f.steps.iter().enumerate() {
            o.push_str(&format!("{}. {}\n", i + 1, step));
        }
        o.push('\n');
    }

    o.push_str("\n## Dual Company Architecture\n\n");
    o.push_str(&bp.dual_company_layers);
    o.push('\n');

    o.push_str("## Entry Points (binaries)\n\n");
    for e in &bp.entry_points {
        o.push_str(&format!("- `{}`\n", e));
    }

    o.push_str("\n## Shared Abstractions (single source of truth)\n\n");
    for a in &bp.shared_abstractions {
        o.push_str(&format!("- `{}`\n", a));
    }

    o.push_str("\n");
    o.push_str(&mermaid_system_overview());
    o
}

/// Hub diagram: one world struct, five layers read/write the same hypergraph.
fn mermaid_system_overview() -> String {
    let mut s = String::from("## System Overview (Mermaid)\n\n```mermaid\n");
    s.push_str("flowchart TB\n");
    s.push_str("    HSM[\"HyperStigmergicMorphogenesis — single source of truth\"]\n");
    s.push_str("    WM[World Model]\n");
    s.push_str("    RL[Reasoning Layer]\n");
    s.push_str("    EL[Execution Layer]\n");
    s.push_str("    IL[\"Intelligence (Paperclip)\"]\n");
    s.push_str("    FI[\"Federation & Interfaces\"]\n");
    s.push_str("    HSM <--> WM\n");
    s.push_str("    HSM <--> RL\n");
    s.push_str("    HSM <--> EL\n");
    s.push_str("    HSM <--> IL\n");
    s.push_str("    HSM <--> FI\n");
    s.push_str("```\n");
    s
}

/// Same as [`blueprint_markdown`] plus a **Live World Model Stats** section (from a mounted or loaded world).
pub fn blueprint_markdown_with_runtime(bp: &ArchitectureBlueprint, runtime: &WorldArchitectureRuntime) -> String {
    let mut o = blueprint_markdown(bp);
    o.push_str("\n## Live World Model Stats (from instance)\n\n");
    o.push_str(&format!(
        "- Beliefs: **{}**\n- Experiences: **{}**\n- Hypergraph edges: **{}**\n- Current tick: **{}**\n- Coherence: **{:.3}**\n- Skill roots (general): **{}**\n",
        runtime.beliefs,
        runtime.experiences,
        runtime.hyper_edges,
        runtime.tick_count,
        runtime.prev_coherence,
        runtime.skill_bank_roots
    ));
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// `ARCHITECTURE.generated.md` must equal `blueprint_markdown` output or CI / `cargo test --lib` fails.
    /// Regenerate from repo root: `./scripts/generate-architecture-md.sh`
    #[test]
    fn generated_markdown_file_matches_blueprint_output() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("ARCHITECTURE.generated.md");
        let disk = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "read {}: {e}. Run ./scripts/generate-architecture-md.sh from the repo root.",
                path.display()
            );
        });
        let fresh = blueprint_markdown(&embedded_blueprint());
        assert_eq!(
            disk, fresh,
            "ARCHITECTURE.generated.md is out of date. From repo root run: ./scripts/generate-architecture-md.sh"
        );
    }

    #[test]
    fn embedded_ron_parses() {
        let bp = embedded_blueprint();
        assert_eq!(bp.schema_version, 1);
        assert_eq!(bp.layers.len(), 5);
        assert_eq!(bp.data_flows.len(), 5);
        assert!(!bp.dual_company_layers.is_empty());
    }

    #[test]
    fn markdown_includes_dual_company_section() {
        let md = blueprint_markdown(&embedded_blueprint());
        assert!(md.contains("## Dual Company Architecture"));
        assert!(md.contains("Company OS (PostgreSQL)"));
    }

    #[test]
    fn markdown_with_runtime_appends_section() {
        let bp = embedded_blueprint();
        let rt = WorldArchitectureRuntime {
            beliefs: 3,
            experiences: 2,
            hyper_edges: 10,
            tick_count: 99,
            prev_coherence: 0.5,
            skill_bank_roots: 4,
        };
        let md = blueprint_markdown_with_runtime(&bp, &rt);
        assert!(md.contains("Live World Model Stats"));
        assert!(md.contains("**3**"));
        assert!(md.contains("flowchart TB"));
        assert!(md.contains("HSM <--> WM"));
    }
}
