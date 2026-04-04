import { PrettyJson } from "@/components/PrettyJson";
import { getCouncilDecisions } from "@/lib/api";

export const revalidate = 20;

export default async function CouncilPage() {
  let decisions: unknown[] = [];
  try { decisions = await getCouncilDecisions(); } catch { /* API offline */ }

  return (
    <div className="space-y-6 max-w-5xl">
      <h1 className="text-xl font-semibold text-zinc-100">Council decisions</h1>
      <p className="text-zinc-400 text-sm">Live decisions as pretty-printed JSON.</p>
      <div className="space-y-4">
        {decisions.map((d, i) => (
          <div key={i} className="rounded-lg border border-zinc-800 bg-zinc-900 p-4 overflow-auto">
            <PrettyJson value={d} />
          </div>
        ))}
        {decisions.length === 0 && (
          <p className="text-zinc-500 text-sm">No decisions yet. Trigger a council proposal first.</p>
        )}
      </div>
    </div>
  );
}
