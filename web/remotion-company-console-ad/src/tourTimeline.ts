/**
 * Tour timing + hit targets aligned with `company-console` `/workspace/*` shell
 * (`ConsoleAppShell.tsx`, `globals.css` — 15.5rem sidebar, black main, right rail).
 */

export type AdWorkspacePage =
  | "dashboard"
  | "agents"
  | "issues"
  | "marketplace"
  | "playbooks"
  | "intelligence"
  | "command";

export const TOUR_PANEL_MAX_W = 920;
/** `w-[15.5rem]` */
export const TOUR_SIDEBAR_W = 248;
/** Right agents rail — slightly under `w-[22rem]` so the mock fits the ad panel. */
export const TOUR_RAIL_W = 300;

export const TOUR_SHELL_HEADER_H = 56;

/** CSS vertical padding uses % of composition width (top band). */
export function estimatePanelSize(compWidth: number, compHeight: number): {
  panelW: number;
  panelH: number;
} {
  const topH = compHeight * 0.68;
  const padT = compWidth * 0.06;
  const padB = compWidth * 0.02;
  const innerH = topH - padT - padB;
  const panelH = innerH * 0.9;
  const panelW = Math.min(TOUR_PANEL_MAX_W, compWidth * 0.86);
  return { panelW, panelH };
}

/** Sidebar: brand block + `p-2` nav — centers align with `nd-ws-nav-link` rows. */
const SIDEBAR_BRAND_H = 56;
const NAV_PAD_TOP = 8;
const NAV_LINK_H = 36;
const NAV_GAP = 1;

function navRowCenterPx(i: number): number {
  return SIDEBAR_BRAND_H + NAV_PAD_TOP + NAV_LINK_H / 2 + i * (NAV_LINK_H + NAV_GAP);
}

/** First agent row in right rail (below rail title strip). */
function firstAgentRowCenterPx(): number {
  const h = TOUR_SHELL_HEADER_H;
  const railTitle = 12 + 11 + 8;
  const rowH = 40;
  return h + railTitle + rowH / 2;
}

export type TourPageStart = {
  dashboard: 0;
  agents: number;
  issues: number;
  marketplace: number;
  playbooks: number;
  intelligence: number;
  command: number;
};

export type PathPoint = { f: number; x: number; y: number };

export type ComputedTour = {
  path: PathPoint[];
  clickFrames: number[];
  pageStart: TourPageStart;
  navHoverRanges: { start: number; end: number }[];
  agentHoverFrom: number;
  agentHoverTo: number;
  typingStart: number;
  cmdGlowFrom: number;
};

function layoutPercents(panelW: number, panelH: number) {
  const sw = TOUR_SIDEBAR_W;
  const rw = TOUR_RAIL_W;
  const navX = (sw / 2 / panelW) * 100;
  const rowY = (i: number) => (navRowCenterPx(i) / panelH) * 100;

  const headerH = TOUR_SHELL_HEADER_H;
  const footerPromptPx = 88;
  const bodyH = Math.max(40, panelH - headerH - footerPromptPx);
  const mainMidX = sw + (panelW - sw - rw) * 0.5;
  const mainMidY = headerH + bodyH * 0.38;
  const mainX = (mainMidX / panelW) * 100;
  const mainY = (mainMidY / panelH) * 100;

  const agentX = ((panelW - rw / 2) / panelW) * 100;
  const agentY = (firstAgentRowCenterPx() / panelH) * 100;

  const cmdX = ((panelW - 44) / panelW) * 100;
  const cmdY = (28 / panelH) * 100;

  const introX = mainX;
  const introY = ((headerH + bodyH * 0.2) / panelH) * 100;

  return { navX, rowY, agentX, agentY, mainX, mainY, cmdX, cmdY, introX, introY };
}

const MOVE_FRAMES = 10;
const HOLD_FRAMES = 5;
const CLICK_AFTER_LAND = 2;

