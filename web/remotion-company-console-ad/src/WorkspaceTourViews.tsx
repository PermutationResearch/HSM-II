import React from "react";
import { interpolate } from "remotion";
import type { AdWorkspacePage, TourPageStart } from "./tourTimeline";

/** Mirrors company-console Nothing-style tokens (`globals.css` / sidebar). */
export const ND = {
  surface: "#111111",
  raised: "#1a1a1a",
  border: "#222222",
  borderV: "#333333",
  text: "#e8e8e8",
  muted: "#999999",
  dim: "#666666",
  accent: "#d71921",
  blue: "#5b9bf6",
  green: "#4a9e5c",
  warn: "#d4a843",
} as const;

export const mono =
  'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace';

export type { AdWorkspacePage } from "./tourTimeline";

export function navIndexForPage(p: AdWorkspacePage): number | null {
  switch (p) {
    case "dashboard":
      return 0;
    case "agents":
      return 1;
    case "issues":
      return 2;
    case "marketplace":
      return 3;
    case "playbooks":
      return 4;
    case "intelligence":
      return 5;
    default:
      return null;
  }
}

/** Breadcrumb leaf — matches `ConsoleAppShell` labels. */
export function workspaceBreadcrumbPage(p: AdWorkspacePage): string {
  switch (p) {
    case "dashboard":
      return "Dashboard";
    case "agents":
      return "Agents";
    case "issues":
      return "Issues";
    case "marketplace":
      return "Marketplace";
    case "playbooks":
      return "Playbooks";
    case "intelligence":
      return "Intelligence";
    case "command":
      return "Issues";
    default:
      return "Dashboard";
  }
}

export function pageHeader(
  p: AdWorkspacePage
): { title: string; subtitle: string } {
  switch (p) {
    case "dashboard":
      return {
        title: "Dashboard",
        subtitle: "Live ops · tasks · spend — same grid as the workspace dashboard",
      };
    case "agents":
      return {
        title: "Agents",
        subtitle: "Personas · checkouts · workforce roster · agent channels",
      };
    case "issues":
      return {
        title: "Issues",
        subtitle: "Task graph · checkout · SLA · requires_human · context_notes",
      };
    case "marketplace":
      return {
        title: "Marketplace",
        subtitle: "Browse packs · seed a workspace",
      };
    case "playbooks":
      return {
        title: "Playbooks",
        subtitle: "SOPs & visions → executable tasks",
      };
    case "intelligence":
      return {
        title: "Intelligence",
        subtitle: "Summary · goals · signals — Postgres-backed",
      };
    case "command":
      return {
        title: "Issues",
        subtitle: "Command palette — ⌘K in the real console",
      };
    default:
      return { title: "", subtitle: "" };
  }
}

function PageTitleBlock({ page }: { page: AdWorkspacePage }) {
  if (page === "command") return null;
  const h = pageHeader(page);
  return (
    <div style={{ marginBottom: 20 }}>
      <div
        style={{
          fontFamily: mono,
          fontSize: 11,
          fontWeight: 400,
          letterSpacing: "0.08em",
          textTransform: "uppercase",
          color: ND.dim,
        }}
      >
        Workspace
      </div>
      <div
        style={{
          marginTop: 8,
          fontSize: 24,
          fontWeight: 500,
          letterSpacing: "-0.02em",
          color: "#ffffff",
          fontFamily: "system-ui, -apple-system, Segoe UI, sans-serif",
        }}
      >
        {h.title}
      </div>
      <div
        style={{
          marginTop: 8,
          maxWidth: 560,
          fontSize: 14,
          lineHeight: 1.5,
          color: ND.muted,
        }}
      >
        {h.subtitle}
      </div>
    </div>
  );
}

function pageFade(
  frame: number,
  page: AdWorkspacePage,
  pageStart: TourPageStart
): number {
  const s = pageStart[page];
  return interpolate(frame, [s, s + 10], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
}

function MetricCard({
  label,
  value,
  hint,
  accent,
}: {
  label: string;
  value: string;
  hint: string;
  accent?: string;
}) {
  return (
    <div
      style={{
        flex: 1,
        minWidth: 0,
        padding: "12px 14px",
        borderRadius: 8,
        background: ND.raised,
        border: `1px solid ${ND.border}`,
      }}
    >
      <div
        style={{
          fontFamily: mono,
          fontSize: 10,
          letterSpacing: "0.08em",
          textTransform: "uppercase",
          color: ND.dim,
        }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: 22,
          fontWeight: 600,
          color: ND.text,
          marginTop: 6,
          letterSpacing: "-0.02em",
        }}
      >
        {value}
      </div>
      <div style={{ fontSize: 11, color: ND.muted, marginTop: 4 }}>{hint}</div>
      {accent ? (
        <div
          style={{
            marginTop: 8,
            height: 3,
            borderRadius: 2,
            background: accent,
            opacity: 0.85,
          }}
        />
      ) : null}
    </div>
  );
}

