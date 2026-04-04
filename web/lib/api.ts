/** Thin typed wrappers around the HSM-II Axum API (proxied via Next.js rewrites). */

const BASE = "/hsm-api";

export async function fetchJson<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { cache: "no-store" });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText} — ${path}`);
  return res.json() as Promise<T>;
}

// ── Health ────────────────────────────────────────────────────────────────────
export interface HealthResponse { status: string; version: string; }
export const getHealth = () => fetchJson<HealthResponse>("/health");

// ── World ─────────────────────────────────────────────────────────────────────
export const getWorld = () => fetchJson<Record<string, unknown>>("/world");

// ── Beliefs ───────────────────────────────────────────────────────────────────
export interface Belief {
  id: number;
  content: string;
  confidence: number;
  network: string;
  entities: string[];
  tags: string[];
  timestamp: number;
}
export const getBeliefs = () => fetchJson<Belief[]>("/beliefs");

// ── Honcho ────────────────────────────────────────────────────────────────────
export interface UserRepresentation {
  peer_id: string;
  communication_style: string;
  goals: { description: string; confidence: number; first_seen_session: number }[];
  frustrations: { description: string; confidence: number }[];
  preferences: { key: string; value: string; confidence: number }[];
  traits: { label: string; evidence: string; confidence: number }[];
  last_updated: number;
  session_count: number;
  total_messages: number;
  confidence: number;
}

export interface HonchoMemoryStats {
  entity_summaries: number;
  total_entries: number;
  beliefs: number;
  experiences: number;
}

export interface PackedContext {
  peer_representation: string | null;
  entity_summaries: string[];
  messages: { role: string; content: string; token_estimate: number }[];
  conclusions: string[];
  token_count: number;
  budget: number;
}

export const getHonchoMemoryStats = () =>
  fetchJson<HonchoMemoryStats>("/honcho/memory");

export const getHonchopeers = () =>
  fetchJson<{ peers: UserRepresentation[]; total: number }>("/honcho/peers");

export const getHonchoPeer = (id: string) =>
  fetchJson<UserRepresentation>(`/honcho/peers/${encodeURIComponent(id)}`);

export const getPackedContext = (id: string, budget = 4096) =>
  fetchJson<PackedContext>(
    `/honcho/peers/${encodeURIComponent(id)}/context?budget=${budget}`
  );

// ── Council ───────────────────────────────────────────────────────────────────
export const getCouncilDecisions = () =>
  fetchJson<unknown[]>("/council/decisions");
