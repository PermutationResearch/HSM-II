import { PrettyJson } from "@/components/PrettyJson";
import { getHonchoPeer, getPackedContext } from "@/lib/api";
import type { UserRepresentation, PackedContext } from "@/lib/api";

export const revalidate = 30;

interface Props { params: Promise<{ id: string }> }

export default async function PeerDetailPage({ params }: Props) {
  const { id } = await params;

  let peer: UserRepresentation | null = null;
  let packed: PackedContext | null = null;

  try { peer = await getHonchoPeer(id); } catch { /* not found */ }
  try { packed = await getPackedContext(id, 4096); } catch { /* not ready */ }

  if (!peer) {
    return (
      <div className="text-zinc-500">
        Peer <code className="text-zinc-300">{id}</code> not found.
      </div>
    );
  }

  return (
    <div className="space-y-8 max-w-4xl">
      <div className="flex items-center gap-3">
        <h1 className="text-xl font-semibold text-zinc-100">{peer.peer_id}</h1>
        <span className="rounded-full bg-violet-900 text-violet-300 text-xs px-2 py-0.5">
          {peer.session_count} sessions
        </span>
        <span className="rounded-full bg-sky-900 text-sky-300 text-xs px-2 py-0.5">
          {(peer.confidence * 100).toFixed(0)}% conf
        </span>
      </div>

      {/* Communication style */}
      {peer.communication_style && (
        <section className="rounded-lg border border-zinc-800 bg-zinc-900 px-5 py-4">
          <h2 className="text-xs font-medium text-zinc-500 mb-2 uppercase tracking-wide">Communication style</h2>
          <p className="text-zinc-200 text-sm leading-relaxed">{peer.communication_style}</p>
        </section>
      )}

      {/* Goals */}
      {peer.goals.length > 0 && (
        <section>
          <h2 className="text-xs font-medium text-zinc-500 mb-3 uppercase tracking-wide">Goals</h2>
          <div className="space-y-2">
            {peer.goals.map((g, i) => (
              <div key={i} className="flex items-start gap-3 rounded border border-zinc-800 bg-zinc-900 px-4 py-2">
                <ConfBar value={g.confidence} />
                <p className="text-sm text-zinc-200">{g.description}</p>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Preferences */}
      {peer.preferences.length > 0 && (
        <section>
          <h2 className="text-xs font-medium text-zinc-500 mb-3 uppercase tracking-wide">Preferences</h2>
          <div className="grid grid-cols-2 gap-2">
            {peer.preferences.map((p) => (
              <div key={p.key} className="rounded border border-zinc-800 bg-zinc-900 px-4 py-2 text-sm">
                <span className="text-zinc-400">{p.key}: </span>
                <span className="text-zinc-100">{p.value}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Traits */}
      {peer.traits.length > 0 && (
        <section>
          <h2 className="text-xs font-medium text-zinc-500 mb-3 uppercase tracking-wide">Traits</h2>
          <div className="flex flex-wrap gap-2">
            {peer.traits.map((t) => (
              <div key={t.label} className="group relative">
                <span className="rounded-full border border-zinc-700 bg-zinc-900 px-3 py-1 text-sm text-zinc-300 cursor-default">
                  {t.label}
                </span>
                <div className="absolute bottom-full mb-2 left-0 hidden group-hover:block z-10 w-64 rounded border border-zinc-700 bg-zinc-800 p-2 text-xs text-zinc-300">
                  {t.evidence}
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Raw JSON */}
      <section>
        <h2 className="text-xs font-medium text-zinc-500 mb-3 uppercase tracking-wide">
          Raw representation (JSON)
        </h2>
        <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-4 overflow-auto text-sm">
          <PrettyJson value={peer} />
        </div>
      </section>

      {/* Packed context stats */}
      {packed && (
        <section className="rounded-lg border border-zinc-800 bg-zinc-900 px-5 py-4 space-y-3">
          <h2 className="text-xs font-medium text-zinc-500 uppercase tracking-wide">
            Packed context (budget {packed.budget} tokens)
          </h2>
          <div className="flex gap-6 text-sm">
            <span className="text-zinc-400">Used: <span className="text-zinc-100">{packed.token_count}</span></span>
            <span className="text-zinc-400">Summaries: <span className="text-zinc-100">{packed.entity_summaries.length}</span></span>
            <span className="text-zinc-400">Messages: <span className="text-zinc-100">{packed.messages.length}</span></span>
          </div>
          <div className="h-2 bg-zinc-800 rounded-full overflow-hidden">
            <div
              className="h-full bg-emerald-500 rounded-full transition-all"
              style={{ width: `${Math.min(100, (packed.token_count / packed.budget) * 100).toFixed(1)}%` }}
            />
          </div>
        </section>
      )}
    </div>
  );
}

function ConfBar({ value }: { value: number }) {
  const color = value >= 0.8 ? "bg-emerald-500" : value >= 0.5 ? "bg-yellow-500" : "bg-red-500";
  return (
    <div className="mt-1 w-1 rounded-full h-10 bg-zinc-800 flex-shrink-0 overflow-hidden">
      <div
        className={`w-full rounded-full ${color}`}
        style={{ height: `${(value * 100).toFixed(0)}%`, marginTop: `${((1 - value) * 100).toFixed(0)}%` }}
      />
    </div>
  );
}
