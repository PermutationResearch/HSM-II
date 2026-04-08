"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import { ExternalLink, RefreshCw } from "lucide-react";
import { Button } from "@/app/components/ui/button";
import { useWorkspace } from "@/app/context/WorkspaceContext";

type LayerSpec = {
  id: string;
  name: string;
  responsibility: string;
  key_abstraction: string;
  lives_inside: string;
  code_modules: string[];
};

type ArchitectureBlueprint = {
  schema_version: number;
  title: string;
  summary: string;
  layers: LayerSpec[];
  entry_points: string[];
  data_flows: { id: string; name: string; description: string; steps: string[] }[];
  shared_abstractions: string[];
  dual_company_layers?: string;
};

type ArchitectureApiResponse = {
  blueprint: ArchitectureBlueprint;
  runtime: {
    beliefs: number;
    experiences: number;
    hyper_edges: number;
    tick_count: number;
    prev_coherence: number;
    skill_bank_roots: number;
  } | null;
};

export default function WorkspaceArchitecturePage() {
  const { apiBase } = useWorkspace();
  const [data, setData] = useState<ArchitectureApiResponse | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setErr(null);
    setLoading(true);
    try {
      const r = await fetch(`${apiBase}/api/architecture`);
      if (!r.ok) throw new Error(`architecture ${r.status}`);
      const j = (await r.json()) as ArchitectureApiResponse;
      setData(j);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setData(null);
    } finally {
      setLoading(false);
    }
  }, [apiBase]);

  useEffect(() => {
    void load();
  }, [load]);

  const bp = data?.blueprint;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="pc-page-eyebrow">Platform blueprint</p>
          <h1 className="pc-page-title">Architecture</h1>
          <p className="pc-page-desc">
            <strong className="font-medium text-foreground/90">HSM-II (platform)</strong> — single source of truth is the
            repo file{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">architecture/hsm-ii-blueprint.ron</code>, embedded
            in the binary and exposed as{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">GET /api/architecture</code> on both{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">hsm_console</code> (blueprint only) and the world
            API (optional <code className="rounded bg-white/5 px-1 font-mono text-[11px]">runtime</code> counts when a world
            is mounted). Human narrative + Mermaid:{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">ARCHITECTURE.md</code> /{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">ARCHITECTURE.generated.md</code>.
          </p>
          <p className="mt-2 max-w-3xl text-xs leading-relaxed text-muted-foreground">
            <strong className="font-medium text-foreground/85">Per company</strong> is a different layer: operators define how{" "}
            <em>this</em> workspace runs via pack files under <code className="font-mono text-[11px]">hsmii_home</code> (e.g.{" "}
            <code className="font-mono text-[11px]">AGENTS.md</code>, <code className="font-mono text-[11px]">visions.md</code>
            ), <span className="font-mono text-[11px]">Shared context</span> on the company, goals/tasks in Company OS — not
            the global RON. Task hyperedges:{" "}
            <Link href="/workspace/graph" className="text-primary underline-offset-4 hover:underline">
              Graph
            </Link>
            .
          </p>
        </div>
        <Button type="button" variant="outline" size="sm" disabled={loading} onClick={() => void load()}>
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
          Refresh
        </Button>
      </div>

      {err ? (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm text-destructive-foreground">
          {err}
          <p className="mt-2 text-xs text-muted-foreground">
            Confirm <code className="rounded bg-white/5 px-1 font-mono text-[10px]">NEXT_PUBLIC_API_BASE</code> points at{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[10px]">hsm_console</code> (or world API). Rebuild/restart
            the console binary if you still see 404. For Markdown locally:{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[10px]">cargo run --bin hsm_archviz</code>.
          </p>
        </div>
      ) : null}

      {data && !data.runtime ? (
        <div className="rounded-lg border border-border/80 bg-muted/20 p-3 text-xs text-muted-foreground">
          <span className="font-medium text-foreground/90">Runtime overlay</span> is omitted on the company-console API
          (no hypergraph world mounted). Point <code className="font-mono text-[10px]">NEXT_PUBLIC_API_BASE</code> at a
          running world API (e.g. <code className="font-mono text-[10px]">personal_agent</code> on{" "}
          <code className="font-mono text-[10px]">HSM_API_PORT</code>) if you need live belief / tick counts alongside the
          same blueprint.
        </div>
      ) : null}

      {data?.runtime ? (
        <div className="rounded-lg border border-admin-border bg-admin-panel/40 p-4">
          <p className="font-mono text-[10px] uppercase tracking-wide text-admin-muted">Runtime overlay</p>
          <dl className="mt-2 grid gap-2 text-sm sm:grid-cols-2 lg:grid-cols-3">
            <div>
              <dt className="text-admin-muted">Beliefs</dt>
              <dd className="font-mono">{data.runtime.beliefs}</dd>
            </div>
            <div>
              <dt className="text-admin-muted">Experiences</dt>
              <dd className="font-mono">{data.runtime.experiences}</dd>
            </div>
            <div>
              <dt className="text-admin-muted">Hyper edges</dt>
              <dd className="font-mono">{data.runtime.hyper_edges}</dd>
            </div>
            <div>
              <dt className="text-admin-muted">Tick</dt>
              <dd className="font-mono">{data.runtime.tick_count}</dd>
            </div>
            <div>
              <dt className="text-admin-muted">Coherence</dt>
              <dd className="font-mono">{data.runtime.prev_coherence.toFixed(4)}</dd>
            </div>
            <div>
              <dt className="text-admin-muted">Skill bank (general roots)</dt>
              <dd className="font-mono">{data.runtime.skill_bank_roots}</dd>
            </div>
          </dl>
        </div>
      ) : null}

      {bp ? (
        <>
          <section className="rounded-lg border border-admin-border bg-admin-panel/30 p-4">
            <h2 className="text-sm font-semibold text-foreground">{bp.title}</h2>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{bp.summary}</p>
            <p className="mt-2 font-mono text-[10px] text-admin-muted">schema_version: {bp.schema_version}</p>
          </section>

          <section className="rounded-lg border border-admin-border bg-admin-panel/20 p-4">
            <h2 className="mb-3 text-sm font-semibold">Five layers</h2>
            <div className="overflow-x-auto">
              <table className="w-full min-w-[640px] border-collapse text-left text-xs">
                <thead>
                  <tr className="border-b border-admin-border text-admin-muted">
                    <th className="py-2 pr-3 font-medium">Layer</th>
                    <th className="py-2 pr-3 font-medium">Responsibility</th>
                    <th className="py-2 pr-3 font-medium">Abstraction</th>
                    <th className="py-2 font-medium">Modules</th>
                  </tr>
                </thead>
                <tbody>
                  {bp.layers.map((l) => (
                    <tr key={l.id} className="border-b border-admin-border/60 align-top">
                      <td className="py-2 pr-3 font-medium text-foreground">{l.name}</td>
                      <td className="py-2 pr-3 text-muted-foreground">{l.responsibility}</td>
                      <td className="py-2 pr-3 font-mono text-[10px] text-primary">{l.key_abstraction}</td>
                      <td className="py-2 font-mono text-[10px] text-muted-foreground">{l.code_modules.join(", ")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <section className="rounded-lg border border-admin-border bg-admin-panel/20 p-4">
            <h2 className="mb-3 text-sm font-semibold">Data flows</h2>
            <ul className="space-y-4 text-sm">
              {bp.data_flows.map((f) => (
                <li key={f.id} className="border-l-2 border-primary/40 pl-3">
                  <p className="font-medium text-foreground">{f.name}</p>
                  <p className="mt-1 text-muted-foreground">{f.description}</p>
                  <ol className="mt-2 list-decimal space-y-1 pl-4 text-xs text-muted-foreground">
                    {f.steps.map((s) => (
                      <li key={s}>{s}</li>
                    ))}
                  </ol>
                </li>
              ))}
            </ul>
          </section>

          {bp.dual_company_layers ? (
            <section className="rounded-lg border border-admin-border bg-admin-panel/20 p-4">
              <h2 className="mb-3 text-sm font-semibold">Dual company architecture</h2>
              <div className="text-sm leading-relaxed text-muted-foreground [&_strong]:font-semibold [&_strong]:text-foreground [&_p]:mb-3 [&_p:last-child]:mb-0">
                <ReactMarkdown
                  components={{
                    ol: ({ children }) => <ol className="list-decimal space-y-2 pl-5">{children}</ol>,
                    li: ({ children }) => <li className="text-muted-foreground">{children}</li>,
                  }}
                >
                  {bp.dual_company_layers}
                </ReactMarkdown>
              </div>
            </section>
          ) : null}

          <section className="rounded-lg border border-admin-border bg-admin-panel/20 p-4">
            <h2 className="mb-2 text-sm font-semibold">Entry points</h2>
            <ul className="columns-1 gap-x-8 font-mono text-[10px] text-muted-foreground sm:columns-2">
              {bp.entry_points.map((e) => (
                <li key={e} className="break-all py-0.5">
                  {e}
                </li>
              ))}
            </ul>
          </section>

          <section className="rounded-lg border border-admin-border bg-admin-panel/20 p-4">
            <h2 className="mb-2 text-sm font-semibold">Shared abstractions</h2>
            <ul className="flex flex-wrap gap-2">
              {bp.shared_abstractions.map((a) => (
                <li
                  key={a}
                  className="rounded-md border border-admin-border bg-admin-bg px-2 py-1 font-mono text-[10px] text-foreground"
                >
                  {a}
                </li>
              ))}
            </ul>
          </section>
        </>
      ) : !err && loading ? (
        <p className="text-sm text-muted-foreground">Loading blueprint…</p>
      ) : null}

      <p className="text-xs text-admin-muted">
        <a href={`${apiBase}/api/architecture`} className="inline-flex items-center gap-1 underline-offset-4 hover:underline" target="_blank" rel="noreferrer">
          Open raw JSON <ExternalLink className="size-3" />
        </a>
      </p>
    </div>
  );
}
