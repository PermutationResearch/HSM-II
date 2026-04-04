import Link from "next/link";
import { getHonchopeers } from "@/lib/api";
import type { UserRepresentation } from "@/lib/api";

export const revalidate = 30;

export default async function PeersPage() {
  let peers: UserRepresentation[] = [];
  try {
    const res = await getHonchopeers();
    peers = res.peers ?? [];
  } catch {
    // API not yet running
  }

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold text-zinc-100">Peers</h1>
      <p className="text-zinc-400 text-sm">
        Honcho cross-session user representations — enriched automatically after each session.
      </p>

      {peers.length === 0 ? (
        <p className="text-zinc-500 text-sm">
          No peers yet. Enable <code className="text-emerald-400">HSM_HONCHO=1</code> and start a session.
        </p>
      ) : (
        <div className="grid gap-4">
          {peers.map((peer) => (
            <PeerCard key={peer.peer_id} peer={peer} />
          ))}
        </div>
      )}
    </div>
  );
}

function PeerCard({ peer }: { peer: UserRepresentation }) {
  const pct = (peer.confidence * 100).toFixed(0);
  return (
    <Link href={`/peers/${encodeURIComponent(peer.peer_id)}`}>
      <div className="rounded-lg border border-zinc-800 bg-zinc-900 hover:border-zinc-600 transition-colors px-5 py-4 space-y-3">
        <div className="flex items-center justify-between">
          <span className="font-medium text-zinc-100">{peer.peer_id}</span>
          <div className="flex gap-3 text-xs text-zinc-500">
            <span>{peer.session_count} sessions</span>
            <span>{peer.total_messages} messages</span>
            <span className="text-violet-400">{pct}% confidence</span>
          </div>
        </div>

        {peer.communication_style && (
          <p className="text-sm text-zinc-400 line-clamp-2">{peer.communication_style}</p>
        )}

        <div className="flex flex-wrap gap-2">
          {peer.traits.slice(0, 6).map((t) => (
            <span key={t.label} className="rounded-full border border-zinc-700 px-2 py-0.5 text-xs text-zinc-300">
              {t.label}
            </span>
          ))}
          {peer.goals.slice(0, 3).map((g) => (
            <span key={g.description} className="rounded-full border border-emerald-800 bg-emerald-950 px-2 py-0.5 text-xs text-emerald-300">
              {g.description.slice(0, 40)}
            </span>
          ))}
        </div>
      </div>
    </Link>
  );
}