export function computeTour(panelW: number, panelH: number): ComputedTour {
  const m = layoutPercents(panelW, panelH);
  const path: PathPoint[] = [];
  const clickFrames: number[] = [];
  const navHoverRanges: { start: number; end: number }[] = [];

  let f = 6;
  path.push({ f, x: m.introX, y: m.introY });

  f += MOVE_FRAMES;
  path.push({ f, x: m.navX, y: m.rowY(0) });
  path.push({ f: f + HOLD_FRAMES, x: m.navX, y: m.rowY(0) });
  clickFrames.push(f + CLICK_AFTER_LAND);
  navHoverRanges.push({ start: f - 2, end: f + HOLD_FRAMES });
  f += HOLD_FRAMES;

  for (let i = 1; i <= 5; i++) {
    f += MOVE_FRAMES;
    path.push({ f, x: m.navX, y: m.rowY(i) });
    path.push({ f: f + HOLD_FRAMES, x: m.navX, y: m.rowY(i) });
    clickFrames.push(f + CLICK_AFTER_LAND);
    navHoverRanges.push({ start: f - 2, end: f + HOLD_FRAMES });
    f += HOLD_FRAMES;
  }

  f += MOVE_FRAMES;
  path.push({ f, x: m.agentX, y: m.agentY });
  path.push({ f: f + HOLD_FRAMES, x: m.agentX, y: m.agentY });
  clickFrames.push(f + CLICK_AFTER_LAND);
  const agentHoverFrom = f - 2;
  const agentHoverTo = f + HOLD_FRAMES + 1;
  f += HOLD_FRAMES;

  f += MOVE_FRAMES;
  path.push({ f, x: m.mainX, y: m.mainY });
  path.push({ f: f + HOLD_FRAMES, x: m.mainX, y: m.mainY });
  clickFrames.push(f + CLICK_AFTER_LAND);
  f += HOLD_FRAMES;

  f += MOVE_FRAMES;
  path.push({ f, x: m.cmdX, y: m.cmdY });
  path.push({ f: f + HOLD_FRAMES, x: m.cmdX, y: m.cmdY });
  const cmdClick = f + CLICK_AFTER_LAND;
  clickFrames.push(cmdClick);
  f += HOLD_FRAMES;

  path.push({ f: f + 90, x: m.cmdX, y: m.cmdY });

  const pageStart: TourPageStart = {
    dashboard: 0,
    agents: clickFrames[1] + 1,
    issues: clickFrames[2] + 1,
    marketplace: clickFrames[3] + 1,
    playbooks: clickFrames[4] + 1,
    intelligence: clickFrames[5] + 1,
    command: cmdClick + 1,
  };

  return {
    path,
    clickFrames,
    pageStart,
    navHoverRanges,
    agentHoverFrom,
    agentHoverTo,
    typingStart: cmdClick + 2,
    cmdGlowFrom: cmdClick - 1,
  };
}

export function adWorkspacePage(
  frame: number,
  ps: TourPageStart
): AdWorkspacePage {
  if (frame < ps.agents) return "dashboard";
  if (frame < ps.issues) return "agents";
  if (frame < ps.marketplace) return "issues";
  if (frame < ps.playbooks) return "marketplace";
  if (frame < ps.intelligence) return "playbooks";
  if (frame < ps.command) return "intelligence";
  return "command";
}

export function cursorAlongTourPath(
  frame: number,
  path: PathPoint[]
): { x: number; y: number } {
  const p = path;
  if (frame <= p[0].f) {
    return { x: p[0].x, y: p[0].y };
  }
  if (frame >= p[p.length - 1].f) {
    return { x: p[p.length - 1].x, y: p[p.length - 1].y };
  }
  for (let i = 0; i < p.length - 1; i++) {
    const a = p[i];
    const b = p[i + 1];
    if (frame >= a.f && frame <= b.f) {
      const dur = b.f - a.f;
      const raw = dur <= 0 ? 1 : (frame - a.f) / dur;
      const t = Math.min(1, Math.max(0, raw));
      return {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
      };
    }
  }
  return { x: p[p.length - 1].x, y: p[p.length - 1].y };
}

export function cursorHoverNavIndex(
  frame: number,
  navHoverRanges: { start: number; end: number }[]
): number | null {
  for (let i = 0; i < navHoverRanges.length; i++) {
    const r = navHoverRanges[i];
    if (frame >= r.start && frame <= r.end) return i;
  }
  return null;
}

export function cursorHoverAgentIndex(
  frame: number,
  from: number,
  to: number
): number | null {
  if (frame >= from && frame < to) return 0;
  return null;
}
