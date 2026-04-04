import type { Spec } from "@json-render/core";
import type { JsonPatch } from "@json-render/core";

/** Turn a full Spec into RFC6902-style lines that {@link useUIStream} applies in order. */
export function specToJsonlPatchLines(spec: Spec): string[] {
  const patches: JsonPatch[] = [];
  const elements = spec.elements ?? {};
  for (const [key, el] of Object.entries(elements)) {
    patches.push({
      op: "add",
      path: `/elements/${key}`,
      value: el,
    });
  }
  patches.push({ op: "replace", path: "/root", value: spec.root });
  if (spec.state && Object.keys(spec.state).length > 0) {
    patches.push({ op: "replace", path: "/state", value: spec.state });
  }
  return patches.map((p) => JSON.stringify(p));
}
