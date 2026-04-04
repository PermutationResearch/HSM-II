import { PrettyJson } from "@/components/PrettyJson";
import { getHonchoMemoryStats, getBeliefs } from "@/lib/api";

export const revalidate = 20;

export default async function MemoryPage() {
  const [statsRes, beliefsRes] = await Promise.allSettled([
    getHonchoMemoryStats(),
    getBeliefs(),
  ]);

  const stats = statsRes.status === "fulfilled" ? statsRes.value : null;
  const beliefs = beliefsRes.status === "fulfilled" ? beliefsRes.value : [];

  return (
    <div className="space-y-8 max-w-5xl">
      <h1 className="text-xl font-semibold text-zinc-100">Memory</h1>

      {/* Stats */}
      {stats && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {Object.entries(stats).map(([k, v]) => (
            <div key={k} className="rounded-lg border border-zinc-800 bg-zinc-900 px-4 py-3">
              <p className="text-xs text-zinc-500 mb-1">{k.replace(/_/g, " ")}</p>
              <p className="text-xl font-semibold text-sky-400">{v}</p>
            </div>
          ))}
        </div>
      )}

      {/* Beliefs table */}
      <section>
        <h2 className="text-sm font-medium text-zinc-400 mb-3">Beliefs — JSON</h2>
        <div className="space-y-3">
          {(Array.isArray(beliefs) ? beliefs : []).map((b) => (
            <div key={String((b as { id?: unknown }).id)} className="rounded-lg border border-zinc-800 bg-zinc-900 p-4 overflow-auto">
              <PrettyJson value={b as unknown} />
            </div>
          ))}
          {(!Array.isArray(beliefs) || beliefs.length === 0) && (
            <p className="text-zinc-500 text-sm">
              No beliefs stored. Start the HSM-II API with <code className="text-emerald-400">hsm-api</code>.
            </p>
          )}
        </div>
      </section>
    </div>
  );
}
