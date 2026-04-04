import type { Spec } from "@json-render/core";

/** Static example spec — replace with model output at runtime. */
export const HSM_DEMO_GEN_UI_SPEC: Spec = {
  root: "root",
  elements: {
    root: {
      type: "DashboardRoot",
      props: {
        title: "Company-as-intelligence (demo)",
        subtitle: "Generated UI spec — not raw API JSON",
      },
      children: ["a1", "m1", "m2", "m3", "t1", "list"],
    },
    a1: {
      type: "AlertBanner",
      props: {
        severity: "info",
        message:
          "Below is a json-render Spec. Production flows should compile LLM JSONL patches into this shape.",
      },
      children: [],
    },
    m1: {
      type: "MetricRow",
      props: { label: "Beliefs (example)", value: "12", hint: "from world snapshot" },
      children: [],
    },
    m2: {
      type: "MetricRow",
      props: { label: "Hypergraph edges", value: "48" },
      children: [],
    },
    m3: {
      type: "MetricRow",
      props: { label: "Coherence", value: "0.94", hint: "illustrative" },
      children: [],
    },
    t1: {
      type: "TextBlock",
      props: {
        variant: "muted",
        body: "Capabilities are tools/skills; the Intelligence Layer composes them; Interfaces deliver here and in Telegram/HTTP.",
      },
      children: [],
    },
    list: {
      type: "BulletList",
      props: { title: "Next steps" },
      children: ["li1", "li2", "li3"],
    },
    li1: {
      type: "ListItem",
      props: { text: "Stream JSONL patches from your model into applySpecPatch / useUIStream." },
      children: [],
    },
    li2: {
      type: "ListItem",
      props: { text: "Validate with hsmDashboardCatalog.validate(spec) before render." },
      children: [],
    },
    li3: {
      type: "ListItem",
      props: { text: "Keep /memory and /council on PrettyJson for ops debug." },
      children: [],
    },
  },
};
