/**
 * Shared stream-shaping policy for operator chat.
 * Kept as plain JS so Node regression tests can import it without TS tooling.
 */

const VISIBLE_RUNTIME_TOOL_ALLOWLIST = new Set([
  "bash",
  "read",
  "read_file",
  "grep",
  "glob",
  "edit",
  "write",
  "delete",
  "ls",
  "find",
  "search",
  "shell",
  "apply_patch",
  "notebookread",
  "notebookedit",
  "webfetch",
  "websearch",
]);

function normalizeToolName(name) {
  return typeof name === "string" ? name.trim().toLowerCase() : "";
}

export function isInternalToolName(name) {
  const t = normalizeToolName(name);
  if (!t) return false;
  return t === "worker_dispatch" || t === "claude_harness";
}

export function shouldExposeRuntimePayload(payload) {
  if (!payload || typeof payload !== "object") return false;
  const eventType = typeof payload.event_type === "string" ? payload.event_type.trim().toLowerCase() : "";
  if (!eventType) return false;
  if (eventType === "stream_event") return false;
  if (eventType === "tool_start_delta") return false;
  if (eventType !== "tool_start" && eventType !== "tool_complete" && eventType !== "tool_error") return false;
  const tool = normalizeToolName(payload.tool_name);
  if (!tool) return false;
  if (isInternalToolName(tool)) return false;
  if (!VISIBLE_RUNTIME_TOOL_ALLOWLIST.has(tool)) return false;
  return true;
}

export function isCompanionPlanningNarration(text) {
  const t = typeof text === "string" ? text.trim().toLowerCase() : "";
  if (!t) return false;
  if (t.includes("i’ll begin by inspecting the repository layout")) return true;
  if (t.includes("i'll begin by inspecting the repository layout")) return true;
  if (t.includes("let me kick things off with a quick ls")) return true;
  if (t.includes("i’ll stream the tool events")) return true;
  if (t.includes("i'll stream the tool events")) return true;
  return false;
}

function cleanedWorkerSummary(summary) {
  if (typeof summary !== "string") return "";
  let s = summary.replace(/\r\n/g, "\n").trim();
  if (!s) return "";
  const lower = s.toLowerCase();
  const replyIdx = lower.lastIndexOf("worker reply:");
  const successIdx = lower.lastIndexOf("worker success:");
  const startIdx = Math.max(replyIdx, successIdx);
  if (startIdx >= 0) {
    const marker = lower.startsWith("worker reply:", startIdx) ? "worker reply:" : "worker success:";
    s = s.slice(startIdx + marker.length).trim();
  }
  s = s.replace(/\n{2,}---\n\*agentic tool loop:[\s\S]*$/i, "").trim();
  s = s
    .split("\n")
    .filter((line) => !/^\s*worker (start|error|success|reply):/i.test(line))
    .join("\n")
    .trim();
  if (/^(we need to|i need to|need to|probably|unclear|hard to guess|maybe\b|might be\b)/i.test(s)) {
    const anchors = [
      /\bHere(?:'s| is)\b/i,
      /\bIt looks like\b/i,
      /\bIn\s+`?Cargo\.toml`?\b/i,
      /\bThe\s+(?:package|project|repository)\b/i,
    ];
    let best = -1;
    for (const re of anchors) {
      const m = re.exec(s);
      if (!m) continue;
      const idx = m.index ?? -1;
      if (idx <= 0) continue;
      if (best < 0 || idx < best) best = idx;
    }
    if (best > 0) s = s.slice(best).trim();
    if (best < 0) return "";
  }
  return s;
}

export function buildExecutionEvidenceReply({ summary, artifacts, executionVerified }) {
  if (executionVerified !== true) {
    return "Execution is still pending verification; no completion claim until runtime evidence is confirmed.";
  }
  const touched = Array.isArray(artifacts?.touched_files) ? artifacts.touched_files : [];
  const fileCount = touched.length;
  const tools = new Set();
  for (const row of touched) {
    if (!row || typeof row !== "object") continue;
    const rowTools = Array.isArray(row.tools) ? row.tools : [];
    for (const tool of rowTools) {
      if (typeof tool === "string" && tool.trim()) tools.add(tool.trim());
    }
  }
  const toolList = [...tools].sort();
  const summaryLine = cleanedWorkerSummary(summary) || "Completed.";
  const evidence = [];
  if (fileCount > 0) evidence.push(`${fileCount} file${fileCount === 1 ? "" : "s"} touched`);
  if (toolList.length > 0) evidence.push(`tools: ${toolList.join(", ")}`);
  if (evidence.length === 0) return summaryLine;
  return `${summaryLine}\n\nEvidence: ${evidence.join(" · ")}.`;
}

export const __testables = {
  VISIBLE_RUNTIME_TOOL_ALLOWLIST,
};
