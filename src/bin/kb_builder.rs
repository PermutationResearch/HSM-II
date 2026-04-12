use anyhow::{Context, Result};
use chrono::Utc;
use std::fs;
use std::path::Path;

fn boilerplate(title: &str, body: &str) -> String {
    format!(
        "# {title}\n\n*Last updated:* {date}\n\n{body}\n",
        title = title,
        date = Utc::now().format("%Y-%m-%d"),
        body = body
    )
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }
    if path.exists() {
        println!("WARN overwrite {}", path.display());
    }
    fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    let kb_root = Path::new("company-files");
    fs::create_dir_all(kb_root).with_context(|| format!("create dir {}", kb_root.display()))?;

    let client_registry = boilerplate(
        "Client Registry",
        r#"| Client ID | Name | Industry | Revenue/hr | Tasks Failed | Status |
|-----------|------|----------|------------|--------------|--------|
| 001 | Acme Corp | FinTech | $12.0 | 0 | Active |
| 002 | Nova Labs | Biotech | $8.0 | 0 | Active |
| ... | ... | ... | ... | ... | ... |

Keep this file up to date via the client-registry admin workflow."#,
    );
    write_file(&kb_root.join("client_registry.md"), &client_registry)?;

    let onboarding = boilerplate(
        "Onboarding Checklist",
        r#"### Core Steps
- [ ] Verify KYC/AML compliance (Compliance Analyst)
- [ ] Sign Master Services Agreement
- [ ] Capture revenue-per-hour estimate
- [ ] Add to `client_registry.md`
- [ ] Assign primary point-of-contact (Research, Crypto, or Engineering)

### Quick Links
- `client_registry.md`
- `sop/research_sop.md`
- `sop/crypto_sop.md`
- `sop/engineering_sop.md`"#,
    );
    write_file(&kb_root.join("onboarding_checklist.md"), &onboarding)?;

    let research_sop = boilerplate(
        "Research SOP",
        r#"1. Pull latest topics from `research-brief` feed.
2. Allocate one analyst per topic.
3. Draft 800-word brief, review, then publish to `research-hub`.
4. Repurpose into LinkedIn carousel and X thread (see `content_calendar.md`)."#,
    );
    write_file(&kb_root.join("sop").join("research_sop.md"), &research_sop)?;

    let crypto_sop = boilerplate(
        "Crypto SOP",
        r#"1. Run `defi-monitor` nightly.
2. Flag any transfer above $500k or protocol risk event.
3. Draft alert, post internally, then emit client digest.
4. Log event in `on_chain_log.md`."#,
    );
    write_file(&kb_root.join("sop").join("crypto_sop.md"), &crypto_sop)?;

    let engineering_sop = boilerplate(
        "Engineering SOP",
        r#"1. Triage incoming GitHub issues via `issue-triage`.
2. Prioritize by impact score and client SLA.
3. Assign to sprint backlog, run CI, merge via `pr-review`.
4. Update `release_notes.md` after deployment."#,
    );
    write_file(
        &kb_root.join("sop").join("engineering_sop.md"),
        &engineering_sop,
    )?;

    let compliance = boilerplate(
        "Compliance Matrix",
        r#"| Regulation | Scope | Owner | Review Frequency |
|------------|-------|-------|------------------|
| GDPR | EU data | Crypto Analyst | Quarterly |
| CCPA | US data | Research Analyst | Annually |
| SOC2 | Service Ops | Engineering Lead | Bi-annual |
| AML/KYC | All clients | Compliance Lead | Ongoing |"#,
    );
    write_file(&kb_root.join("compliance_matrix.md"), &compliance)?;

    let kpi = boilerplate(
        "KPI Dashboard Template",
        r#"## Weekly KPI Dashboard

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| ARR Growth | +8% | 5% | down |
| Revenue/hr per employee | $12 | $9.3 | down |
| Task Success Rate | 98% | 96% | warning |
| On-time Delivery | 95% | 92% | warning |
| Client churn | <2% | 1.5% | good |

Data source: `goal-tracker` and `heartbeat` logs."#,
    );
    write_file(&kb_root.join("kpi_dashboard_template.md"), &kpi)?;

    let manifest = r#"[[file]]
path = "client_registry.md"
type = "registry"

[[file]]
path = "onboarding_checklist.md"
type = "checklist"

[[file]]
path = "sop/research_sop.md"
type = "sop"

[[file]]
path = "sop/crypto_sop.md"
type = "sop"

[[file]]
path = "sop/engineering_sop.md"
type = "sop"

[[file]]
path = "compliance_matrix.md"
type = "compliance"

[[file]]
path = "kpi_dashboard_template.md"
type = "template"
"#;
    write_file(&kb_root.join("manifest.toml"), manifest)?;

    println!("OK knowledge-base assets generated under {}", kb_root.display());
    Ok(())
}
