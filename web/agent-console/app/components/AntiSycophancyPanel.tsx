"use client";

import { useState } from "react";
import { Panel } from "./Panel";

type CriticVerdict = "accept" | "revise";

type CriticParse = {
  raw: string;
  risk_score: number;
  issues: string[];
  revised_directives: string[];
  verdict: CriticVerdict;
};

type RoundLog = {
  index: number;
  draft_excerpt: string;
  heuristic_risk: number;
  critic: CriticParse;
};

type RunResult = {
  final_text: string;
  aggregated_directives: string[];
  rounds: RoundLog[];
  stopped_reason: string;
};

type Props = {
  api: string;
  setErr: (msg: string | null) => void;
};

export function AntiSycophancyPanel({ api, setErr }: Props) {
  const [userMsg, setUserMsg] = useState("");
  const [draft, setDraft] = useState("");
  const [ctx, setCtx] = useState("");
  const [seeds, setSeeds] = useState("");
  const [maxRounds, setMaxRounds] = useState(3);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<RunResult | null>(null);
  const [apiErr, setApiErr] = useState<string | null>(null);

  async function run() {
    setErr(null);
    setApiErr(null);
    setResult(null);
    const um = userMsg.trim();
    const dr = draft.trim();
    if (!um || !dr) {
      setApiErr("User message and draft are required.");
      return;
    }
    setLoading(true);
    try {
      const seed_directives = seeds
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean);
      const r = await fetch(`${api}/api/console/anti-sycophancy`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          user_message: um,
          draft_response: dr,
          context: ctx.trim() || null,
          seed_directives,
          max_rounds: maxRounds,
        }),
      });
      const j = (await r.json()) as { ok?: boolean; error?: string; result?: RunResult };
      if (!r.ok || !j.ok) {
        setApiErr(j.error ?? r.statusText);
        return;
      }
      if (j.result) setResult(j.result);
    } catch (e) {
      setApiErr(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-medium text-white">Anti-sycophancy loop</h1>
        <p className="mt-2 max-w-3xl text-sm text-gray-500">
          A critic pass scores flattery, false agreement, and epistemic risk; optional rewrite rounds merge
          stricter directives until risk drops or rounds cap. Runs on the server via{" "}
          <code className="rounded bg-white/5 px-1 font-mono text-[11px]">POST /api/console/anti-sycophancy</code>
          .
        </p>
      </header>

      {apiErr && (
        <div className="rounded border border-red-900/50 bg-red-950/30 px-3 py-2 text-sm text-red-300">
          {apiErr}
        </div>
      )}

      <div className="grid gap-4 lg:grid-cols-2">
        <Panel title="Input">
          <label className="mb-1 block text-[11px] uppercase text-gray-500">User message</label>
          <textarea
            className="mb-3 min-h-[100px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={userMsg}
            onChange={(e) => setUserMsg(e.target.value)}
            placeholder="What the user asked..."
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Model draft (to audit)</label>
          <textarea
            className="mb-3 min-h-[160px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Paste assistant output..."
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Context (optional)</label>
          <textarea
            className="mb-3 min-h-[72px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={ctx}
            onChange={(e) => setCtx(e.target.value)}
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">
            Seed directives (one per line)
          </label>
          <textarea
            className="mb-3 min-h-[64px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={seeds}
            onChange={(e) => setSeeds(e.target.value)}
            placeholder="Prefer concise answers&#10;Cite uncertainty when needed"
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Max rounds (1–6)</label>
          <input
            type="number"
            min={1}
            max={6}
            className="mb-3 w-24 rounded border border-line bg-ink px-2 py-1 text-sm"
            value={maxRounds}
            onChange={(e) => setMaxRounds(Number(e.target.value) || 1)}
          />
          <button
            type="button"
            disabled={loading}
            className="rounded bg-accent/25 px-4 py-2 text-sm font-medium text-accent disabled:opacity-40"
            onClick={() => void run()}
          >
            {loading ? "Running…" : "Run critique loop"}
          </button>
        </Panel>

        <Panel title="Result">
          {result ? (
            <div className="space-y-4 text-sm">
              <div className="text-xs text-gray-500">
                Stopped: <span className="text-gray-300">{result.stopped_reason}</span>
              </div>
              {result.aggregated_directives.length > 0 && (
                <div>
                  <div className="mb-1 text-[11px] uppercase text-gray-500">Aggregated directives</div>
                  <ul className="list-inside list-disc text-xs text-gray-400">
                    {result.aggregated_directives.map((d, i) => (
                      <li key={i}>{d}</li>
                    ))}
                  </ul>
                </div>
              )}
              <div>
                <div className="mb-1 text-[11px] uppercase text-gray-500">Final text</div>
                <pre className="max-h-[280px] overflow-auto whitespace-pre-wrap rounded border border-line bg-black/30 p-3 text-xs text-gray-300">
                  {result.final_text}
                </pre>
              </div>
              <div>
                <div className="mb-1 text-[11px] uppercase text-gray-500">Rounds</div>
                <ul className="space-y-3 text-xs text-gray-400">
                  {result.rounds.map((r) => (
                    <li key={r.index} className="rounded border border-line/60 bg-ink/40 p-2">
                      <div className="font-mono text-[10px] text-gray-500">
                        Round {r.index} · model risk {r.critic.risk_score.toFixed(2)} · heuristic{" "}
                        {r.heuristic_risk.toFixed(2)} · {r.critic.verdict}
                      </div>
                      {r.critic.issues.length > 0 && (
                        <ul className="mt-1 list-inside list-disc">
                          {r.critic.issues.map((x, i) => (
                            <li key={i}>{x}</li>
                          ))}
                        </ul>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          ) : (
            <p className="text-sm text-gray-600">Run the loop to see critiques and revised output.</p>
          )}
        </Panel>
      </div>
    </div>
  );
}
