import React, { useMemo } from "react";
import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { loadFont } from "@remotion/google-fonts/Fraunces";
import { loadFont as loadInter } from "@remotion/google-fonts/Inter";
import {
  mono,
  navIndexForPage,
  ND,
  workspaceBreadcrumbPage,
  WorkspaceTourMainPane,
} from "./WorkspaceTourViews";
import {
  adWorkspacePage,
  computeTour,
  cursorAlongTourPath,
  cursorHoverAgentIndex,
  cursorHoverNavIndex,
  estimatePanelSize,
  TOUR_PANEL_MAX_W,
  TOUR_RAIL_W,
  TOUR_SHELL_HEADER_H,
  TOUR_SIDEBAR_W,
} from "./tourTimeline";
import { LogoOutroCanvas } from "./LogoOutro";
import {
  OUTRO_CONSOLE_FADE_END,
  OUTRO_CONSOLE_FADE_START,
  OUTRO_LOGO_FADE_END,
  OUTRO_LOGO_FADE_START,
  TOP_BAND_FRACTION,
} from "./outroConstants";

/* Bold high-contrast editorial serif (Fraunces) + neutral UI sans — Arc/Linear-style hero type */
const { fontFamily: displaySerif } = loadFont("normal", {
  weights: ["500", "600", "700", "800"],
  subsets: ["latin", "latin-ext"],
});

const { fontFamily: uiSans } = loadInter("normal", {
  weights: ["400", "500", "600"],
  subsets: ["latin"],
});

export type CompanyConsoleAdProps = {
  headline: string;
  subline: string;
  prompt: string;
};

const CREAM = "#f2ebe3";
const COSMIC_DEEP = "#0d0a14";
/** Workspace root — matches `ConsoleAppShell` / `.nd-ws-main`. */
const PANEL = "#000000";
const PEACH_GLOW = "rgba(201, 168, 130, 0.32)";
const MUTED = "#8a8494";
const LIVE_GREEN = "#4A9E5C";
const WARN_AMBER = "#D4A843";

/** First six items of `ConsoleAppShell` `nav` — same labels & order. */
const NAV: { label: string; icon: string }[] = [
  { label: "Dashboard", icon: "▣" },
  { label: "Agents", icon: "◆" },
  { label: "Issues", icon: "☰" },
  { label: "Marketplace", icon: "⬡" },
  { label: "Playbooks", icon: "≡" },
  { label: "Intelligence", icon: "◇" },
];

/** Right rail agent list — mirrors `WorkspaceRightRail` personas row. */
const RAIL_AGENTS: { id: string; label: string; live: number }[] = [
  { id: "code-agent", label: "code-agent", live: 2 },
  { id: "research-agent", label: "research-agent", live: 0 },
  { id: "governance-agent", label: "governance-agent", live: 1 },
  { id: "operator-agent", label: "operator-agent", live: 0 },
];

/** Click pulse peaks on the same frames as `computeTour().clickFrames`. */
function clickPulse(frame: number, stops: number[]): number {
  let max = 0;
  for (const s of stops) {
    const dt = frame - s;
    if (dt >= 0 && dt < 10) {
      const v = interpolate(dt, [0, 1, 10], [1, 0.55, 0], {
        extrapolateLeft: "clamp",
        extrapolateRight: "clamp",
        easing: Easing.out(Easing.quad),
      });
      max = Math.max(max, v);
    }
  }
  return max;
}

