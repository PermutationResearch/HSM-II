/**
 * POST /api/council/deliberate
 *
 * Streams a multi-agent Socratic council deliberation as NDJSON.
 * Each registered agent speaks in turn, grounded in their role/capabilities,
 * and the council resolves to a task-assignment consensus.
 *
 * NDJSON event types:
 *   { type: "start" }
 *   { type: "turn_start", agent: string, role: string, round: number }
 *   { type: "token",      agent: string, delta: string }
 *   { type: "turn_done",  agent: string, content: string }
 *   { type: "consensus_start" }
 *   { type: "done",       consensus: string }
 *   { type: "error",      message: string }
 */
import { NextRequest } from "next/server";
import { OR_BASE, CHAT_MODEL, readOpenRouterApiKey } from "@/app/lib/agent-chat-server";
import { asObject } from "@/app/lib/runtime-contract";

interface CouncilAgent {
  name: string;
  role?: string | null;
  title?: string | null;
  capabilities?: string | null;
}

interface RequestBody {
  query: string;
  context?: string | null;
  /** Accumulated prior-deliberation history (may be compacted) injected by the client */
  prior_context?: string | null;
  agents: CouncilAgent[];
  rounds?: number;
}

function parseBody(raw: unknown): RequestBody | null {
  const o = asObject(raw);
  if (!o) return null;
  const query = typeof o.query === "string" ? o.query.trim() : "";
  if (!query) return null;
  const agents = Array.isArray(o.agents) ? (o.agents as CouncilAgent[]).filter((a) => a?.name) : [];
  if (agents.length === 0) return null;
  return {
    query,
    context: typeof o.context === "string" ? o.context.trim() || null : null,
    prior_context: typeof o.prior_context === "string" ? o.prior_context.trim() || null : null,
    agents,
    rounds: typeof o.rounds === "number" ? Math.min(Math.max(1, o.rounds), 3) : 1,
  };
}

