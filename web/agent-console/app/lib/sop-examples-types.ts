export type SopPhase = {
  id: string;
  name: string;
  actor: "ai" | "human" | "both";
  sop_logic: string;
  actions: string[];
  company_os: string[];
  resolution?: string;
  escalation?: string;
};

export type SopGovernanceEvent = {
  action: string;
  subject_type: string;
  subject_hint: string;
  payload_summary: string;
};

export type SopExampleDocument = {
  kind: "hsm.sop_reference.v1";
  id: string;
  jsonFilename: string;
  tab_label: string;
  department?: string;
  title: string;
  summary: string;
  phases: SopPhase[];
  interaction_log: {
    description: string;
    suggested_events: SopGovernanceEvent[];
  };
};