const WorkspaceCursor: React.FC<{
  x: number;
  y: number;
  pulse: number;
}> = ({ x, y, pulse }) => {
  const tipX = 5;
  const tipY = 3.5;
  return (
    <div
      style={{
        position: "absolute",
        left: `${x}%`,
        top: `${y}%`,
        zIndex: 60,
        pointerEvents: "none",
        transform: `translate(calc(-1px * ${tipX}), calc(-1px * ${tipY})) scale(${1 - pulse * 0.14})`,
        filter:
          "drop-shadow(0 2px 4px rgba(0,0,0,0.45)) drop-shadow(0 8px 20px rgba(0,0,0,0.35))",
      }}
    >
      {pulse > 0.12 ? (
        <>
          <div
            style={{
              position: "absolute",
              left: -10,
              top: -10,
              width: 48,
              height: 48,
              borderRadius: "50%",
              background: `radial-gradient(circle, rgba(201,168,130,${0.22 * pulse}) 0%, transparent 70%)`,
              transform: `scale(${1 + pulse * 0.5})`,
              opacity: pulse * 0.9,
            }}
          />
          <div
            style={{
              position: "absolute",
              left: -6,
              top: -6,
              width: 40,
              height: 40,
              borderRadius: "50%",
              border: `2px solid rgba(255,255,255,${0.35 * pulse})`,
              boxShadow: `0 0 0 1px rgba(201,168,130,${0.5 * pulse}), 0 0 24px rgba(201,168,130,${0.25 * pulse})`,
              transform: `scale(${1 + pulse * 0.28})`,
              opacity: pulse * 0.88,
            }}
          />
        </>
      ) : null}
      <svg width="36" height="36" viewBox="0 0 36 36" fill="none" aria-hidden>
        <defs>
          <linearGradient id="cursorGrad" x1="6" y1="4" x2="28" y2="30" gradientUnits="userSpaceOnUse">
            <stop stopColor="#ffffff" />
            <stop offset="1" stopColor="#f0eaef" />
          </linearGradient>
        </defs>
        <path
          d="M5 3.5L5 28L12.5 20.5L17.5 30L21.5 27.5L16.5 16L27 14L5 3.5Z"
          fill="url(#cursorGrad)"
          stroke="#141018"
          strokeWidth="1.35"
          strokeLinejoin="round"
        />
        <path
          d="M8 8L8 22L12.5 17.5L16 24L17.5 23L14 14.5L21 13L8 8Z"
          fill="rgba(255,255,255,0.35)"
        />
      </svg>
    </div>
  );
};

function badgeStyles(
  variant: "live" | "warn" | "neutral" | undefined
): React.CSSProperties {
  switch (variant) {
    case "live":
      return {
        color: LIVE_GREEN,
        border: "1px solid rgba(74,158,92,0.45)",
        background: "rgba(74,158,92,0.12)",
      };
    case "warn":
      return {
        color: WARN_AMBER,
        border: "1px solid rgba(212,168,67,0.45)",
        background: "rgba(212,168,67,0.1)",
      };
    default:
      return {
        color: MUTED,
        border: "1px solid rgba(255,255,255,0.1)",
        background: "rgba(255,255,255,0.04)",
      };
  }
}

const WarpStars: React.FC = () => {
  const frame = useCurrentFrame();
  const drift = frame * 1.8;
  const streaks = useMemo(
    () =>
      Array.from({ length: 28 }, (_, i) => ({
        id: i,
        left: (i * 37 + (i % 7) * 13) % 100,
        height: 40 + (i * 17) % 120,
        delay: i * 2.3,
        opacity: 0.15 + (i % 5) * 0.08,
      })),
    []
  );

  return (
    <AbsoluteFill
      style={{
        background: `radial-gradient(ellipse 80% 60% at 50% 35%, ${PEACH_GLOW} 0%, transparent 55%), linear-gradient(165deg, #1a1228 0%, ${COSMIC_DEEP} 45%, #0a0812 100%)`,
        overflow: "hidden",
      }}
    >
      {streaks.map((s) => (
        <div
          key={s.id}
          style={{
            position: "absolute",
            left: `${s.left}%`,
            top: "-20%",
            width: 2,
            height: `${s.height}%`,
            background: `linear-gradient(180deg, transparent, rgba(200,190,255,${s.opacity}), transparent)`,
            transform: `translateY(${(drift + s.delay) % 140}vh) rotate(12deg)`,
            filter: "blur(0.5px)",
          }}
        />
      ))}
      <div
        style={{
          position: "absolute",
          inset: 0,
          backgroundImage:
            "radial-gradient(1px 1px at 20% 30%, rgba(255,255,255,0.4) 50%, transparent 51%), radial-gradient(1px 1px at 60% 70%, rgba(255,255,255,0.25) 50%, transparent 51%), radial-gradient(1px 1px at 80% 20%, rgba(255,255,255,0.3) 50%, transparent 51%)",
          backgroundSize: "200px 200px, 320px 320px, 180px 180px",
          opacity: 0.35,
          transform: `translateY(${frame * 0.15}px)`,
        }}
      />
    </AbsoluteFill>
  );
};

