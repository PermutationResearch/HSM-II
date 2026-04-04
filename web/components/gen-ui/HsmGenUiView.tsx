"use client";

import { JSONUIProvider, Renderer, defineRegistry } from "@json-render/react";
import type { Spec } from "@json-render/core";
import { hsmDashboardCatalog } from "@/lib/gen-ui/hsm-catalog";

const { registry } = defineRegistry(hsmDashboardCatalog, {
  actions: {
    noop: async () => {},
  },
  components: {
    DashboardRoot: ({ props, children }) => (
      <div className="rounded-xl border border-zinc-700 bg-zinc-900/90 p-5 space-y-4 shadow-lg shadow-black/20">
        {(props.title || props.subtitle) && (
          <div className="space-y-1 border-b border-zinc-800 pb-3">
            {props.title && <h2 className="text-lg font-semibold text-zinc-50">{props.title}</h2>}
            {props.subtitle && <p className="text-xs text-zinc-500">{props.subtitle}</p>}
          </div>
        )}
        <div className="space-y-3">{children}</div>
      </div>
    ),
    MetricRow: ({ props }) => (
      <div className="flex items-baseline justify-between gap-4 rounded-lg border border-zinc-800 bg-zinc-950/60 px-4 py-2">
        <span className="text-xs uppercase tracking-wide text-zinc-500">{props.label}</span>
        <div className="text-right">
          <span className="text-lg font-semibold text-emerald-400 tabular-nums">{props.value}</span>
          {props.hint && <p className="text-[10px] text-zinc-600 mt-0.5">{props.hint}</p>}
        </div>
      </div>
    ),
    TextBlock: ({ props }) => (
      <p
        className={
          props.variant === "muted"
            ? "text-sm leading-relaxed text-zinc-500"
            : "text-sm leading-relaxed text-zinc-300"
        }
      >
        {props.body}
      </p>
    ),
    AlertBanner: ({ props }) => {
      const tone =
        props.severity === "error"
          ? "border-red-900/60 bg-red-950/40 text-red-200"
          : props.severity === "warn"
            ? "border-amber-900/50 bg-amber-950/30 text-amber-100"
            : "border-sky-900/50 bg-sky-950/30 text-sky-100";
      return (
        <div className={`rounded-lg border px-4 py-3 text-sm ${tone}`} role="status">
          {props.message}
        </div>
      );
    },
    BulletList: ({ props, children }) => (
      <div className="space-y-2">
        {props.title && <p className="text-xs font-medium uppercase tracking-wide text-zinc-500">{props.title}</p>}
        <ul className="list-disc list-inside space-y-1 text-sm text-zinc-400">{children}</ul>
      </div>
    ),
    ListItem: ({ props }) => <li className="text-zinc-300">{props.text}</li>,
  },
});

export function HsmGenUiView({ spec, loading }: { spec: Spec | null; loading?: boolean }) {
  return (
    <JSONUIProvider registry={registry} initialState={{}}>
      <Renderer spec={spec} registry={registry} loading={loading} />
    </JSONUIProvider>
  );
}
