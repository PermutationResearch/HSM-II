/**
 * Headless check: catalog validation, JSONL patches, applySpecPatch round-trip.
 * Run: npx tsx scripts/verify-gen-ui.ts (from web/)
 */

import { applySpecPatch, type Spec } from "@json-render/core";
import { HSM_DEMO_GEN_UI_SPEC } from "../lib/gen-ui/demo-spec";
import { hsmDashboardCatalog } from "../lib/gen-ui/hsm-catalog";
import { specToJsonlPatchLines } from "../lib/gen-ui/spec-to-stream-patches";

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const validated = hsmDashboardCatalog.validate(HSM_DEMO_GEN_UI_SPEC);
if (!validated.success) {
  throw new Error(validated.error?.message ?? "catalog validate failed");
}

const lines = specToJsonlPatchLines(HSM_DEMO_GEN_UI_SPEC);
assert(lines.length > 0, "expected patch lines");

let spec: Spec = { root: "", elements: {} };
for (const line of lines) {
  const patch = JSON.parse(line) as Parameters<typeof applySpecPatch>[1];
  spec = applySpecPatch(spec, patch);
}

assert(spec.root === HSM_DEMO_GEN_UI_SPEC.root, `root mismatch: ${spec.root}`);
assert(
  Object.keys(spec.elements ?? {}).length === Object.keys(HSM_DEMO_GEN_UI_SPEC.elements ?? {}).length,
  "element count mismatch",
);

const jsonSchema = hsmDashboardCatalog.jsonSchema({ strict: true });
assert(typeof jsonSchema === "object" && jsonSchema !== null, "jsonSchema");

console.log("verify-gen-ui: ok", {
  patchLines: lines.length,
  root: spec.root,
  elementKeys: Object.keys(spec.elements ?? {}).length,
});
