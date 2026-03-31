# Claude Prompt Catalog (Local Mapping)

This document maps the public "1..30 prompt catalog" style to files extracted from:

- `external/claude-code-from-npm/package/unpacked/src/`

It is a practical coverage map, not a claim of exact one-to-one parity with any external repo.

## Coverage Snapshot

- Exact markdown catalog (`prompts/01_...md` to `30_...md`): **not present locally**
- Prompt logic in TypeScript source modules: **present**
- Tool-level prompt modules: **present across many tools**

## Mapped Prompt Areas

### Core Identity / System Assembly

- Main system prompt assembly:
  - `src/utils/systemPrompt.ts`
  - `src/constants/systemPromptSections.ts`
  - `src/utils/systemPromptType.ts`
- Team memory prompt composition:
  - `src/memdir/teamMemPrompts.ts`

### Orchestration / Multi-Agent

- Agent tool prompt:
  - `src/tools/AgentTool/prompt.ts`
- Teammate addendum / swarm messaging:
  - `src/utils/swarm/teammatePromptAddendum.ts`
  - `src/tools/SendMessageTool/prompt.ts`

### Security / Permissions

- Permission prompt schema:
  - `src/utils/permissions/PermissionPromptToolResultSchema.ts`
- Permission UI prompt components:
  - `src/components/permissions/PermissionPrompt.tsx`
- Sandbox guidance hints:
  - `src/components/PromptInput/SandboxPromptFooterHint.tsx`

### Context Window / Compaction

- Compact service prompt:
  - `src/services/compact/prompt.ts`
- Session memory prompts:
  - `src/services/SessionMemory/prompts.ts`
- Memory extraction prompts:
  - `src/services/extractMemories/prompts.ts`

### Suggestions / Utility

- Prompt suggestion engine:
  - `src/services/PromptSuggestion/promptSuggestion.ts`
  - `src/hooks/usePromptSuggestion.ts`
- Prompt dumping/debug:
  - `src/services/api/dumpPrompts.ts`

### Tool Prompt Modules (examples)

- `src/tools/BashTool/prompt.ts`
- `src/tools/FileReadTool/prompt.ts`
- `src/tools/FileWriteTool/prompt.ts`
- `src/tools/FileEditTool/prompt.ts`
- `src/tools/GlobTool/prompt.ts`
- `src/tools/GrepTool/prompt.ts`
- `src/tools/WebSearchTool/prompt.ts`
- `src/tools/WebFetchTool/prompt.ts`
- `src/tools/LSPTool/prompt.ts`
- `src/tools/MCPTool/prompt.ts`
- `src/tools/ReadMcpResourceTool/prompt.ts`
- `src/tools/ListMcpResourcesTool/prompt.ts`
- `src/tools/NotebookEditTool/prompt.ts`
- `src/tools/TodoWriteTool/prompt.ts`
- `src/tools/ScheduleCronTool/prompt.ts`
- `src/tools/AskUserQuestionTool/prompt.ts`
- `src/tools/TaskCreateTool/prompt.ts`
- `src/tools/TaskGetTool/prompt.ts`
- `src/tools/TaskUpdateTool/prompt.ts`
- `src/tools/TaskListTool/prompt.ts`
- `src/tools/TaskStopTool/prompt.ts`

## Known Gaps vs Numbered Markdown Catalog

The local extracted source is code-centric. The following are not laid out as dedicated numbered markdown files:

- single file `01_main_system_prompt.md` style artifacts
- explicit standalone "catalog entries" for each conceptual prompt
- normalized metadata per prompt (owner, inputs, outputs, risk class)

## Suggested Next Action

If you want strict catalog parity, generate:

- `documentation/references/claude_prompt_catalog/01_...30_...md`

from extracted TS modules via a script that:

1. discovers prompt-bearing files
2. extracts exported prompt strings/builders
3. writes normalized markdown with source links
