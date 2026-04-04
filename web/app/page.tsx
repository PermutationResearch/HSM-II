import { getHealth, getHonchoMemoryStats, getBeliefs, type Belief } from "@/lib/api";

export const revalidate = 10;

export default async function OverviewPage() {
  const [health, memStats, beliefs] = await Promise.allSettled([
    getHealth(),
    getHonchoMemoryStats(),
    getBeliefs(),
  ]);

  const h = health.status === "fulfilled" ? health.value : null;
  const m = memStats.status === "fulfilled" ? memStats.value : null;
  const b = beliefs.status === "fulfilled" ? beliefs.value : [];

  return (
    <div className="space-y-8">
      <h1 className="text-xl font-semibold text-zinc-100">Overview</h1>

      {/* Status strip */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="API" value={h ? h.status : "unreachable"} accent={h ? "emerald" : "red"} />
        <StatCard label="Version" value={h?.version ?? "—"} accent="zinc" />
        <StatCard label="Entity summaries" value={m?.entity_summaries ?? "—"} accent="violet" />
        <StatCard label="Memory entries" value={m?.total_entries ?? "—"} accent="sky" />
      </div>

      {/* Recent beliefs — structured cards */}
      <section>
        <h2 className="text-sm font-medium text-zinc-400 mb-3">Recent beliefs</h2>
        <div className="grid gap-3">
          {(Array.isArray(b) ? b.slice(0, 10) : []).map((belief: Belief) => (
            <BeliefCard key={String(belief.id)} belief={belief} />
          ))}
          {(!Array.isArray(b) || b.length === 0) && (
            <p className="text-zinc-500 text-sm">No beliefs yet — start a session to populate.</p>
          )}
        </div>
      </section>
    </div>
  );
}

function StatCard({ label, value, accent }: { label: string; value: string | number; accent: string }) {
  const colors: Record<string, string> = {
    emerald: "text-emerald-400",
    red: "text-red-400",
    violet: "text-violet-400",
    sky: "text-sky-400",
    zinc: "text-zinc-300",
  };
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900 px-4 py-3">
      <p className="text-xs text-zinc-500 mb-1">{label}</p>
      <p className={`text-lg font-semibold ${colors[accent] ?? "text-zinc-100"}`}>{String(value)}</p>
    </div>
  );
}

function BeliefCard({ belief }: { belief: Belief }) {
  const conf = belief.confidence;
  const confColor = conf >= 0.8 ? "text-emerald-400" : conf >= 0.5 ? "text-yellow-400" : "text-red-400";

  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900 px-4 py-3 text-sm space-y-1">
      <p className="text-zinc-100">{belief.content}</p>
      <div className="flex gap-4 text-xs text-zinc-500">
        <span>network: <span className="text-zinc-300">{belief.network}</span></span>
        <span>conf: <span className={confColor}>{(conf * 100).toFixed(0)}%</span></span>
        {Array.isArray(belief.tags) && belief.tags.length > 0 && (
          <span>tags: {(belief.tags as string[]).join(", ")}</span>
        )}
      </div>
    </div>
  );
}
