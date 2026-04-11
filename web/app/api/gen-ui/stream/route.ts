import type { Spec } from "@json-render/core";
import { hsmDashboardCatalog } from "@/lib/gen-ui/hsm-catalog";
import { hsmGenUiSystemMessage, hsmGenUiUserMessage } from "@/lib/gen-ui/llm-prompt";
import { specToJsonlPatchLines } from "@/lib/gen-ui/spec-to-stream-patches";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/** OpenAI-compatible POST target: supports `https://api.openai.com` and `https://openrouter.ai/api/v1`. */
function chatCompletionsUrl(baseRaw: string): string {
  const base = baseRaw.replace(/\/$/, "");
  return base.endsWith("/v1") ? `${base}/chat/completions` : `${base}/v1/chat/completions`;
}

const DEFAULT_OPENROUTER_MODEL = "openai/gpt-oss-120b:free";
const DEFAULT_OPENAI_MODEL = "gpt-4o-mini";

type ResolvedProvider = {
  base: string;
  defaultModel: string;
  headers: Record<string, string>;
};

function resolveOpenAiCompatibleProvider(): ResolvedProvider | { error: string } {
  const openrouterKey = process.env.OPENROUTER_API_KEY?.trim();
  const openaiKey = process.env.OPENAI_API_KEY?.trim();
  const routerBase = process.env.OPENROUTER_BASE_URL?.trim();
  const openaiBase = process.env.OPENAI_BASE_URL?.trim();

  if (openrouterKey) {
    const base = (routerBase || openaiBase || "https://openrouter.ai/api/v1").replace(/\/$/, "");
    const headers: Record<string, string> = {
      Authorization: `Bearer ${openrouterKey}`,
      "Content-Type": "application/json",
    };
    const referer = process.env.OPENROUTER_HTTP_REFERER?.trim();
    const title = process.env.OPENROUTER_APP_TITLE?.trim();
    if (referer) headers["HTTP-Referer"] = referer;
    if (title) headers["X-OpenRouter-Title"] = title;
    return { base, defaultModel: DEFAULT_OPENROUTER_MODEL, headers };
  }

  if (openaiKey) {
    const base = (openaiBase || "https://api.openai.com").replace(/\/$/, "");
    return {
      base,
      defaultModel: DEFAULT_OPENAI_MODEL,
      headers: {
        Authorization: `Bearer ${openaiKey}`,
        "Content-Type": "application/json",
      },
    };
  }

  return {
    error:
      "No API key: set OPENROUTER_API_KEY (recommended; defaults to OpenRouter + free GPT-OSS 120B) or OPENAI_API_KEY for any OpenAI-compatible endpoint (OpenAI, Anthropic via a proxy, Groq, etc.). Optional: OPENROUTER_BASE_URL / OPENAI_BASE_URL, HSM_GEN_UI_MODEL.",
  };
}

const JSON_SUFFIX = `

---
CRITICAL: Output a single JSON object only (no markdown, no code fences).
Shape:
{
  "root": "<id of root element>",
  "elements": {
    "<id>": {
      "type": "DashboardRoot" | "MetricRow" | "TextBlock" | "AlertBanner" | "BulletList" | "ListItem",
      "props": { ... },
      "children": ["childId", ...]   // optional; omit or [] for leaves
    }
  }
}
Every "type" must be exactly one of the allowed component names. "root" must match one key in "elements".`;

interface StreamBody {
  prompt?: string;
  context?: { previousSpec?: Spec | null };
  currentSpec?: Spec | null;
}

function parseAssistantJsonContent(raw: string): unknown {
  const t = raw.trim();
  if (t.startsWith("```")) {
    const withoutFence = t.replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/i, "");
    return JSON.parse(withoutFence);
  }
  return JSON.parse(t);
}

