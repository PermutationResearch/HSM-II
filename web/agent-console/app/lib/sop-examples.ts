/**
 * Reference SOPs — map to Company OS (tasks, governance, policies, queue).
 * Use **Implement in workspace** (SopReferenceExamples) to create playbook + phase tasks and seed governance rows; or download JSON for offline use.
 */
import { departmentalSopReferenceExamples } from "./sop-examples-departmental";
import type { SopExampleDocument } from "./sop-examples-types";

export type { SopExampleDocument, SopGovernanceEvent, SopPhase } from "./sop-examples-types";

export const sopReferenceExamples: SopExampleDocument[] = [
  {
    kind: "hsm.sop_reference.v1",
    id: "customer_complaint",
    jsonFilename: "customer-complaint-sop-example.json",
    tab_label: "Complaint",
    department: "customer_success",
    title: "Customer complaint — AI SOP + human escalation",
    summary:
      "Intake and triage run as algorithmic steps (tasks + governance log); resolution closes the loop; escalation raises decision_mode and queue views.",
    phases: [
      {
        id: "intake",
        name: "Intake",
        actor: "ai",
        sop_logic:
          "Parse channel (email/chat/ticket). Classify: product area, severity (1=low, 2=medium, 3=high/legal/safety). Detect PII; redact in stored spec.",
        actions: [
          "Create task: title Complaint · {source_id}, state open, owner_persona complaint_router_ai",
          "Append specification: raw summary + classification JSON",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/tasks",
          "POST /api/company/companies/{company_id}/governance/events — action complaint_intake",
        ],
      },
      {
        id: "triage",
        name: "Triage & policy gate",
        actor: "both",
        sop_logic:
          "Evaluate policy_rules for action_type (e.g. refund_message, account_change). decision_mode auto → draft reply; admin_required → waiting_admin; blocked → stop automation.",
        actions: [
          "POST /policies/evaluate with risk_level + amount if applicable",
          "PATCH task decision or POST /tasks/{id}/decision to set decision_mode",
          "If auto: attach SOP snippet to task spec for the drafting agent",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/policies/evaluate",
          "POST /api/company/tasks/{task_id}/decision",
        ],
        resolution: "If auto and low risk: proceed to respond.",
        escalation: "If admin_required or blocked: task state waiting_admin or blocked; appears in Inbox queue tabs.",
      },
      {
        id: "respond",
        name: "AI draft + human send (SOP)",
        actor: "both",
        sop_logic:
          "AI generates reply from task spec + MEMORY/business pack; human approves send OR edits. Anti-sycophancy optional on customer-facing text.",
        actions: [
          "checkout_task for drafting agent",
          "Human review → release_task; log governance response_approved or response_edited",
          "POST governance customer_outbound_logged with channel + template id",
        ],
        company_os: [
          "POST /api/company/tasks/{task_id}/checkout",
          "POST /api/company/tasks/{task_id}/release",
          "POST /api/company/companies/{company_id}/governance/events",
        ],
        resolution: "After send: task state in_progress → done (or closed) per your state machine.",
        escalation: "If customer replies with escalation cues → bump severity; re-run triage.",
      },
      {
        id: "close",
        name: "Resolution",
        actor: "human",
        sop_logic:
          "Confirm customer satisfied or ticket timed out. Record outcome code (resolved, partial, churn_risk).",
        actions: [
          "Update task state to done/closed",
          "Governance: complaint_resolved with payload { outcome, minutes_to_resolve }",
        ],
        company_os: [
          "Task update via console or future PATCH task state API",
          "POST .../governance/events",
        ],
        resolution: "Terminal success path.",
      },
      {
        id: "escalate",
        name: "Escalation path",
        actor: "human",
        sop_logic:
          "Parallel to respond: severity 3, legal threat, fraud, or policy blocked → senior owner, SLA shorten, optional spawn subtask for legal_review.",
        actions: [
          "Set priority + SLA fields (PATCH .../tasks/{id}/sla)",
          "spawn-subagents or handoff to legal_ops persona per spawn-rules",
          "Queue view pending_approvals / waiting_admin for board",
        ],
        company_os: [
          "PATCH /api/company/tasks/{task_id}/sla",
          "POST .../tasks/{task_id}/spawn-subagents",
          "GET .../tasks/queue?view=waiting_admin",
        ],
        escalation: "Human must clear decision_mode or reassign owner_persona.",
      },
    ],
    interaction_log: {
      description:
        "Every material step should emit governance_events (and optionally run-telemetry on the active task) so leadership sees the algorithm, not only the final email.",
      suggested_events: [
        {
          action: "complaint_intake",
          subject_type: "task",
          subject_hint: "new task id",
          payload_summary: "{ source, severity, product_area }",
        },
        {
          action: "policy_evaluated",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ action_type, decision_mode, risk_level }",
        },
        {
          action: "customer_outbound_logged",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ channel, template_id, redacted: true }",
        },
        {
          action: "complaint_escalated",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ reason, new_owner? }",
        },
        {
          action: "complaint_resolved",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ outcome }",
        },
      ],
    },
  },
  {
    kind: "hsm.sop_reference.v1",
    id: "vendor_po_approval",
    jsonFilename: "vendor-po-approval-sop-example.json",
    tab_label: "Vendor / PO",
    department: "procurement",
    title: "Vendor / PO approval — procurement + finance bridge",
    summary:
      "Requisition flows through classification, policy bands (catalog vs non-catalog, amount), and finance/budget checks; dual approval over threshold; PO and vendor state logged for audit.",
    phases: [
      {
        id: "intake",
        name: "Intake",
        actor: "both",
        sop_logic:
          "Capture requester, cost_center, category (MRO, services, SaaS, …), estimated_amount, vendor_id or new_vendor flag, contract_reference if any. Attach quotes or SOW excerpt to task spec.",
        actions: [
          "Create task: Vendor/PO · {req_id}, owner_persona procurement_intake_ai or human queue",
          "Specification JSON: { category, amount_band, urgency, existing_vendor }",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/tasks",
          "POST .../governance/events — vendor_request_intake",
        ],
      },
      {
        id: "classify_route",
        name: "Classify & route",
        actor: "ai",
        sop_logic:
          "Determine catalog match vs non-catalog; map to action_type (e.g. po_small, po_medium, vendor_onboard_new). Compute effective risk_level from amount and category.",
        actions: [
          "Update spec with routing decision and required approver roles",
          "If new vendor: spawn linked subtask onboarding_checklist",
        ],
        company_os: [
          "GET /api/company/companies/{company_id}/agents (for approver role hints)",
          "POST .../governance/events — procurement_routed",
        ],
      },
      {
        id: "finance_bridge",
        name: "Finance bridge",
        actor: "both",
        sop_logic:
          "Confirm budget envelope or cost center authority; if overrun or unmapped, decision_mode admin_required. Log finance signoff or rejection reason.",
        actions: [
          "Optional integration: read budget snapshot; else human attestation in governance payload",
          "POST policy evaluate for spend_commit if amount present",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/policies/evaluate",
          "POST .../governance/events — finance_budget_checked",
        ],
        resolution: "Budget OK: proceed to approval gate.",
        escalation: "Budget blocked: task blocked or waiting_admin with finance owner.",
      },
      {
        id: "approval_gate",
        name: "Approval gate",
        actor: "human",
        sop_logic:
          "policy_rules set decision_mode: auto under floor; admin_required if amount > threshold or non-catalog or new vendor; dual_approver when SOC2 / policy requires separating buyer and payer.",
        actions: [
          "Approvals console / queue: POST .../tasks/{id}/decision with decision_mode cleared after signoff",
          "Log each approver as governance vendor_approval_recorded",
        ],
        company_os: [
          "GET .../tasks/queue?view=pending_approvals",
          "POST /api/company/tasks/{task_id}/decision",
          "POST .../governance/events",
        ],
        resolution: "All required approvers cleared → issue PO or mark ready_for_erp.",
        escalation: "SLA breach → escalate_after on task; optional spawn executive_review subtask.",
      },
      {
        id: "execute_close",
        name: "Execute & close",
        actor: "both",
        sop_logic:
          "Record PO number, ERP id, vendor status active, effective dates. Telemetry for LLM spend if contract includes AI usage line items.",
        actions: [
          "Update task state done; payload includes po_number, vendor_id, amount_final",
          "POST .../governance/events — po_issued or vendor_activated",
        ],
        company_os: [
          "POST .../governance/events",
          "POST .../spend/summary consumption indirect via spend_events elsewhere",
        ],
        resolution: "Terminal: master data and audit trail consistent.",
      },
    ],
    interaction_log: {
      description:
        "Procurement–finance transparency: each gate emits events so leadership sees amount, policy path, and approvers—not only the PO record.",
      suggested_events: [
        {
          action: "vendor_request_intake",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ category, estimated_amount, cost_center }",
        },
        {
          action: "procurement_routed",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ action_type, catalog_match, risk_level }",
        },
        {
          action: "finance_budget_checked",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ ok, budget_owner, notes }",
        },
        {
          action: "policy_evaluated",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ action_type, decision_mode, amount? }",
        },
        {
          action: "vendor_approval_recorded",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ approver_role, actor, dual_index? }",
        },
        {
          action: "po_issued",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ po_number, amount_final, vendor_id }",
        },
      ],
    },
  },
  {
    kind: "hsm.sop_reference.v1",
    id: "production_deploy",
    jsonFilename: "production-deploy-sop-example.json",
    tab_label: "Deploy",
    department: "engineering",
    title: "Production deploy / change — engineering",
    summary:
      "Change is a task with explicit tool surface (CI, deploy, flags), policy on deploy_production, canary semantics, and rollback plan pointer in spec. Every transition is logged.",
    phases: [
      {
        id: "intake",
        name: "Change intake",
        actor: "both",
        sop_logic:
          "Link artifact: commit SHA, release tag, CHANGELOG slice, blast_radius (service list). Classify change type: standard | hotfix | config_only | data_migration.",
        actions: [
          "Create task Deploy · {service}/{version}, owner_persona release_owner",
          "Spec must include rollback_plan_ref (runbook path or sibling task id)",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/tasks",
          "POST .../governance/events — change_intake",
        ],
      },
      {
        id: "preflight",
        name: "Preflight & tools",
        actor: "ai",
        sop_logic:
          "Tools are explicit: CI pipeline id, deploy runner, feature-flag API, observability query pack. AI may run read-only checks; mutation tools only after policy gate allows.",
        actions: [
          "checkout_task for release_bot with tool allowlist: ci_status, diff_summary, flag_read",
          "Attach to spec: tool_contract_version, allowed_tools[]",
        ],
        company_os: [
          "POST /api/company/tasks/{task_id}/checkout",
          "POST .../governance/events — deploy_preflight_ok or preflight_failed",
        ],
        escalation: "Failing checks → blocked; no deploy tool calls until cleared.",
      },
      {
        id: "policy_gate",
        name: "Policy gate",
        actor: "both",
        sop_logic:
          "POST policies/evaluate for action_type deploy_production with risk_level from change type + incident history flag. admin_required during freeze windows or SEV-1 open.",
        actions: [
          "If auto: document allow in governance; if admin_required: queue to waiting_admin",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/policies/evaluate",
          "POST /api/company/tasks/{task_id}/decision",
        ],
        resolution: "Approved: hand out deploy tool scope or human runs external pipeline.",
        escalation: "Blocked: rollback_plan_ref validated but no forward deploy.",
      },
      {
        id: "execute",
        name: "Execute deploy",
        actor: "both",
        sop_logic:
          "Progressive: canary % or single shard first; tool execution sandbox logs arguments class only (no secrets). Human or agent triggers each phase per SOP.",
        actions: [
          "POST governance deploy_phase_started { phase, target }",
          "On failure: execute rollback_plan_ref; POST deploy_rollback_invoked",
        ],
        company_os: [
          "POST .../governance/events",
          "POST .../tasks/{id}/release when automations done",
        ],
        escalation: "Error budget burn → automatic halt + incident task spawn.",
      },
      {
        id: "verify_close",
        name: "Verify & close",
        actor: "both",
        sop_logic:
          "Success metrics in spec: error rate, latency SLI, business KPI window. If green, close task; if soft fail, extend observation window subtask.",
        actions: [
          "Log deploy_verified with metric snapshot ids",
          "Task done | closed",
        ],
        company_os: [
          "POST .../governance/events — deploy_verified / deploy_incident_linked",
        ],
        resolution: "Healthy production at new version.",
      },
    ],
    interaction_log: {
      description:
        "Leadership sees the change algorithm: intake → preflight → policy → phases → verify; rollback is a first-class event, not an afterthought.",
      suggested_events: [
        {
          action: "change_intake",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ service, version, change_type, rollback_plan_ref }",
        },
        {
          action: "deploy_preflight_ok",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ ci_run_id, checks }",
        },
        {
          action: "policy_evaluated",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ action_type: deploy_production, decision_mode }",
        },
        {
          action: "deploy_phase_started",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ phase, canary_pct? }",
        },
        {
          action: "deploy_rollback_invoked",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ reason, rollback_plan_ref }",
        },
        {
          action: "deploy_verified",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ slis, observation_window_minutes }",
        },
      ],
    },
  },
  {
    kind: "hsm.sop_reference.v1",
    id: "public_comms_launch",
    jsonFilename: "public-comms-launch-sop-example.json",
    tab_label: "Launch / comms",
    department: "marketing",
    title: "Public comms / marketing launch",
    summary:
      "Parallel to customer complaint: brand + legal checklist, policy on public outbound, AI draft with anti-sycophancy, human-only send, full interaction log across channels.",
    phases: [
      {
        id: "intake",
        name: "Intake",
        actor: "both",
        sop_logic:
          "Capture launch type (campaign, product announcement, blog, paid social), audiences, regions, mandatory disclaimers, and link to brand kit version.",
        actions: [
          "Create task Launch · {campaign_id}, owner_persona comms_pm_ai",
          "Specification: channel list, go_live_ts, assets hash, legal_ticket ref if any",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/tasks",
          "POST .../governance/events — launch_intake",
        ],
      },
      {
        id: "checklist",
        name: "Brand & legal checklist",
        actor: "both",
        sop_logic:
          "AI verifies spec against checklist; unresolved items set decision_mode admin_required (legal, brand, accessibility as needed).",
        actions: [
          "Log checklist_passed or checklist_blocked per section",
        ],
        company_os: [
          "POST .../governance/events — launch_checklist_status",
        ],
        resolution: "All required sections green → draft stage.",
        escalation: "Legal hold → blocked until counsel decision logged.",
      },
      {
        id: "draft_review",
        name: "AI draft + human review",
        actor: "both",
        sop_logic:
          "Draft per channel from templates; run anti-sycophancy / factual-critique loop on public-facing copy. No send without human release_task.",
        actions: [
          "checkout_task for copy_ai; human edits in properties or linked doc",
          "POST governance copy_approved_for_send",
        ],
        company_os: [
          "POST /api/company/tasks/{task_id}/checkout",
          "POST /api/console/council-socratic or /api/console/anti-sycophancy (optional)",
          "POST /api/company/tasks/{task_id}/release",
        ],
      },
      {
        id: "policy_gate",
        name: "Policy gate (outbound)",
        actor: "both",
        sop_logic:
          "Evaluate public_comms_send or marketing_launch with risk_level from spend + region + claims strength. High impact → pending_approvals.",
        actions: [
          "POST .../policies/evaluate",
          "POST .../tasks/{id}/decision",
        ],
        company_os: [
          "POST /api/company/companies/{company_id}/policies/evaluate",
          "GET .../tasks/queue?view=pending_approvals",
        ],
        escalation: "Crisis signal in spec → switch to crisis_comms SOP branch; shorten SLA.",
      },
      {
        id: "send_log",
        name: "Human send & log",
        actor: "human",
        sop_logic:
          "Human publishes through authorized tools only; agent never holds live publish keys unless explicitly scoped. Each channel emission logged with redacted payload pointer.",
        actions: [
          "POST governance public_outbound_logged { channel, template_id, version }",
          "Optional spend_events for ad platform commits",
        ],
        company_os: [
          "POST .../governance/events",
          "POST .../companies/{company_id}/spend/summary (indirect)",
        ],
        resolution: "Campaign live; monitoring window defined in spec.",
      },
      {
        id: "close",
        name: "Retrospective",
        actor: "human",
        sop_logic:
          "Capture metrics vs hypothesis; archive creative variants; close task with outcome code.",
        actions: [
          "POST .../governance/events — launch_closed { outcome, learnings }",
        ],
        company_os: ["POST .../governance/events"],
        resolution: "Terminal.",
      },
    ],
    interaction_log: {
      description:
        "Commercial speech is high-risk automate: the log proves who approved what, which policy version applied, and which human sent to the public internet.",
      suggested_events: [
        {
          action: "launch_intake",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ campaign_id, channels[], go_live_ts }",
        },
        {
          action: "launch_checklist_status",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ brand, legal, a11y, all_green }",
        },
        {
          action: "copy_approved_for_send",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ reviewer, variant_ids[] }",
        },
        {
          action: "policy_evaluated",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ action_type: public_comms_send, decision_mode }",
        },
        {
          action: "public_outbound_logged",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ channel, template_id, redacted: true }",
        },
        {
          action: "launch_closed",
          subject_type: "task",
          subject_hint: "task id",
          payload_summary: "{ outcome, metrics_ref }",
        },
      ],
    },
  },
  ...departmentalSopReferenceExamples,
];

/** First catalog entry; same as `sopReferenceExamples[0]`. */
export const customerComplaintSopExample = sopReferenceExamples[0];
