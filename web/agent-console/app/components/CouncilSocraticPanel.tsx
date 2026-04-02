"use client";

import { useState } from "react";
import { Panel } from "./Panel";

type CouncilTurn = {
  round: number;
  role: string;
  content: string;
};

type AntiResult = {
  final_text: string;
  aggregated_directives: string[];
  rounds: {
    index: number;
    critic: { risk_score: number; verdict: string; issues: string[] };
  }[];
  stopped_reason: string;
};

type CouncilResult = {
  proposition: string;
  roles_used: string[];
  turns: CouncilTurn[];
  synthesis_draft: string;
  anti_sycophancy: AntiResult;
};

type Props = {
  api: string;
  setErr: (msg: string | null) => void;
};

const DEFAULT_ROLES = "socratic_questioner\nepistemic_critic\nintegrator";

export function CouncilSocraticPanel({ api, setErr }: Props) {
  const [proposition, setProposition] = useState("");
  const [context, setContext] = useState("");
  const [rolesText, setRolesText] = useState(DEFAULT_ROLES);
  const [councilRounds, setCouncilRounds] = useState(1);
  const [antiRounds, setAntiRounds] = useState(3);
  const [seeds, setSeeds] = useState("Prefer concise answers\nDisagree when evidence warrants");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<CouncilResult | null>(null);
  const [apiErr, setApiErr] = useState<string | null>(null);

  async function run() {
    setErr(null);
    setApiErr(null);
    setResult(null);
    const p = proposition.trim();
    if (!p) {
      setApiErr("Proposition is required.");
      return;
    }
    setLoading(true);
    try {
      const roles = rolesText
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean);
      const seed_directives = seeds
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean);
      const r = await fetch(`${api}/api/console/council-socratic`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          proposition: p,
          context: context.trim() || null,
          roles,
          council_rounds: councilRounds,
          seed_directives,
          anti_sycophancy_max_rounds: antiRounds,
        }),
      });
      const j = (await r.json()) as { ok?: boolean; error?: string; result?: CouncilResult };
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
        <h2 className="text-lg font-medium text-white">Socratic council + anti-sycophancy</h2>
        <p className="mt-2 max-w-3xl text-sm text-gray-500">
          Role agents take turns in Socratic style (questions, epistemic challenge, integration), then a draft is
          synthesized and passed through the same critic loop as{" "}
          <code className="rounded bg-white/5 px-1 font-mono text-[11px]">/api/console/anti-sycophancy</code>.
        </p>
      </header>

      {apiErr && (
        <div className="rounded border border-red-900/50 bg-red-950/30 px-3 py-2 text-sm text-red-300">{apiErr}</div>
      )}

      <div className="grid gap-4 lg:grid-cols-2">
        <Panel title="Council input">
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Proposition / question</label>
          <textarea
            className="mb-3 min-h-[120px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={proposition}
            onChange={(e) => setProposition(e.target.value)}
            placeholder="What should the council deliberate?"
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Context (optional)</label>
          <textarea
            className="mb-3 min-h-[72px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={context}
            onChange={(e) => setContext(e.target.value)}
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">
            Role ids (one per line; empty uses defaults)
          </label>
          <textarea
            className="mb-3 min-h-[72px] w-full rounded border border-line bg-ink px-3 py-2 font-mono text-xs text-gray-200"
            value={rolesText}
            onChange={(e) => setRolesText(e.target.value)}
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Council rounds (1–4)</label>
          <input
            type="number"
            min={1}
            max={4}
            className="mb-3 w-24 rounded border border-line bg-ink px-2 py-1 text-sm"
            value={councilRounds}
            onChange={(e) => setCouncilRounds(Number(e.target.value) || 1)}
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Anti-sycophancy max rounds (1–6)</label>
          <input
            type="number"
            min={1}
            max={6}
            className="mb-3 w-24 rounded border border-line bg-ink px-2 py-1 text-sm"
            value={antiRounds}
            onChange={(e) => setAntiRounds(Number(e.target.value) || 1)}
          />
          <label className="mb-1 block text-[11px] uppercase text-gray-500">Seed directives (one per line)</label>
          <textarea
            className="mb-3 min-h-[64px] w-full rounded border border-line bg-ink px-3 py-2 text-sm text-gray-200"
            value={seeds}
            onChange={(e) => setSeeds(e.target.value)}
          />
          <button
            type="button"
            disabled={loading}
            className="rounded-md bg-primary px-4 py-2 text-sm font-semibold text-primary-foreground shadow-sm hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-40"
            onClick={() => void run()}
          >
            {loading ? "Running council…" : "Run council + critique"}
          </button>
        </Panel>

        <Panel title="Output">
          {result ? (
            <div className="max-h-[70vh] space-y-4 overflow-auto text-sm">
              <p className="text-xs text-gray-500">
                Roles:{" "}
                <span className="font-mono text-gray-400">{result.roles_used.join(", ")}</span>
              </p>
              <div>
                <div className="mb-1 text-[11px] uppercase text-gray-500">Council turns</div>
                <ul className="space-y-2 text-xs text-gray-400">
                  {result.turns.map((t, i) => (
                    <li key={i} className="rounded border border-line/60 bg-ink/40 p-2">
                      <div className="font-mono text-[10px] text-gray-500">
                        r{t.round + 1} · {t.role}
                      </div>
                      <p className="mt-1 whitespace-pre-wrap text-gray-300">{t.content}</p>
                    </li>
                  ))}
                </ul>
              </div>
              <div>
                <div className="mb-1 text-[11px] uppercase text-gray-500">Synthesis (pre–anti-sycophancy)</div>
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded border border-line bg-black/30 p-2 text-xs text-gray-400">
                  {result.synthesis_draft}
                </pre>
              </div>
              <div>
                <div className="mb-1 text-[11px] uppercase text-gray-500">
                  After anti-sycophancy ({result.anti_sycophancy.stopped_reason})
                </div>
                <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded border border-line bg-black/30 p-3 text-xs text-gray-200">
                  {result.anti_sycophancy.final_text}
                </pre>
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              Run to see turns, synthesis, and audited final text.
            </p>
          )}
        </Panel>
      </div>
    </div>
  );
}