async function generateSpecFromLlm(
  provider: ResolvedProvider,
  userPrompt: string,
  prior: Spec | null | undefined,
): Promise<{ spec: Spec; usage?: { prompt: number; completion: number; total: number } }> {
  const model = process.env.HSM_GEN_UI_MODEL?.trim() || provider.defaultModel;
  const url = chatCompletionsUrl(provider.base);

  const system = `${hsmGenUiSystemMessage()}${JSON_SUFFIX}`;
  const user = hsmGenUiUserMessage(userPrompt, prior ?? null);

  const res = await fetch(url, {
    method: "POST",
    headers: provider.headers,
    body: JSON.stringify({
      model,
      messages: [
        { role: "system", content: system },
        { role: "user", content: user },
      ],
      response_format: { type: "json_object" },
      temperature: 0.35,
    }),
  });

  if (!res.ok) {
    const errText = await res.text();
    throw new Error(`LLM ${res.status}: ${errText.slice(0, 500)}`);
  }

  const data = (await res.json()) as {
    choices?: { message?: { content?: string } }[];
    usage?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number };
  };
  const content = data.choices?.[0]?.message?.content;
  if (!content) {
    throw new Error("Empty completion from model");
  }

  let parsed: unknown;
  try {
    parsed = parseAssistantJsonContent(content);
  } catch (e) {
    throw new Error(`Model returned non-JSON: ${(e as Error).message}`);
  }

  const validated = hsmDashboardCatalog.validate(parsed);
  if (!validated.success) {
    const msg = validated.error?.message ?? "catalog validation failed";
    throw new Error(msg);
  }

  const spec = validated.data as Spec;
  if (!spec.root || !spec.elements?.[spec.root]) {
    throw new Error('Spec must include "root" and elements[root]');
  }

  return {
    spec,
    usage: data.usage
      ? {
          prompt: data.usage.prompt_tokens ?? 0,
          completion: data.usage.completion_tokens ?? 0,
          total: data.usage.total_tokens ?? 0,
        }
      : undefined,
  };
}

function nonEmptyPriorSpec(prior: unknown): Spec | null {
  if (!prior || typeof prior !== "object") return null;
  const s = prior as Spec;
  if (typeof s.root !== "string" || !s.root || !s.elements?.[s.root]) return null;
  return s;
}

export async function POST(request: Request) {
  const resolved = resolveOpenAiCompatibleProvider();
  if ("error" in resolved) {
    return Response.json({ error: resolved.error, message: resolved.error }, { status: 503 });
  }

  let body: StreamBody;
  try {
    body = (await request.json()) as StreamBody;
  } catch {
    return Response.json({ error: "Invalid JSON body" }, { status: 400 });
  }

  const prompt = typeof body.prompt === "string" ? body.prompt.trim() : "";
  if (!prompt) {
    return Response.json({ error: "Missing prompt" }, { status: 400 });
  }

  const priorRaw = body.context?.previousSpec ?? body.currentSpec;
  const priorForPrompt = nonEmptyPriorSpec(priorRaw);

  let spec: Spec;
  let usage: { prompt: number; completion: number; total: number } | undefined;
  try {
    const out = await generateSpecFromLlm(resolved, prompt, priorForPrompt);
    spec = out.spec;
    usage = out.usage;
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    return Response.json({ error: message, message }, { status: 502 });
  }

  const lines = specToJsonlPatchLines(spec);
  const encoder = new TextEncoder();

  const stream = new ReadableStream({
    async start(controller) {
      try {
        for (const line of lines) {
          controller.enqueue(encoder.encode(`${line}\n`));
        }
        controller.enqueue(
          encoder.encode(
            `${JSON.stringify({
              __meta: "usage",
              promptTokens: usage?.prompt ?? 0,
              completionTokens: usage?.completion ?? 0,
              totalTokens: usage?.total ?? 0,
            })}\n`,
          ),
        );
      } catch (e) {
        controller.error(e);
        return;
      }
      controller.close();
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Cache-Control": "no-store",
    },
  });
}
