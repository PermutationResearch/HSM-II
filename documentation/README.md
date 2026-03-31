# HSM-II Documentation

Welcome to the HSM-II documentation. This directory contains comprehensive guides, references, and reports for the Hyper-Stigmergic Morphogenesis II system.

## 📚 Documentation Structure

### [guides/](guides/) — Getting Started & User Guides
Essential guides for running and using HSM-II:

| Document | Description |
|----------|-------------|
| [EASY_START.md](guides/EASY_START.md) | 🚀 Quick start guide — the simplest way to begin |
| [DEPLOYMENT.md](guides/DEPLOYMENT.md) | 🐳 Production deployment with Docker |
| [COMMANDS_GUIDE.md](guides/COMMANDS_GUIDE.md) | ⌨️ CLI commands and usage reference |
| [PERSONAL_AGENT_README.md](guides/PERSONAL_AGENT_README.md) | 🤖 Personal AI companion setup |
| [A2A_MESSAGE_CONTRACTS.md](guides/A2A_MESSAGE_CONTRACTS.md) | 🔌 JSON-RPC contracts for CEO/PM + engineer orchestration |
| [HARNESS_V1_PLAN.md](guides/HARNESS_V1_PLAN.md) | 🧱 Incremental file-by-file harness hardening plan |
| [REPL.md](guides/REPL.md) | 💻 Interactive REPL usage |

### [architecture/](architecture/) — System Design & Architecture
Deep dives into the HSM-II architecture:

| Document | Description |
|----------|-------------|
| [ANTIFRAGILE_ARCHITECTURE.md](architecture/ANTIFRAGILE_ARCHITECTURE.md) | 🏗️ Core architecture and antifragile design principles |
| [GROUNDED_HSMII_VISION.md](architecture/GROUNDED_HSMII_VISION.md) | 🎯 Vision and design philosophy |
| [MODES_DIAGRAM.md](architecture/MODES_DIAGRAM.md) | 📊 System modes and state transitions |
| [OUROBOROS_HSMII_PHASES.md](architecture/OUROBOROS_HSMII_PHASES.md) | 🔄 Ouroboros integration phases |

### [references/](references/) — Technical References
Detailed technical documentation:

| Document | Description |
|----------|-------------|
| [TOOLS_SUITE.md](references/TOOLS_SUITE.md) | 🛠️ Complete tool suite documentation (57 tools) |
| [RUST_TOOLS.md](references/RUST_TOOLS.md) | ⚙️ Rust development tools and utilities |
| [METRICS_README.md](references/METRICS_README.md) | 📈 Observability and metrics reference |
| [a2a_schemas/README.md](references/a2a_schemas/README.md) | 🧩 JSON Schema contracts for inter-agent JSON-RPC |
| [CLAUDE_PROMPT_CATALOG.md](references/CLAUDE_PROMPT_CATALOG.md) | 🗂️ Mapping of extracted prompt modules to numbered catalog concepts |
| [EXPERIMENTS.md](references/EXPERIMENTS.md) | 🧪 Experimental features and protocols |
| [INVESTIGATION_SYSTEM.md](references/INVESTIGATION_SYSTEM.md) | 🔍 Investigation and debugging tools |

### [integrations/](integrations/) — Third-Party Integrations
Connecting HSM-II with external systems:

| Document | Description |
|----------|-------------|
| [HERMES_INTEGRATION.md](integrations/HERMES_INTEGRATION.md) | 🔗 Hermes Agent bridge integration |
| [HERMES_INTEGRATION_QUICKSTART.md](integrations/HERMES_INTEGRATION_QUICKSTART.md) | ⚡ Quick start for Hermes |
| [HERMES_BRIDGE_STATUS.md](integrations/HERMES_BRIDGE_STATUS.md) | 📋 Hermes bridge current status |
| [WEB_SEARCH_CLOUDFLARE.md](integrations/WEB_SEARCH_CLOUDFLARE.md) | 🔎 Cloudflare web search setup |

