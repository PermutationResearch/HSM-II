import type { SopExampleDocument } from "./sop-examples-types";

function key(companyId: string): string {
  return `hsm.ii.custom_sops.v1:${companyId}`;
}

export function loadCustomSops(companyId: string): SopExampleDocument[] {
  if (typeof window === "undefined" || !companyId) return [];
  try {
    const raw = localStorage.getItem(key(companyId));
    if (!raw) return [];
    const j = JSON.parse(raw) as unknown;
    if (!Array.isArray(j)) return [];
    return j.filter(
      (x): x is SopExampleDocument =>
        x !== null &&
        typeof x === "object" &&
        (x as SopExampleDocument).kind === "hsm.sop_reference.v1"
    );
  } catch {
    return [];
  }
}

export function saveCustomSops(companyId: string, docs: SopExampleDocument[]): void {
  if (typeof window === "undefined" || !companyId) return;
  localStorage.setItem(key(companyId), JSON.stringify(docs));
}

export function upsertCustomSop(companyId: string, doc: SopExampleDocument): SopExampleDocument[] {
  const list = loadCustomSops(companyId);
  const idx = list.findIndex((d) => d.id === doc.id);
  if (idx >= 0) list[idx] = doc;
  else list.push(doc);
  saveCustomSops(companyId, list);
  return list;
}

export function removeCustomSop(companyId: string, id: string): SopExampleDocument[] {
  const list = loadCustomSops(companyId).filter((d) => d.id !== id);
  saveCustomSops(companyId, list);
  return list;
}
