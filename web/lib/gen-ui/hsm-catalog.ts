/**
 * HSM-II generative UI catalog — components an LLM may emit as a json-render {@link Spec}.
 * Do not feed raw REST blobs here; use {@link ../components/PrettyJson} for debug.
 */

import { defineCatalog } from "@json-render/core";
import { schema } from "@json-render/react/schema";
import { z } from "zod";

export const hsmDashboardCatalog = defineCatalog(schema, {
  components: {
    DashboardRoot: {
      props: z.object({
        title: z.string().optional(),
        subtitle: z.string().optional(),
      }),
      description:
        "Root panel for an HSM-II dashboard section. Put MetricRow, TextBlock, AlertBanner children inside.",
      slots: ["default"],
    },
    MetricRow: {
      props: z.object({
        label: z.string(),
        value: z.string(),
        hint: z.string().optional(),
      }),
      description: "Single KPI / stat row (e.g. coherence, belief count, API latency).",
    },
    TextBlock: {
      props: z.object({
        body: z.string(),
        variant: z.enum(["default", "muted"]).optional(),
      }),
      description: "Explanatory paragraph for operators.",
    },
    AlertBanner: {
      props: z.object({
        severity: z.enum(["info", "warn", "error"]),
        message: z.string(),
      }),
      description: "Highlighted system or governance message.",
    },
    BulletList: {
      props: z.object({
        title: z.string().optional(),
      }),
      description: "List container; each child should be a ListItem.",
      slots: ["default"],
    },
    ListItem: {
      props: z.object({
        text: z.string(),
      }),
      description: "One bullet line.",
    },
  },
  actions: {
    noop: {
      params: z.object({}).optional(),
      description: "No-op placeholder so the catalog matches json-render built-in action wiring.",
    },
  },
});

/** System prompt fragment describing allowed components (for your LLM system message). */
export function hsmGenUiCatalogPrompt(): string {
  return hsmDashboardCatalog.prompt({
    customRules: [
      "This UI describes HSM-II operational summaries (world model, council, paperclip signals).",
      "Prefer DashboardRoot as the root type. Use MetricRow for numbers, TextBlock for narrative.",
      "Never put raw API response objects into props; only use catalog field shapes.",
    ],
  });
}

/** Strict JSON Schema subset for structured-output APIs (optional). */
export function hsmGenUiSpecJsonSchema(): object {
  return hsmDashboardCatalog.jsonSchema({ strict: true });
}