export const CompanyConsoleAd: React.FC<CompanyConsoleAdProps> = ({
  headline,
  subline,
  prompt,
}) => {
  const frame = useCurrentFrame();
  const { fps, width: compW, height: compH } = useVideoConfig();
  const { panelW, panelH } = useMemo(
    () => estimatePanelSize(compW, compH),
    [compW, compH]
  );
  const tour = useMemo(() => computeTour(panelW, panelH), [panelW, panelH]);

  const windowEnter = spring({
    frame,
    fps,
    config: { damping: 18, mass: 0.9 },
    from: 0,
    to: 1,
    durationInFrames: 28,
  });

  const windowY = interpolate(windowEnter, [0, 1], [80, 0]);
  const windowOpacity = interpolate(windowEnter, [0, 1], [0, 1]);

  const charsPerFrame = 2.65;
  const typingStart = tour.typingStart;
  const typedLen = Math.max(
    0,
    Math.min(
      prompt.length,
      Math.floor((frame - typingStart) * charsPerFrame)
    )
  );
  const typed = prompt.slice(0, typedLen);
  const showCursor =
    frame >= typingStart &&
    typedLen < prompt.length &&
    Math.floor(frame / 15) % 2 === 0;

  /* Editorial appears early (~1.5s): reference-style, not end-of-spot */
  const headlineOpacity = interpolate(frame, [42, 72], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const headlineY = interpolate(frame, [42, 72], [22, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  const subOpacity = interpolate(frame, [58, 88], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  const navPulse =
    1 +
    0.04 *
      Math.sin((frame / fps) * Math.PI * 2 * 0.45) *
      interpolate(frame, [24, 90], [0, 1], {
        extrapolateLeft: "clamp",
        extrapolateRight: "clamp",
      });

  const workspacePage = adWorkspacePage(frame, tour.pageStart);
  const selectedNavIdx =
    workspacePage === "command" ? 5 : navIndexForPage(workspacePage) ?? 0;
  const crumbPage = workspaceBreadcrumbPage(workspacePage);
  const hoverNavIdx = cursorHoverNavIndex(frame, tour.navHoverRanges);
  const hoverAgentIdx = cursorHoverAgentIndex(
    frame,
    tour.agentHoverFrom,
    tour.agentHoverTo
  );

  const cursorPos = cursorAlongTourPath(frame, tour.path);
  const cursorPulse = clickPulse(frame, tour.clickFrames);
  const cursorIdle =
    frame >= typingStart
      ? Math.sin((frame / fps) * Math.PI * 2 * 0.85) * 0.4
      : 0;
  const cursorX = cursorPos.x + cursorIdle * 0.15;
  const cursorY = cursorPos.y + cursorIdle * 0.1;

  const topBandPx = Math.max(2, Math.round(compH * TOP_BAND_FRACTION));
  const consoleTopOpacity = interpolate(
    frame,
    [OUTRO_CONSOLE_FADE_START, OUTRO_CONSOLE_FADE_END],
    [1, 0],
    {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
      easing: Easing.inOut(Easing.cubic),
    }
  );
  const logoTopOpacity = interpolate(
    frame,
    [OUTRO_LOGO_FADE_START, OUTRO_LOGO_FADE_END],
    [0, 1],
    {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
      easing: Easing.out(Easing.cubic),
    }
  );

  return (
    <AbsoluteFill style={{ backgroundColor: CREAM }}>
      <div
        style={{
          position: "absolute",
          top: 0,
          left: 0,
          right: 0,
          height: `${TOP_BAND_FRACTION * 100}%`,
          overflow: "hidden",
          background: "#0d0a14",
        }}
      >
        <div
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 2,
            opacity: consoleTopOpacity,
            pointerEvents: "none",
          }}
        >
      {/* Top: cosmic + floating console (under the SVG outro during the handoff) */}
      <div style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0 }}>
        <WarpStars />
        <div
          style={{
            position: "absolute",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: "6% 7% 2%",
          }}
        >
          <div
            style={{
              position: "relative",
              width: "100%",
              maxWidth: TOUR_PANEL_MAX_W,
              height: "90%",
              borderRadius: 10,
              background: PANEL,
              border: `1px solid ${ND.border}`,
              boxShadow: "0 24px 70px rgba(0,0,0,0.72)",
              transform: `translateY(${windowY}px) scale(${0.96 + windowEnter * 0.04})`,
              opacity: windowOpacity,
              display: "flex",
              overflow: "hidden",
              fontFamily: uiSans,
            }}
          >
            {windowOpacity > 0.05 ? (
              <WorkspaceCursor x={cursorX} y={cursorY} pulse={cursorPulse} />
            ) : null}
            {/* Left rail — `ConsoleAppShell` aside */}
            <div
              style={{
                width: TOUR_SIDEBAR_W,
                flexShrink: 0,
                borderRight: `1px solid ${ND.border}`,
                background: ND.surface,
                display: "flex",
                flexDirection: "column",
                minHeight: 0,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  flexShrink: 0,
                  borderBottom: `1px solid ${ND.border}`,
                  padding: "16px 12px",
                }}
              >
                <div
                  style={{
                    fontSize: 13,
                    fontWeight: 500,
                    letterSpacing: "-0.02em",
                    color: ND.text,
                  }}
                >
                  Company console
                </div>
                <div
                  style={{
                    marginTop: 4,
                    fontFamily: mono,
                    fontSize: 10,
                    fontWeight: 400,
                    letterSpacing: "0.08em",
                    textTransform: "uppercase",
                    color: ND.muted,
                  }}
                >
                  Workspace
                </div>
              </div>
              <nav
                style={{
                  flex: 1,
                  minHeight: 0,
                  overflow: "hidden",
                  padding: 8,
                  display: "flex",
                  flexDirection: "column",
                  gap: 1,
                }}
              >
                {NAV.map((item, i) => {
                  const stagger = interpolate(frame, [12 + i * 3, 28 + i * 3], [0, 1], {
                    extrapolateLeft: "clamp",
                    extrapolateRight: "clamp",
                  });
                  const selected = selectedNavIdx === i;
                  const tourOn = hoverNavIdx === i;
                  return (
                    <div
                      key={item.label}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        padding: "8px 8px",
                        borderRadius: 2,
                        borderLeft: `2px solid ${selected ? ND.borderV : "transparent"}`,
                        fontSize: 13,
                        lineHeight: 1.25,
                        fontWeight: 400,
                        color: selected ? "#ffffff" : ND.muted,
                        background: selected
                          ? "rgba(255,255,255,0.03)"
                          : tourOn
                            ? "rgba(255,255,255,0.04)"
                            : "transparent",
                        boxShadow:
                          tourOn && !selected
                            ? "inset 0 0 0 1px rgba(255,255,255,0.12)"
                            : selected
                              ? `0 0 ${8 + 6 * navPulse}px rgba(255,255,255,0.04)`
                              : "none",
                        opacity: stagger,
                        transform: `translateX(${(1 - stagger) * -8}px)`,
                      }}
                    >
                      <span style={{ width: 18, textAlign: "center", opacity: 0.9, fontSize: 12 }}>
                        {item.icon}
                      </span>
                      <span style={{ flex: 1, minWidth: 0 }}>{item.label}</span>
                    </div>
                  );
                })}
                <div
                  style={{
                    marginTop: 8,
                    paddingTop: 8,
                    borderTop: `1px solid ${ND.border}`,
                  }}
                />
                <div
                  style={{
                    padding: "8px 8px",
                    borderRadius: 2,
                    fontFamily: mono,
                    fontSize: 11,
                    fontWeight: 400,
                    letterSpacing: "0.06em",
                    textTransform: "uppercase",
                    color: ND.muted,
                  }}
                >
                  Legacy console →
                </div>
              </nav>
              <div
                style={{
                  flexShrink: 0,
                  borderTop: `1px solid ${ND.border}`,
                  padding: "8px 10px 10px",
                  fontFamily: mono,
                }}
              >
                <div
                  style={{
                    fontSize: 10,
                    fontWeight: 600,
                    letterSpacing: "0.06em",
                    textTransform: "uppercase",
                    color: ND.dim,
                  }}
                >
                  API
                </div>
                <div
                  style={{
                    marginTop: 4,
                    fontSize: 9,
                    lineHeight: 1.35,
                    color: ND.muted,
                    wordBreak: "break-all",
                    textTransform: "none",
                    letterSpacing: 0,
                  }}
                >
                  /api/company/…
                </div>
              </div>
            </div>

            {/* Main + right rail — `ConsoleAppShell` */}
            <div
              style={{
                flex: 1,
                display: "flex",
                flexDirection: "column",
                minWidth: 0,
                background: PANEL,
              }}
            >
              <header
                style={{
                  height: TOUR_SHELL_HEADER_H,
                  flexShrink: 0,
                  borderBottom: `1px solid ${ND.border}`,
                  background: PANEL,
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "0 14px 0 16px",
                }}
              >
                <div
                  style={{
                    flex: 1,
                    minWidth: 0,
                    fontFamily: mono,
                    fontSize: 11,
                    letterSpacing: "0.06em",
                    textTransform: "uppercase",
                    color: ND.muted,
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    overflow: "hidden",
                    whiteSpace: "nowrap",
                  }}
                >
                  <span>Workspace</span>
                  <span style={{ opacity: 0.45 }}>›</span>
                  <span style={{ color: ND.text, fontWeight: 500 }}>{crumbPage}</span>
                </div>
                <div
                  style={{
                    height: 36,
                    maxWidth: 200,
                    padding: "0 10px",
                    borderRadius: 4,
                    border: `1px solid ${ND.borderV}`,
                    background: ND.surface,
                    fontFamily: mono,
                    fontSize: 12,
                    color: ND.text,
                    display: "flex",
                    alignItems: "center",
                    flexShrink: 0,
                  }}
                >
                  Apex Labs
                </div>
                <div
                  style={{
                    height: 32,
                    padding: "0 10px",
                    borderRadius: 4,
                    border: `1px solid ${ND.borderV}`,
                    background: ND.surface,
                    fontFamily: mono,
                    fontSize: 11,
                    letterSpacing: "0.04em",
                    color: ND.text,
                    display: "flex",
                    alignItems: "center",
                    gap: 4,
                    flexShrink: 0,
                  }}
                >
                  <span aria-hidden>⌘</span>K
                </div>
                <div
                  style={{
                    width: 32,
                    height: 32,
                    borderRadius: 4,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    color: ND.muted,
                    flexShrink: 0,
                  }}
                  title="Agents rail"
                >
                  ▥
                </div>
              </header>

              <div
                style={{
                  flex: 1,
                  display: "flex",
                  flexDirection: "row",
                  minHeight: 0,
                  overflow: "hidden",
                }}
              >
                <main
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: "auto",
                    padding: "16px 24px 20px",
                    background: PANEL,
                  }}
                >
                  <WorkspaceTourMainPane
                    page={workspacePage}
                    frame={frame}
                    pageStart={tour.pageStart}
                  />
                </main>
                <aside
                  style={{
                    width: TOUR_RAIL_W,
                    flexShrink: 0,
                    borderLeft: `1px solid ${ND.border}`,
                    background: ND.surface,
                    display: "flex",
                    flexDirection: "column",
                    padding: "12px 10px",
                    minHeight: 0,
                    overflow: "hidden",
                  }}
                >
                  <div
                    style={{
                      fontFamily: mono,
                      fontSize: 11,
                      fontWeight: 600,
                      letterSpacing: "0.08em",
                      textTransform: "uppercase",
                      color: ND.dim,
                      marginBottom: 10,
                    }}
                  >
                    Agents
                  </div>
                  {RAIL_AGENTS.map((ag, i) => {
                    const tourOn = hoverAgentIdx === i;
                    return (
                      <div
                        key={ag.id}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 8,
                          padding: "9px 8px",
                          marginBottom: 2,
                          borderRadius: 4,
                          fontSize: 12,
                          fontWeight: 500,
                          color: ND.muted,
                          fontFamily: mono,
                          background: tourOn ? "rgba(255,255,255,0.06)" : "transparent",
                          boxShadow: tourOn ? "inset 0 0 0 1px rgba(91,155,246,0.35)" : "none",
                        }}
                      >
                        <span style={{ width: 16, textAlign: "center", opacity: 0.85 }} aria-hidden>
                          ◆
                        </span>
                        <span
                          style={{
                            flex: 1,
                            minWidth: 0,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {ag.label}
                        </span>
                        {ag.live > 0 ? (
                          <span
                            style={{
                              fontSize: 9,
                              fontWeight: 700,
                              letterSpacing: "0.04em",
                              padding: "2px 6px",
                              borderRadius: 4,
                              flexShrink: 0,
                              ...badgeStyles("live"),
                            }}
                          >
                            ● {ag.live}
                          </span>
                        ) : (
                          <span style={{ fontSize: 9, color: ND.dim, flexShrink: 0 }}>—</span>
                        )}
                      </div>
                    );
                  })}
                </aside>
              </div>

              <div
                style={{
                  flexShrink: 0,
                  padding: "14px 20px 18px",
                  borderTop: `1px solid ${ND.border}`,
                  display: "flex",
                  justifyContent: "flex-end",
                  background: PANEL,
                }}
              >
                <div
                  style={{
                    maxWidth: "88%",
                    padding: "12px 18px",
                    borderRadius: 8,
                    background: ND.surface,
                    border:
                      frame >= tour.cmdGlowFrom && frame < OUTRO_CONSOLE_FADE_START
                        ? `1px solid ${ND.blue}`
                        : `1px solid ${ND.borderV}`,
                    boxShadow:
                      frame >= tour.cmdGlowFrom && frame < OUTRO_CONSOLE_FADE_START
                        ? `0 0 0 1px rgba(91,155,246,0.25), 0 0 28px rgba(91,155,246,0.12)`
                        : "none",
                    fontSize: 14,
                    color: ND.text,
                    fontWeight: 400,
                    fontFamily: uiSans,
                  }}
                >
                  <span style={{ color: ND.blue }}>&ldquo;</span>
                  {typed}
                  {showCursor ? (
                    <span style={{ color: ND.blue, marginLeft: 2 }}>|</span>
                  ) : null}
                  {typedLen >= prompt.length ? (
                    <span style={{ color: ND.blue }}>&rdquo;</span>
                  ) : null}
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
        </div>
        <div
          style={{
            position: "absolute",
            inset: 0,
            zIndex: 3,
            opacity: logoTopOpacity,
            pointerEvents: "none",
            width: "100%",
            height: "100%",
          }}
        >
          <LogoOutroCanvas width={compW} height={topBandPx} />
        </div>
      </div>

      {/* Bottom: editorial — never part of the top-band / logo outro */}
      <div
        style={{
          position: "absolute",
          bottom: 0,
          left: 0,
          right: 0,
          height: `${(1 - TOP_BAND_FRACTION) * 100}%`,
          zIndex: 10,
          padding: "5.5% 9% 5.5% 9%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          alignItems: "flex-start",
          textAlign: "left",
          background: `linear-gradient(180deg, ${CREAM}00 0%, ${CREAM} 14%)`,
        }}
      >
        <h1
          style={{
            fontFamily: displaySerif,
            fontSize: 64,
            lineHeight: 1.05,
            fontWeight: 800,
            color: "#0a0908",
            margin: 0,
            maxWidth: "92%",
            opacity: headlineOpacity,
            transform: `translateY(${headlineY}px)`,
            letterSpacing: "-0.035em",
            fontFeatureSettings: '"lnum" 1, "kern" 1',
          }}
        >
          {headline}
        </h1>
        <p
          style={{
            fontFamily: displaySerif,
            fontSize: 30,
            lineHeight: 1.38,
            fontWeight: 500,
            color: "#2f2a26",
            marginTop: 20,
            maxWidth: "94%",
            opacity: subOpacity,
            whiteSpace: "pre-line",
            letterSpacing: "-0.012em",
            fontFeatureSettings: '"lnum" 1, "kern" 1',
          }}
        >
          {subline}
        </p>
        <div
          style={{
            marginTop: 20,
            fontFamily: uiSans,
            fontSize: 13,
            letterSpacing: "0.18em",
            textTransform: "uppercase",
            color: "#8a8178",
            opacity: interpolate(frame, [78, 108], [0, 1], {
              extrapolateLeft: "clamp",
              extrapolateRight: "clamp",
            }),
          }}
        >
          HSM-II · AI company operating system
        </div>
      </div>
    </AbsoluteFill>
  );
};
