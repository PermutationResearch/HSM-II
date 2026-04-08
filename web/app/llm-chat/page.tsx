"use client";

import { useCallback, useState } from "react";
import { streamLlmChat } from "@/lib/llm-chat-stream";

export default function LlmChatStreamPage() {
  const [input, setInput] = useState("Explain stigmergy in one short paragraph.");
  const [out, setOut] = useState("");
  const [meta, setMeta] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const run = useCallback(async () => {
    setBusy(true);
    setErr(null);
    setOut("");
    setMeta(null);
    try {
      await streamLlmChat(
        [
          { role: "system", content: "You are a concise assistant." },
          { role: "user", content: input.trim() || "Hello" },
        ],
        {
          onDelta: (t) => setOut((p) => p + t),
          onDone: (ev) =>
            setMeta(
              `done — model: ${ev.model || "?"}` +
                (ev.provider ? ` · provider: ${ev.provider}` : "")
            ),
          onError: (m) => setErr(m),
        }
      );
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [input]);

  return (
    <div className="max-w-3xl space-y-4">
      <h1 className="text-xl font-semibold text-zinc-100">LLM streaming</h1>
      <p className="text-sm text-zinc-500">
        Requires <code className="text-zinc-400">cargo run --bin hsm-api</code> (default port{" "}
        <code className="text-zinc-400">3000</code>, matches <code className="text-zinc-400">web/next.config.ts</code>{" "}
        rewrite) or set <code className="text-zinc-400">HSM_API_URL</code>. POST{" "}
        <code className="text-zinc-400">/hsm-api/llm/chat/stream</code> → SSE{" "}
        <code className="text-zinc-400">data: {"{...}"}</code>. With Ollama only, set{" "}
        <code className="text-zinc-400">HSM_LLM_PROVIDER_ORDER=ollama</code> and{" "}
        <code className="text-zinc-400">DEFAULT_LLM_MODEL</code> to an installed tag (e.g.{" "}
        <code className="text-zinc-400">llama3.2</code>).
      </p>
      <textarea
        className="w-full min-h-[100px] rounded border border-zinc-800 bg-zinc-900 p-3 text-sm text-zinc-200"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        disabled={busy}
      />
      <button
        type="button"
        onClick={run}
        disabled={busy}
        className="rounded bg-emerald-700 px-4 py-2 text-sm font-medium text-white hover:bg-emerald-600 disabled:opacity-50"
      >
        {busy ? "Streaming…" : "Stream reply"}
      </button>
      {err && <p className="text-sm text-red-400">{err}</p>}
      {meta && <p className="text-xs text-zinc-500">{meta}</p>}
      <pre className="whitespace-pre-wrap rounded border border-zinc-800 bg-zinc-900/80 p-4 text-sm text-zinc-200 min-h-[120px]">
        {out || (busy ? "…" : "")}
      </pre>
    </div>
  );
}