### [reports/](reports/) — Research Reports & Analysis
Technical reports, test results, and analysis:

| Document | Description |
|----------|-------------|
| [IMPLEMENTATION_STATUS.md](reports/IMPLEMENTATION_STATUS.md) | ✅ Current feature completeness |
| [TRANSFORMATION_SUMMARY.md](reports/TRANSFORMATION_SUMMARY.md) | 📝 System evolution summary |
| [PRODUCTION_FIXES.md](reports/PRODUCTION_FIXES.md) | 🔧 Production readiness fixes |
| [PAPER_IMPLEMENTATION_COVERAGE.md](reports/PAPER_IMPLEMENTATION_COVERAGE.md) | 📄 Academic paper coverage |
| **Social Memory** | |
| [JW_SOCIAL_MEMORY_DEEP_DIVE.md](reports/JW_SOCIAL_MEMORY_DEEP_DIVE.md) | 🔬 Deep analysis of social memory |
| [JW_SOCIAL_MEMORY_TEST_RESULTS.md](reports/JW_SOCIAL_MEMORY_TEST_RESULTS.md) | 📊 Social memory test results |
| **Kuramoto Protocol** | |
| [KURAMOTO_PROTOCOL_REPORT_2026-02-24.md](reports/KURAMOTO_PROTOCOL_REPORT_2026-02-24.md) | 📈 Kuramoto synchronization analysis |
| [KURAMOTO_VALIDATION_PROTOCOL.md](reports/KURAMOTO_VALIDATION_PROTOCOL.md) | ✅ Validation methodology |
| **Test Reports** | |
| [ABLITERATED_TEST_REPORT.md](reports/ABLITERATED_TEST_REPORT.md) | 🧪 Abliterated model testing |
| [MISSING_FOR_ALL_IN_ONE.md](reports/MISSING_FOR_ALL_IN_ONE.md) | 📋 Gap analysis for all-in-one deployment |
| [ANTIFRAGILE_IMPLEMENTATION_SUMMARY.md](reports/ANTIFRAGILE_IMPLEMENTATION_SUMMARY.md) | 🏗️ Antifragile features summary |

---

## 🗺️ Documentation Map

```
documentation/
├── README.md                    (you are here)
├── guides/                      🚀 Start here!
│   ├── EASY_START.md
│   ├── DEPLOYMENT.md
│   └── ...
├── architecture/                🏗️ System design
│   ├── ANTIFRAGILE_ARCHITECTURE.md
│   └── ...
├── references/                  📚 Technical details
│   ├── TOOLS_SUITE.md
│   └── ...
├── integrations/                🔗 External connections
│   ├── HERMES_INTEGRATION.md
│   └── ...
└── reports/                     📊 Research & analysis
    ├── IMPLEMENTATION_STATUS.md
    └── ...
```

---

## 🎯 Recommended Reading Order

### For New Users
1. Start with [EASY_START.md](guides/EASY_START.md)
2. Try the [Personal Agent](guides/PERSONAL_AGENT_README.md)
3. Review [COMMANDS_GUIDE.md](guides/COMMANDS_GUIDE.md)

### For Developers
1. Read [ANTIFRAGILE_ARCHITECTURE.md](architecture/ANTIFRAGILE_ARCHITECTURE.md)
2. Explore [TOOLS_SUITE.md](references/TOOLS_SUITE.md)
3. Check [RUST_TOOLS.md](references/RUST_TOOLS.md)

### For Production Deployment
1. Follow [DEPLOYMENT.md](guides/DEPLOYMENT.md)
2. Review [PRODUCTION_FIXES.md](reports/PRODUCTION_FIXES.md)
3. Check [IMPLEMENTATION_STATUS.md](reports/IMPLEMENTATION_STATUS.md)

---

*For the main project README, see [../README.md](../README.md)*