function Row({
  children,
  dim,
}: {
  children: React.ReactNode;
  dim?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 12px",
        borderRadius: 6,
        border: `1px solid ${ND.border}`,
        background: dim ? "rgba(0,0,0,0.35)" : ND.raised,
        marginBottom: 6,
        opacity: dim ? 0.55 : 1,
      }}
    >
      {children}
    </div>
  );
}

export const WorkspaceTourMainPane: React.FC<{
  page: AdWorkspacePage;
  frame: number;
  pageStart: TourPageStart;
}> = ({ page, frame, pageStart }) => {
  const fade = pageFade(frame, page, pageStart);
  const inner = (() => {
    switch (page) {
      case "dashboard":
        return (
          <>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                letterSpacing: "0.08em",
                textTransform: "uppercase",
                color: ND.dim,
                marginBottom: 14,
              }}
            >
              Company · Apex Labs · UUID…
            </div>
            <div
              style={{
                marginBottom: 14,
                padding: "10px 12px",
                borderRadius: 8,
                border: `1px solid ${ND.border}`,
                background: ND.raised,
              }}
            >
              <div
                style={{
                  fontFamily: mono,
                  fontSize: 9,
                  letterSpacing: "0.12em",
                  textTransform: "uppercase",
                  color: ND.dim,
                  marginBottom: 8,
                }}
              >
                Active agents
              </div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                {[
                  { id: "code-agent", live: 2 },
                  { id: "research-agent", live: 0 },
                  { id: "governance-agent", live: 1 },
                  { id: "operator-agent", live: 0 },
                ].map((a) => (
                  <div
                    key={a.id}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 6,
                      padding: "6px 10px",
                      borderRadius: 999,
                      border: `1px solid ${ND.borderV}`,
                      background: ND.surface,
                      fontFamily: mono,
                      fontSize: 11,
                      color: ND.text,
                    }}
                  >
                    <span style={{ opacity: 0.75 }} aria-hidden>
                      ◆
                    </span>
                    {a.id}
                    {a.live > 0 ? (
                      <span
                        style={{
                          fontSize: 9,
                          fontWeight: 700,
                          padding: "2px 6px",
                          borderRadius: 6,
                          color: ND.green,
                          border: "1px solid rgba(74,158,92,0.45)",
                          background: "rgba(74,158,92,0.12)",
                        }}
                      >
                        ● {a.live} live
                      </span>
                    ) : (
                      <span style={{ fontSize: 9, color: ND.muted }}>idle</span>
                    )}
                  </div>
                ))}
              </div>
            </div>
            <div style={{ display: "flex", gap: 10, marginBottom: 14 }}>
              <MetricCard
                label="Open tasks"
                value="38"
                hint="12 in progress"
                accent={ND.blue}
              />
              <MetricCard
                label="Inbox"
                value="3"
                hint="Needs you"
                accent={ND.accent}
              />
              <MetricCard
                label="Live agents"
                value="5"
                hint="Checkouts + runs"
                accent={ND.green}
              />
              <MetricCard
                label="Spend (30d)"
                value="$184"
                hint="LLM + tools"
                accent={ND.warn}
              />
            </div>
            <div
              style={{
                padding: 12,
                borderRadius: 8,
                border: `1px solid ${ND.border}`,
                background: ND.surface,
                marginBottom: 12,
              }}
            >
              <div
                style={{
                  fontFamily: mono,
                  fontSize: 10,
                  color: ND.dim,
                  marginBottom: 10,
                  textTransform: "uppercase",
                  letterSpacing: "0.08em",
                }}
              >
                Run activity
              </div>
              <div style={{ display: "flex", alignItems: "flex-end", gap: 4, height: 52 }}>
                {[40, 65, 35, 80, 55, 90, 48, 72, 60].map((h, i) => (
                  <div
                    key={i}
                    style={{
                      flex: 1,
                      height: `${h}%`,
                      background: `linear-gradient(180deg, ${ND.blue}88, ${ND.blue}33)`,
                      borderRadius: 3,
                    }}
                  />
                ))}
              </div>
            </div>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                color: ND.dim,
                marginBottom: 8,
                textTransform: "uppercase",
                letterSpacing: "0.08em",
              }}
            >
              Recent activity
            </div>
            <Row>
              <span style={{ color: ND.green, fontSize: 12 }}>●</span>
              <div style={{ flex: 1 }}>
                <div style={{ color: ND.text, fontSize: 13 }}>Task HSM-142 checked out</div>
                <div style={{ color: ND.muted, fontSize: 11 }}>code-agent · 2m ago</div>
              </div>
            </Row>
            <Row>
              <span style={{ color: ND.warn, fontSize: 12 }}>◆</span>
              <div style={{ flex: 1 }}>
                <div style={{ color: ND.text, fontSize: 13 }}>Inbox: policy approval pending</div>
                <div style={{ color: ND.muted, fontSize: 11 }}>governance · 14m ago</div>
              </div>
            </Row>
            <Row>
              <span style={{ color: ND.blue, fontSize: 12 }}>◇</span>
              <div style={{ flex: 1 }}>
                <div style={{ color: ND.text, fontSize: 13 }}>Memory entry indexed (FTS)</div>
                <div style={{ color: ND.muted, fontSize: 11 }}>company_memory · 1h ago</div>
              </div>
            </Row>
          </>
        );
      case "agents":
        return (
          <>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                letterSpacing: "0.08em",
                textTransform: "uppercase",
                color: ND.dim,
                marginBottom: 12,
              }}
            >
              Workforce · personas · registry
            </div>
            {[
              { id: "code-agent", role: "Engineering", live: 2 },
              { id: "research-agent", role: "Research", live: 0 },
              { id: "governance-agent", role: "Policy", live: 1 },
              { id: "operator-agent", role: "Ops", live: 0 },
            ].map((a) => (
              <Row key={a.id}>
                <span style={{ color: ND.blue, fontSize: 14 }} aria-hidden>
                  ◆
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ color: ND.text, fontSize: 14, fontWeight: 600 }}>{a.id}</div>
                  <div style={{ color: ND.muted, fontSize: 11, marginTop: 3 }}>{a.role}</div>
                </div>
                {a.live > 0 ? (
                  <span
                    style={{
                      fontFamily: mono,
                      fontSize: 9,
                      fontWeight: 700,
                      padding: "4px 8px",
                      borderRadius: 4,
                      textTransform: "uppercase",
                      color: ND.green,
                      border: "1px solid rgba(74,158,92,0.45)",
                      background: "rgba(74,158,92,0.12)",
                    }}
                  >
                    ● {a.live} live
                  </span>
                ) : (
                  <span style={{ fontFamily: mono, fontSize: 10, color: ND.dim }}>idle</span>
                )}
              </Row>
            ))}
          </>
        );
      case "issues":
        return (
          <>
            <div
              style={{
                display: "flex",
                gap: 8,
                marginBottom: 14,
                fontFamily: mono,
                fontSize: 10,
                textTransform: "uppercase",
                letterSpacing: "0.06em",
                color: ND.dim,
              }}
            >
              {["All", "Open", "In progress", "Blocked"].map((t, i) => (
                <span
                  key={t}
                  style={{
                    padding: "6px 10px",
                    borderRadius: 6,
                    border: `1px solid ${ND.borderV}`,
                    background: i === 2 ? ND.raised : "transparent",
                    color: i === 2 ? ND.text : ND.muted,
                  }}
                >
                  {t}
                </span>
              ))}
            </div>
            {[
              {
                id: "HSM-142",
                t: "Ship Q2 llm-context pack for sales pod",
                st: "in_progress",
                sub: "code-agent · checkout · capability_refs",
              },
              {
                id: "HSM-141",
                t: "YC-bench medium preset · pack grid",
                st: "open",
                sub: "research-agent · SLA Fri",
              },
              {
                id: "HSM-138",
                t: "Sync Paperclip goals → Postgres",
                st: "blocked",
                sub: "requires_human · inbox",
              },
            ].map((x) => (
              <Row key={x.id} dim={x.st === "blocked"}>
                <span style={{ fontFamily: mono, fontSize: 11, color: ND.blue }}>{x.id}</span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ color: ND.text, fontSize: 14, fontWeight: 600 }}>{x.t}</div>
                  <div style={{ color: ND.muted, fontSize: 11, marginTop: 3 }}>{x.sub}</div>
                </div>
                <span
                  style={{
                    fontFamily: mono,
                    fontSize: 9,
                    padding: "4px 8px",
                    borderRadius: 4,
                    textTransform: "uppercase",
                    border:
                      x.st === "blocked"
                        ? `1px solid ${ND.accent}55`
                        : `1px solid ${ND.borderV}`,
                    color:
                      x.st === "in_progress"
                        ? ND.green
                        : x.st === "blocked"
                          ? ND.accent
                          : ND.muted,
                  }}
                >
                  {x.st.replace("_", " ")}
                </span>
              </Row>
            ))}
          </>
        );
      case "marketplace":
        return (
          <>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                color: ND.dim,
                marginBottom: 12,
                textTransform: "uppercase",
                letterSpacing: "0.08em",
              }}
            >
              companies.sh · install into Company OS
            </div>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: 10,
              }}
            >
              {[
                { n: "hsm_market_growth", d: "GTM + content agents" },
                { n: "paperclip_default", d: "Goals + DRI templates" },
                { n: "apex_ops", d: "Incident + SRE playbooks" },
                { n: "bench_profile", d: "YC-bench ready profile" },
              ].map((p) => (
                <div
                  key={p.n}
                  style={{
                    padding: 14,
                    borderRadius: 10,
                    border: `1px solid ${ND.border}`,
                    background: ND.raised,
                  }}
                >
                  <div style={{ fontFamily: mono, fontSize: 11, color: ND.blue }}>{p.n}</div>
                  <div style={{ color: ND.muted, fontSize: 12, marginTop: 6 }}>{p.d}</div>
                  <div
                    style={{
                      marginTop: 10,
                      fontFamily: mono,
                      fontSize: 10,
                      textTransform: "uppercase",
                      letterSpacing: "0.08em",
                      color: ND.text,
                      border: `1px solid ${ND.borderV}`,
                      padding: "6px 0",
                      textAlign: "center",
                      borderRadius: 6,
                    }}
                  >
                    Add workspace
                  </div>
                </div>
              ))}
            </div>
          </>
        );
      case "playbooks":
        return (
          <>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                color: ND.dim,
                marginBottom: 12,
                textTransform: "uppercase",
                letterSpacing: "0.08em",
              }}
            >
              SOP composer · attach to project / task
            </div>
            {[
              "Incident response — P0",
              "Customer refund — dual control",
              "Launch checklist — GTM",
              "On-call handoff",
            ].map((title) => (
              <Row key={title}>
                <span style={{ color: ND.warn, fontSize: 12 }}>▸</span>
                <div style={{ flex: 1 }}>
                  <div style={{ color: ND.text, fontSize: 14, fontWeight: 600 }}>{title}</div>
                  <div style={{ color: ND.muted, fontSize: 11, marginTop: 3 }}>
                    Implement as tasks · vision + steps
                  </div>
                </div>
                <span style={{ fontFamily: mono, fontSize: 9, color: ND.blue }}>OPEN</span>
              </Row>
            ))}
          </>
        );
      case "intelligence":
        return (
          <>
            <div
              style={{
                padding: 14,
                borderRadius: 8,
                border: `1px solid ${ND.border}`,
                background: ND.surface,
                marginBottom: 12,
              }}
            >
              <div
                style={{
                  fontFamily: mono,
                  fontSize: 10,
                  color: ND.dim,
                  textTransform: "uppercase",
                  letterSpacing: "0.08em",
                  marginBottom: 10,
                }}
              >
                GET /api/company/…/intelligence/summary
              </div>
              <ul
                style={{
                  margin: 0,
                  paddingLeft: 18,
                  color: ND.text,
                  fontSize: 13,
                  lineHeight: 1.55,
                }}
              >
                <li>Goals synced from Paperclip layer · 6 active</li>
                <li>DRIs registered · 4 · escalation paths wired</li>
                <li>Last signal: task graph coherence ↑ (checkout health)</li>
                <li>Recommended: review inbox + bench profile this week</li>
              </ul>
            </div>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                color: ND.muted,
                textTransform: "uppercase",
                letterSpacing: "0.06em",
              }}
            >
              Postgres is source of truth · not parallel global state
            </div>
          </>
        );
      case "command":
        return (
          <>
            <div
              style={{
                fontFamily: mono,
                fontSize: 10,
                color: ND.dim,
                marginBottom: 10,
                textTransform: "uppercase",
                letterSpacing: "0.08em",
              }}
            >
              Same task graph — drive it with natural language
            </div>
            <Row dim>
              <span style={{ fontFamily: mono, fontSize: 11, color: ND.blue }}>HSM-142</span>
              <div style={{ flex: 1 }}>
                <div style={{ color: ND.text, fontSize: 13 }}>Ship Q2 llm-context pack…</div>
                <div style={{ color: ND.muted, fontSize: 10, marginTop: 3 }}>
                  Background while you type the command below
                </div>
              </div>
            </Row>
            <p style={{ color: ND.muted, fontSize: 12, lineHeight: 1.45, marginTop: 8 }}>
              Dashboard, agents, issues, marketplace, playbooks, and intelligence share one{" "}
              <strong style={{ color: ND.text }}>company_id</strong> graph — the same routes as{" "}
              <span style={{ fontFamily: mono, fontSize: 11 }}>/workspace/*</span> in Company console.
            </p>
          </>
        );
      default:
        return null;
    }
  })();

  return (
    <div style={{ opacity: fade, height: "100%" }}>
      <PageTitleBlock page={page} />
      {inner}
    </div>
  );
};
