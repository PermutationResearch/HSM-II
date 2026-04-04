import { PrettyJson } from "@/components/PrettyJson";

export const revalidate = 0;

type ArchitectureApiResponse = {
  blueprint?: unknown;
  runtime?: unknown;
};

function apiBase(): string {
  return (process.env.HSM_API_URL ?? "http://127.0.0.1:8080").replace(/\/$/, "");
}

export default async function ArchitecturePage() {
  let data: ArchitectureApiResponse | null = null;
  let err: string | null = null;
  try {
    const res = await fetch(`${apiBase()}/api/architecture`, { cache: "no-store" });
    if (!res.ok) {
      err = `HTTP ${res.status}`;
    } else {
      data = (await res.json()) as ArchitectureApiResponse;
    }
  } catch (e) {
    err = e instanceof Error ? e.message : String(e);
  }

  return (
    <div className="space-y-8 max-w-4xl">
      <div>
        <h1 className="text-xl font-semibold text-zinc-100">Architecture</h1>
        <p className="mt-2 text-sm text-zinc-400 leading-relaxed">
          Live JSON from <code className="text-emerald-400/90">GET /api/architecture</code> on{" "}
          <code className="text-zinc-500">{apiBase()}</code> (set{" "}
          <code className="text-zinc-500">HSM_API_URL</code> for Next server-side fetch). Canonical RON:{" "}
          <code className="text-zinc-500">architecture/hsm-ii-blueprint.ron</code>. Full generated Markdown (sync-checked
          in CI): repo root <code className="text-zinc-500">ARCHITECTURE.generated.md</code>; curated notes:{" "}
          <code className="text-zinc-500">ARCHITECTURE.md</code>.
        </p>
      </div>

      {err ? (
        <div className="rounded-lg border border-amber-900/50 bg-amber-950/30 px-4 py-3 text-sm text-amber-100">
          Could not load architecture: {err}. Start the API (e.g. <code className="text-amber-200/90">hsm_api</code>) or set{" "}
          <code className="text-amber-200/90">HSM_API_URL</code>.
        </div>
      ) : null}

      {data?.runtime != null ? (
        <section className="space-y-2">
          <h2 className="text-sm font-medium text-zinc-500">Runtime overlay</h2>
          <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-4 overflow-auto max-h-[200px]">
            <PrettyJson value={data.runtime} />
          </div>
        </section>
      ) : !err ? (
        <p className="text-sm text-zinc-500">No mounted world — runtime is null (blueprint still valid).</p>
      ) : null}

      {data?.blueprint != null ? (
        <section className="space-y-2">
          <h2 className="text-sm font-medium text-zinc-500">Blueprint (JSON)</h2>
          <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-4 overflow-auto max-h-[min(70vh,720px)]">
            <PrettyJson value={data.blueprint} />
          </div>
        </section>
      ) : null}
    </div>
  );
}