/** Call OpenRouter and stream tokens via the writeLine callback. Returns full content. */
async function streamAgentTurn(
  systemPrompt: string,
  userMessage: string,
  agentName: string,
  apiKey: string,
  writeLine: (obj: Record<string, unknown>) => Promise<void>,
): Promise<string> {
  const res = await fetch(`${OR_BASE}/chat/completions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
      "HTTP-Referer": "https://hsm.ai",
      "X-Title": "HSM Council Chamber",
    },
    body: JSON.stringify({
      model: CHAT_MODEL,
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: userMessage },
      ],
      max_tokens: 300,
      temperature: 0.75,
      stream: true,
    }),
  });

  if (!res.ok || !res.body) {
    const errText = await res.text().catch(() => res.statusText);
    throw new Error(`OpenRouter ${res.status}: ${errText}`);
  }

  let full = "";
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const lines = buf.split("\n");
    buf = lines.pop() ?? "";
    for (const line of lines) {
      const trimmed = line.replace(/^data:\s*/, "").trim();
      if (!trimmed || trimmed === "[DONE]") continue;
      try {
        const ev = JSON.parse(trimmed) as unknown;
        const evObj = asObject(ev);
        const choices = Array.isArray(evObj?.choices) ? evObj.choices : [];
        const delta = asObject(asObject(choices[0])?.delta);
        const token = typeof delta?.content === "string" ? delta.content : "";
        if (token) {
          full += token;
          await writeLine({ type: "token", agent: agentName, delta: token });
        }
      } catch {
        /* skip malformed SSE line */
      }
    }
  }

  return full;
}

function agentLabel(a: CouncilAgent): string {
  const parts = [a.title, a.role].filter(Boolean);
  return parts.length > 0 ? `${a.name} (${parts.join(", ")})` : a.name;
}

export async function POST(req: NextRequest) {
  const raw = await req.json().catch(() => null);
  const body = parseBody(raw);

  const encoder = new TextEncoder();
  const { readable, writable } = new TransformStream<Uint8Array, Uint8Array>();
  const writer = writable.getWriter();

  const writeLine = async (obj: Record<string, unknown>) => {
    await writer.write(encoder.encode(`${JSON.stringify(obj)}\n`));
  };

  if (!body) {
    void (async () => {
      await writeLine({ type: "error", message: "query and at least one agent are required" });
      await writer.close();
    })();
    return new Response(readable, {
      status: 400,
      headers: { "Content-Type": "application/x-ndjson; charset=utf-8" },
    });
  }

  void (async () => {
    try {
      const apiKey = readOpenRouterApiKey();
      if (!apiKey) throw new Error("OpenRouter API key not configured (OPENROUTER_API_KEY or HSM_OPENROUTER_API_KEY)");

      await writeLine({ type: "start" });

      const rosterSummary = body.agents
        .map((a) => `- ${agentLabel(a)}${a.capabilities ? `: ${a.capabilities}` : ""}`)
        .join("\n");

      const history: Array<{ agent: string; content: string; round: number }> = [];

      for (let round = 0; round < body.rounds!; round++) {
        for (const agent of body.agents) {
          const priorTurns =
            history.length > 0
              ? history.map((h) => `[${h.agent}, r${h.round + 1}]: ${h.content}`).join("\n\n")
              : "None yet — you speak first.";

          const systemPrompt = [
            `You are ${agent.name}${agent.title ? `, ${agent.title}` : ""}${agent.role ? ` (${agent.role})` : ""}.`,
            agent.capabilities ? `Your expertise: ${agent.capabilities}` : null,
            `You are participating in a council deliberation with these colleagues:\n${rosterSummary}`,
            `\nThe council task or question is:\n"${body.query}"`,
            body.context ? `\nAdditional context:\n${body.context}` : null,
            body.prior_context
              ? `\n## Prior council history\nThe following are decisions and deliberations the council has already conducted. Use them as background — do not simply repeat them, but build on them where relevant.\n\n${body.prior_context}`
              : null,
            `\nSpeak in 2–4 sentences. Be direct and specific. Reference your domain expertise. Ask a probing question or challenge an assumption if relevant. Do not be sycophantic. Do not repeat what others said verbatim.`,
          ]
            .filter(Boolean)
            .join("\n");

          const userMessage =
            round === 0
              ? `Council deliberation — round ${round + 1}. Share your initial perspective on the task.`
              : `Council deliberation — round ${round + 1}. Prior discussion:\n\n${priorTurns}\n\nReact to the discussion and build toward resolution.`;

          await writeLine({ type: "turn_start", agent: agent.name, role: agent.role ?? agent.title ?? "", round });
          const content = await streamAgentTurn(systemPrompt, userMessage, agent.name, apiKey, writeLine);
          await writeLine({ type: "turn_done", agent: agent.name, content, round });
          history.push({ agent: agent.name, content, round });
        }
      }

      // Consensus turn
      await writeLine({ type: "consensus_start" });

      const deliberationSummary = history
        .map((h) => `[${h.agent}, r${h.round + 1}]: ${h.content}`)
        .join("\n\n");

      const consensusSystem = [
        `You are a neutral council secretary recording the outcome of a deliberation.`,
        `Council members: ${body.agents.map((a) => agentLabel(a)).join("; ")}`,
        `\nTask deliberated:\n"${body.query}"`,
        body.context ? `\nContext:\n${body.context}` : null,
        body.prior_context
          ? `\nPrior council history (for continuity with past decisions):\n${body.prior_context}`
          : null,
        `\nFull deliberation:\n${deliberationSummary}`,
        `\nWrite a council resolution in 3 parts:`,
        `1. **Assigned to**: Name the best-fit agent and one-sentence rationale grounded in their expertise.`,
        `2. **Key insight**: One sentence capturing the sharpest point raised.`,
        `3. **Conditions**: Any collaboration or prerequisite needed (or "None" if self-contained).`,
        `Be specific. Use the agents' actual names.`,
      ]
        .filter(Boolean)
        .join("\n");

      const consensusContent = await streamAgentTurn(
        consensusSystem,
        "Produce the council resolution now.",
        "council",
        apiKey,
        writeLine,
      );

      await writeLine({ type: "done", consensus: consensusContent });
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      try {
        await writeLine({ type: "error", message });
      } catch {
        /* ignore */
      }
    } finally {
      try {
        await writer.close();
      } catch {
        /* ignore */
      }
    }
  })();

  return new Response(readable, {
    headers: {
      "Content-Type": "application/x-ndjson; charset=utf-8",
      "Cache-Control": "no-store",
      Connection: "keep-alive",
    },
  });
}
