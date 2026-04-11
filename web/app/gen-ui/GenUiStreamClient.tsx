"use client";

import { useUIStream } from "@json-render/react";
import { useCallback, useState } from "react";
import { HsmGenUiView } from "@/components/gen-ui/HsmGenUiView";

const DEFAULT_PROMPT =
  "Build a compact HSM-II status panel: DashboardRoot titled 'Live snapshot', one info AlertBanner, two MetricRows (Beliefs, Edges) with placeholder values, and a short muted TextBlock.";

export function GenUiStreamClient() {
  const [prompt, setPrompt] = useState(DEFAULT_PROMPT);
  const { spec, send, isStreaming, error, clear, rawLines, usage } = useUIStream({
    api: "/api/gen-ui/stream",
  });

  const onGenerate = useCallback(() => {
    void send(prompt.trim(), spec?.root ? { previousSpec: spec } : undefined);
  }, [prompt, send, spec]);

  return (
    <div className="space-y-4 rounded-xl border border-zinc-800 bg-zinc-900/40 p-5">
      <div className="flex flex-col gap-2">
        <label htmlFor="gen-ui-prompt" className="text-xs font-medium uppercase tracking-wide text-zinc-500">
          Prompt (model → Spec → streamed JSONL patches → Renderer)
        </label>
        <textarea
          id="gen-ui-prompt"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={4}
          className="w-full resize-y rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-200 placeholder:text-zinc-600 focus:border-emerald-600 focus:outline-none focus:ring-1 focus:ring-emerald-600"
          disabled={isStreaming}
        />
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => void onGenerate()}
          disabled={isStreaming || !prompt.trim()}
          className="rounded-lg bg-emerald-700 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-600 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {isStreaming ? "Streaming…" : spec?.root ? "Regenerate (with prior spec)" : "Generate"}
        </button>
        <button
          type="button"
          onClick={() => clear()}
          disabled={isStreaming}
          className="rounded-lg border border-zinc-600 px-4 py-2 text-sm text-zinc-300 hover:bg-zinc-800 disabled:opacity-40"
        >
          Clear
        </button>
      </div>

      <p className="text-[11px] leading-relaxed text-zinc-500">
        Set <code className="text-emerald-400/90">OPENROUTER_API_KEY</code> in{" "}
        <code className="text-zinc-400">web/.env.local</code> (defaults to OpenRouter +{" "}
        <code className="text-zinc-400">openai/gpt-oss-120b:free</code>). Or use{" "}
        <code className="text-zinc-400">OPENAI_API_KEY</code> for OpenAI or any OpenAI-compatible API. Optional:{" "}
        <code className="text-zinc-400">OPENROUTER_BASE_URL</code> /{" "}
        <code className="text-zinc-400">OPENAI_BASE_URL</code>,{" "}
        <code className="text-zinc-400">HSM_GEN_UI_MODEL</code>, OpenRouter attribution{" "}
        <code className="text-zinc-400">OPENROUTER_HTTP_REFERER</code> /{" "}
        <code className="text-zinc-400">OPENROUTER_APP_TITLE</code>.
      </p>

      {error ? (
        <div className="rounded-lg border border-red-900/50 bg-red-950/40 px-3 py-2 text-sm text-red-200">{error.message}</div>
      ) : null}

      {usage && !isStreaming ? (
        <p className="text-[10px] text-zinc-600">
          Tokens — prompt: {usage.promptTokens}, completion: {usage.completionTokens}, total: {usage.totalTokens}
        </p>
      ) : null}

      <div className="space-y-2">
        <h3 className="text-xs font-medium text-zinc-500">Live render</h3>
        <HsmGenUiView spec={spec} loading={isStreaming} />
      </div>

      {rawLines.length > 0 ? (
        <details className="text-xs text-zinc-500">
          <summary className="cursor-pointer text-zinc-400">Patch lines ({rawLines.length})</summary>
          <pre className="mt-2 max-h-40 overflow-auto rounded border border-zinc-800 bg-zinc-950 p-2 text-[10px] text-zinc-400">
            {rawLines.join("\n")}
          </pre>
        </details>
      ) : null}
    </div>
  );
}
