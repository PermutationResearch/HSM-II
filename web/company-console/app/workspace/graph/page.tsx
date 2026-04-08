"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { ExternalLink, RefreshCw } from "lucide-react";
import { TrailGraphView, type GraphLink, type GraphNode, type TrailGraphPayload } from "@/app/components/TrailGraphView";
import { Button } from "@/app/components/ui/button";
import { useWorkspace } from "@/app/context/WorkspaceContext";

function downloadJson(filename: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = filename;
  a.click();
  URL.revokeObjectURL(a.href);
}

export default function WorkspaceGraphPage() {
  const { apiBase, companyId, companies } = useWorkspace();
  const companyLabel = companies.find((c) => c.id === companyId)?.display_name ?? null;
  const [trailGraph, setTrailGraph] = useState<TrailGraphPayload | null>(null);
  const [hyperFileGraph, setHyperFileGraph] = useState<{
    path: string | null;
    graph: { nodes?: GraphNode[]; links?: GraphLink[] };
  } | null>(null);
  const [hyperHint, setHyperHint] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setErr(null);
    setLoading(true);
    try {
      const [g, hg] = await Promise.all([
        fetch(`${apiBase}/api/console/graph/trail?limit=500`).then((r) => {
          /**
           * Console API may be unavailable if HSM core isn’t running on apiBase.
           */
          if (!r.ok) throw new Error(`trail graph ${r.status}`);
          return r.json() as Promise<TrailGraphPayload>;
        }),
        fetch(`${apiBase}/api/console/graph/hypergraph`).then((r) => {
          if (!r.ok) throw new Error(`hypergraph ${r.status}`);
          return r.json() as Promise<{
            path?: string | null;
            graph?: { nodes?: GraphNode[]; links?: GraphLink[] };
            hint?: string;
          }>;
        }),
      ]);
      setTrailGraph(g);
      setHyperFileGraph(
        hg?.path
          ? { path: hg.path as string, graph: (hg.graph ?? {}) as { nodes?: GraphNode[]; links?: GraphLink[] } }
          : null,
      );
      if (hg?.hint && !hg?.path) setHyperHint(String(hg.hint));
      else setHyperHint(null);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setTrailGraph(null);
      setHyperFileGraph(null);
      setHyperHint(null);
    } finally {
      setLoading(false);
    }
  }, [apiBase]);

  useEffect(() => {
    void load();
  }, [load]);

  const trailPreviewUrl = `${apiBase}/api/console/graph/trail?limit=500`;
  const hyperPreviewUrl = `${apiBase}/api/console/graph/hypergraph`;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="pc-page-eyebrow">Workflow transparency</p>
          <h1 className="pc-page-title">Graph</h1>
          <p className="pc-page-desc">
            Hyperedges from task trail JSONL (relation hub → participants). Same data as the{" "}
            <Link href="/" className="text-primary underline-offset-4 hover:underline">
              legacy console
            </Link>{" "}
            Graph view; export file-based hypergraph via{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">viz/hyper_graph.json</code> or{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">memory/hyper_graph.json</code>.
            {companyLabel ? (
              <>
                {" "}
                Selected company: <span className="text-foreground/90">{companyLabel}</span> (these endpoints are global; use{" "}
                <Link href="/workspace/intelligence" className="text-primary underline-offset-4 hover:underline">
                  Intelligence
                </Link>{" "}
                for Postgres company metrics.)
              </>
            ) : null}
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
        </div>
      ) : null}

      {hyperHint ? (
        <div className="rounded border border-amber-900/40 bg-amber-950/20 px-3 py-2 text-xs text-amber-200">
          {hyperHint}
        </div>
      ) : null}

      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="secondary" size="sm" asChild>
          <a href={trailPreviewUrl} target="_blank" rel="noreferrer">
            <ExternalLink className="size-3.5" />
            Open trail-derived JSON
          </a>
        </Button>
        <Button type="button" variant="secondary" size="sm" asChild>
          <a href={hyperPreviewUrl} target="_blank" rel="noreferrer">
            <ExternalLink className="size-3.5" />
            Open hypergraph API JSON
          </a>
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={!trailGraph?.graph}
          onClick={() => trailGraph && downloadJson("trail_graph_export.json", trailGraph)}
        >
          Download trail graph
        </Button>
        {hyperFileGraph?.path &&
        ((hyperFileGraph.graph.nodes?.length ?? 0) > 0 || (hyperFileGraph.graph.links?.length ?? 0) > 0) ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => downloadJson("hyper_graph_export.json", hyperFileGraph.graph)}
          >
            Download file export snapshot
          </Button>
        ) : null}
        {hyperFileGraph?.path ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="font-mono text-xs"
            onClick={() => void navigator.clipboard.writeText(hyperFileGraph.path!)}
          >
            Copy server path
          </Button>
        ) : null}
      </div>

      <div>
        <div className="mb-2 text-xs text-muted-foreground">
          From task trail (<code className="font-mono">hyperedge</code> events)
        </div>
        {loading ? (
          <div className="rounded border border-admin-border bg-card p-8 text-sm text-muted-foreground">Loading graph…</div>
        ) : (
          <TrailGraphView
            graph={trailGraph?.graph}
            emptyClassName="rounded border border-admin-border bg-card p-8 text-center text-sm text-muted-foreground"
          />
        )}
      </div>

      {hyperFileGraph?.path &&
      ((hyperFileGraph.graph.nodes?.length ?? 0) > 0 || (hyperFileGraph.graph.links?.length ?? 0) > 0) ? (
        <div>
          <div className="mb-2 mt-8 text-xs text-muted-foreground">
            File export: <code className="font-mono text-foreground">{hyperFileGraph.path}</code>
          </div>
          <TrailGraphView
            graph={{
              nodes: hyperFileGraph.graph.nodes ?? [],
              links: hyperFileGraph.graph.links ?? [],
            }}
            emptyClassName="rounded border border-admin-border bg-card p-8 text-center text-sm text-muted-foreground"
          />
        </div>
      ) : null}
    </div>
  );
}
