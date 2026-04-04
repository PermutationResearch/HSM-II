import type { Spec } from "@json-render/core";
import { PrettyJson } from "@/components/PrettyJson";
import { HsmGenUiView } from "@/components/gen-ui/HsmGenUiView";
import { GenUiStreamClient } from "@/app/gen-ui/GenUiStreamClient";
import { HSM_DEMO_GEN_UI_SPEC } from "@/lib/gen-ui/demo-spec";
import { hsmDashboardCatalog } from "@/lib/gen-ui/hsm-catalog";

export const revalidate = 0;

export default function GenUiPage() {
  const validated = hsmDashboardCatalog.validate(HSM_DEMO_GEN_UI_SPEC);
  const specForRender: Spec | null = validated.success
    ? ((validated.data ?? HSM_DEMO_GEN_UI_SPEC) as Spec)
    : null;

  return (
    <div className="space-y-8 max-w-4xl">
      <div>
        <h1 className="text-xl font-semibold text-zinc-100">Generative UI (json-render)</h1>
        <p className="mt-2 text-sm text-zinc-400 leading-relaxed">
          <strong className="text-zinc-300">Renderer</strong> below uses a{" "}
          <span className="text-emerald-400">model-shaped Spec</span> (root + elements map). For raw HSM API
          responses, use <a href="/memory" className="text-sky-400 hover:underline">Memory</a> /{" "}
          <a href="/council" className="text-sky-400 hover:underline">Council</a> with{" "}
          <code className="text-emerald-400/90">PrettyJson</code>.
        </p>
      </div>

      {!validated.success && (
        <div className="rounded-lg border border-red-900/50 bg-red-950/30 px-4 py-3 text-sm text-red-200">
          Demo spec validation failed (should not happen): {validated.error?.message ?? "unknown"}
        </div>
      )}

      <section className="space-y-2">
        <h2 className="text-sm font-medium text-zinc-500">Streamed generation</h2>
        <p className="text-sm text-zinc-500 leading-relaxed">
          POST <code className="text-emerald-400/90">/api/gen-ui/stream</code> uses{" "}
          <code className="text-zinc-400">hsmGenUiSystemMessage</code> /{" "}
          <code className="text-zinc-400">hsmGenUiUserMessage</code>, validates with the catalog, then streams JSON
          Patch lines for <code className="text-zinc-400">useUIStream</code>.
        </p>
        <GenUiStreamClient />
      </section>

      <section className="space-y-2">
        <h2 className="text-sm font-medium text-zinc-500">Rendered spec</h2>
        <HsmGenUiView spec={specForRender} />
      </section>

      <section className="space-y-2">
        <h2 className="text-sm font-medium text-zinc-500">Spec JSON (debug)</h2>
        <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-4 overflow-auto max-h-[480px]">
          <PrettyJson value={HSM_DEMO_GEN_UI_SPEC} />
        </div>
      </section>
    </div>
  );
}
