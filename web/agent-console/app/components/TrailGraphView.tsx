"use client";

import { useMemo } from "react";

export type GraphNode = { id: string; label: string; kind: string };
export type GraphLink = { source: string; target: string; rel?: string };

export type TrailGraphPayload = {
  source: string;
  graph: { nodes: GraphNode[]; links: GraphLink[] };
};

export function TrailGraphView({
  graph,
  emptyClassName = "rounded border border-line bg-panel p-8 text-center text-sm text-gray-500",
}: {
  graph?: { nodes: GraphNode[]; links: GraphLink[] };
  /** Tailwind classes for the empty state container (legacy vs workspace). */
  emptyClassName?: string;
}) {
  const layout = useMemo(() => {
    const nodes = graph?.nodes ?? [];
    if (!nodes.length) return { pts: new Map<string, { x: number; y: number }>(), w: 400, h: 320 };

    const w = 520;
    const h = 420;
    const cx = w / 2;
    const cy = h / 2;
    const r = Math.min(w, h) / 2 - 40;
    const pts = new Map<string, { x: number; y: number }>();
    nodes.forEach((n, i) => {
      const ang = (2 * Math.PI * i) / nodes.length - Math.PI / 2;
      pts.set(n.id, { x: cx + r * Math.cos(ang), y: cy + r * Math.sin(ang) });
    });
    return { pts, w, h };
  }, [graph]);

  const nodes = graph?.nodes ?? [];
  const links = graph?.links ?? [];

  if (!nodes.length) {
    return (
      <div className={emptyClassName}>
        No hyperedge events in trail yet. Use{" "}
        <code className="font-mono text-gray-400">record_hyperedge</code> from the agent to populate.
      </div>
    );
  }

  const { pts, w, h } = layout;

  return (
    <div className="rounded border border-line bg-panel p-4">
      <svg width={w} height={h} className="mx-auto text-gray-200">
        {links.map((L, i) => {
          const a = pts.get(L.source);
          const b = pts.get(L.target);
          if (!a || !b) return null;
          return (
            <line
              key={i}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              stroke="rgba(148,163,184,0.35)"
              strokeWidth={1}
            />
          );
        })}
        {nodes.map((n) => {
          const p = pts.get(n.id);
          if (!p) return null;
          const col = n.kind === "relation" ? "#38bdf8" : "#a78bfa";
          return (
            <g key={n.id}>
              <circle cx={p.x} cy={p.y} r={n.kind === "relation" ? 8 : 5} fill={col} opacity={0.9} />
              <text x={p.x + 10} y={p.y + 4} fontSize={10} fill="#e2e8f0" className="select-none">
                {(n.label || n.id).slice(0, 32)}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="mt-2 text-center text-xs text-gray-600">
        {nodes.length} nodes · {links.length} links (trail-derived)
      </div>
    </div>
  );
}
