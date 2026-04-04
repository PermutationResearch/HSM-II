/**
 * Reference workforce presets aligned with:
 * - **Paperclip** — onboarding `owner_role` values in `template_defaults` (Company OS) + common roster ids
 * - **Hermes** — personal/A2A tool bridge (`adapter_type: hermes`) per `src/personal` + `hsm_a2a_adapter`
 * - **SOP demos** — `owner_persona` / checkout names referenced in `sop-examples*.ts`
 */

export type ExampleWorkforceAgentSource = "paperclip" | "hermes" | "sop";

export type ExampleWorkforceAgentPreset = {
  name: string;
  role: string;
  title?: string;
  capabilities?: string;
  briefing?: string;
  adapter_type?: string;
  adapter_config?: Record<string, unknown>;
  source: ExampleWorkforceAgentSource;
};

/** Stable presets; agent `name` must be letters, digits, `_`, `-` only (API constraint). */
export const EXAMPLE_WORKFORCE_AGENT_PRESETS: readonly ExampleWorkforceAgentPreset[] = [
  // --- Paperclip-class onboarding & roster (template_defaults + dashboard-style ids)
  {
    name: "support_admin",
    role: "manager",
    title: "Support & inbox owner",
    source: "paperclip",
    capabilities: "Triage, SLAs, customer messaging",
  },
  {
    name: "ops_admin",
    role: "manager",
    title: "Operations administrator",
    source: "paperclip",
    capabilities: "Workflows, scheduling, exceptions",
  },
  {
    name: "marketing_admin",
    role: "manager",
    title: "Marketing ops",
    source: "paperclip",
    capabilities: "Content refresh, campaign ops",
  },
  {
    name: "finance_admin",
    role: "manager",
    title: "Finance & approvals",
    source: "paperclip",
    capabilities: "Refunds, POs, budget gates",
  },
  {
    name: "legal_owner",
    role: "manager",
    title: "Legal / compliance owner",
    source: "paperclip",
    capabilities: "Legal replies, contract risk",
  },
  {
    name: "account_manager",
    role: "manager",
    title: "Account manager",
    source: "paperclip",
    capabilities: "Client comms, lead follow-up",
  },
  {
    name: "ads_manager",
    role: "worker",
    title: "Ads & campaigns",
    source: "paperclip",
    capabilities: "Publish campaigns, performance summaries",
  },
  {
    name: "property_admin",
    role: "manager",
    title: "Property coordinator",
    source: "paperclip",
    capabilities: "Tenant requests, vendor coordination",
  },
  {
    name: "maintenance_coord",
    role: "worker",
    title: "Maintenance coordinator",
    source: "paperclip",
    capabilities: "Dispatch, work orders, follow-up",
  },
  {
    name: "manager",
    role: "manager",
    title: "General manager",
    source: "paperclip",
    capabilities: "Escalations, approvals",
  },
  {
    name: "owner",
    role: "owner",
    title: "Company owner",
    source: "paperclip",
    capabilities: "Final approval, budget policy",
  },
  {
    name: "billing_clerk",
    role: "worker",
    title: "Billing clerk",
    source: "paperclip",
    capabilities: "Invoices, payment status",
  },
  {
    name: "ops_lead",
    role: "manager",
    title: "Operations lead",
    source: "paperclip",
    capabilities: "Queue health, staffing",
  },
  {
    name: "concierge",
    role: "worker",
    title: "Concierge",
    source: "paperclip",
    capabilities: "Front-line requests, routing",
  },
  // --- Hermes bridge workers (tool/MCP execution via Hermes CLI — see `hsm_a2a_adapter`)
  {
    name: "hermes_tools_runner",
    role: "worker",
    title: "Hermes tool runner",
    source: "hermes",
    adapter_type: "hermes",
    capabilities: "External tools and MCP via Hermes bridge",
    briefing:
      "Runs delegated work through the Hermes CLI / MCP path when the console resolves this agent at checkout.",
    adapter_config: { runner: "hermes_cli" },
  },
  {
    name: "hermes_mcp_worker",
    role: "worker",
    title: "Hermes MCP worker",
    source: "hermes",
    adapter_type: "hermes",
    capabilities: "Scoped MCP tools; MEMORY/USER/AGENTS context when present",
    briefing:
      "Hermes-style grounded context: repo MEMORY.md, USER.md, AGENTS.md excerpts may augment system prompts.",
    adapter_config: { context: "hermes_memory_files" },
  },
  {
    name: "hermes_a2a_sidecar",
    role: "worker",
    title: "A2A JSON-RPC worker (Hermes)",
    source: "hermes",
    adapter_type: "hermes",
    capabilities: "JSON-RPC A2A task execution sidecar",
    briefing: "Pair with `hsm_a2a_adapter` + `hermes_bin` for remote agent turns.",
    adapter_config: { transport: "a2a_jsonrpc" },
  },
  // --- SOP reference personas (sop-examples.ts + sop-examples-departmental.ts)
  {
    name: "complaint_router_ai",
    role: "worker",
    title: "Complaint triage agent",
    source: "sop",
    capabilities: "Intake, routing, SLA for complaints",
  },
  {
    name: "procurement_intake_ai",
    role: "worker",
    title: "Procurement intake",
    source: "sop",
    capabilities: "Vendor/PO intake and classification",
  },
  {
    name: "procurement_triage_ai",
    role: "worker",
    title: "Procurement triage",
    source: "sop",
    capabilities: "PR classification, catalog match, routing",
  },
  {
    name: "release_owner",
    role: "manager",
    title: "Release owner",
    source: "sop",
    capabilities: "Deploy tasks, approvals, comms",
  },
  {
    name: "release_bot",
    role: "worker",
    title: "Release automation",
    source: "sop",
    capabilities: "CI status, diffs, flag reads (tool allowlist)",
  },
  {
    name: "comms_pm_ai",
    role: "worker",
    title: "Comms / launch PM",
    source: "sop",
    capabilities: "Launch and campaign task ownership",
  },
  {
    name: "copy_ai",
    role: "worker",
    title: "Copy drafting agent",
    source: "sop",
    capabilities: "Drafts; human finalize",
  },
  {
    name: "drafting_agent",
    role: "worker",
    title: "General drafting agent",
    source: "sop",
    capabilities: "Checkout drafting for tasks",
  },
  {
    name: "soc_analyst_bot",
    role: "worker",
    title: "SOC analyst bot",
    source: "sop",
    capabilities: "Scoped security tooling, checkout for investigations",
  },
  {
    name: "legal_ops",
    role: "worker",
    title: "Legal operations",
    source: "sop",
    capabilities: "Spawn handoffs, policy gates",
  },
  {
    name: "deal_desk",
    role: "manager",
    title: "Deal desk",
    source: "sop",
    capabilities: "Commercial exceptions, floor approvals",
  },
  {
    name: "incident_commander",
    role: "manager",
    title: "Incident commander",
    source: "sop",
    capabilities: "Incident response, handoffs, SLAs",
  },
];

const byName = new Map<string, ExampleWorkforceAgentPreset>();
for (const p of EXAMPLE_WORKFORCE_AGENT_PRESETS) {
  byName.set(p.name, p);
}

export function getExampleWorkforceAgentPreset(name: string): ExampleWorkforceAgentPreset | undefined {
  return byName.get(name.trim());
}

/** Sorted unique agent ids for datalists (ids only). */
export function exampleWorkforceAgentNames(): string[] {
  return [...byName.keys()].sort((a, b) => a.localeCompare(b));
}
