# Recursive Investigation Agent System

A comprehensive investigation framework integrated into HSM-II for analyzing heterogeneous datasets, resolving entities, and discovering non-obvious connections.

## Overview

The investigation system provides:
- **19 specialized tools** organized around the investigation workflow
- **Recursive sub-agent delegation** for parallel processing
- **Provider-agnostic LLM abstraction** (Ollama, OpenAI, etc.)
- **Session persistence** with full lifecycle management
- **Evidence-backed analysis** with audit trails

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                   Investigation CLI                          │
│  (investigate binary: new, list, resume, repl, query)       │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              InvestigationEngine                            │
│  - Session management                                       │
│  - Tool orchestration                                       │
│  - Recursive delegation                                     │
│  - Evidence chain construction                              │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  Tool Registry  │ │  AgentLoop      │ │  Session Store  │
│  (19 tools)     │ │  (LLM + Tools)  │ │  (Persistence)  │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

## The 19 Investigation Tools

### Dataset & Workspace Tools (7)
1. **list_files** - List files in workspace or subdirectory
2. **search_files** - Search for files by pattern or content
3. **repo_map** - Generate structured workspace/dataset map
4. **read_file** - Read file contents with offset/limit
5. **write_file** - Write or overwrite files
6. **edit_file** - Edit files by search/replace
7. **apply_patch** - Apply unified diff patches

### Shell Execution Tools (4)
8. **run_shell** - Execute shell commands synchronously
9. **run_shell_bg** - Execute shell commands in background
10. **check_shell_bg** - Check background job status
11. **kill_shell_bg** - Terminate background jobs

### Web & Search Tools (2)
12. **web_search** - Search web (Exa or similar providers)
13. **fetch_url** - Fetch and extract URL content

### Planning & Delegation Tools (4)
14. **think** - Record reasoning and plan next steps
15. **subtask** - Delegate to recursive sub-agents
16. **list_artifacts** - List subtask artifacts
17. **read_artifact** - Read artifact contents

### Dataset Analysis Tools (2)
18. **load_dataset** - Load CSV/JSON/Parquet datasets
19. **inspect_dataset** - Analyze schema and statistics

## CLI Usage

```bash
# Create a new investigation
investigate --new "Campaign Finance Analysis" --workspace ./cases/case_001

# List all investigations
investigate --list

# Resume an investigation
investigate --resume <session_id>

# Interactive REPL
investigate --repl

# Run single query
investigate --query "Analyze connections between datasets" --title "Quick Analysis"

# Export report
investigate --export <session_id> --format markdown --output report.md
```

## REPL Commands

```
> investigate <query>     - Start an investigation
> load <path>             - Load a dataset
> delegate <description>  - Delegate a subtask
> status                  - Show investigation status
> save                    - Save session
> help                    - Show help
> exit/quit               - Exit REPL
```

## Session Persistence

Sessions are automatically saved to the workspace directory as JSON files:
- Full conversation history
- Dataset references
- Resolved entities
- Findings with evidence chains
- Tool call audit trail
- Subtask hierarchy

## Recursive Delegation

The system supports recursive investigation through the `subtask` tool:

1. Parent agent decomposes investigation into focused subtasks
2. Each subtask has acceptance criteria
3. Sub-agents run in parallel (when configured)
4. Results are aggregated into parent session
5. Evidence chains are maintained across delegation levels

## Entity Resolution

Built-in entity resolution across heterogeneous datasets:
- **Persons**: Name matching, alias resolution
- **Organizations**: Corporate registries, subsidiaries
- **Locations**: Address normalization, geocoding
- **Contracts**: Government contracts, grant awards
- **Campaigns**: Campaign finance records
- **Lobbying Filings**: Disclosure records

Entity attributes are merged from multiple sources with confidence scoring.

## Evidence Chain Construction

All findings include evidence chains:
```
Finding
├── Evidence Step 1: Dataset record
├── Evidence Step 2: Entity resolution
├── Evidence Step 3: Correlation analysis
└── Evidence Step 4: Inference rule application
```

## Integration with HSM-II

The investigation system integrates with existing HSM-II components:

- **Council**: Investigations can spawn Socratic councils for complex decisions
- **DKS**: Entity persistence uses Dynamic Kinetic Stability principles
- **CASS**: Semantic matching for entity resolution
- **Skills**: Investigation patterns stored as reusable skills
- **Memory**: Hybrid memory for context retention

## Configuration

Environment variables:
- `LLM_INVESTIGATION_MODEL` - Default model for investigations
- `INVESTIGATION_WORKSPACE` - Default workspace directory
- `MAX_SUBTASK_DEPTH` - Maximum recursion depth (default: 3)
- `MAX_CONCURRENT_SUBTASKS` - Parallel subtask limit (default: 5)

## Example Investigation

```bash
# Start investigation
investigate --new "Corporate Lobbying Analysis" --workspace ./lobbying

# In REPL:
> load ./data/lobbying_disclosures_2024.csv
> load ./data/campaign_finance_2024.csv
> load ./data/government_contracts_2024.csv

> investigate "Find corporations that both lobbied and received contracts"

# System will:
# 1. Load and parse all three datasets
# 2. Resolve entities across datasets
# 3. Find correlations between lobbying and contracts
# 4. Generate findings with evidence chains
# 5. Create exportable report
```

## Files Added

```
src/
├── investigation_tools.rs      # 19 investigation tools
├── investigation_engine.rs     # Core engine + session management
└── bin/
    └── investigate.rs          # CLI entry point
```

## Build and Run

```bash
# Build the investigate binary
cargo build --release --bin investigate

# Run
./target/release/investigate --help
```
