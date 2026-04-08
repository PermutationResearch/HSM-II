---
name: remotion-company-ad
description: Reusable Remotion vertical product spot (1080×1920)—workspace tour, editorial band, logo outro, typography, and timing. Use when building or adapting a company promo video in the same structure as web/remotion-company-console-ad, or when the user mentions Remotion company console ad, product tour video, or similar B2B spot template.
---

# Remotion company product spot (template)

This repo ships a **complete Remotion composition** any team can fork to produce a **similar** vertical ad: product UI tour → crossfade → **logo outro** (zoom) → editorial copy. The implementation lives under **`web/remotion-company-console-ad/`**.

## What you get (structure)

| Block | Role |
|--------|------|
| **Top band (~68% height)** | Animated “console” walkthrough (nav, main pane, cursor path, typing). Cosmic background + floating window. Fades out into the outro. |
| **Logo outro layer** | Full-bleed black band; centered raster logo; **scale-in** on reveal + **dolly zoom** during the hold. See `src/LogoOutro.tsx`. |
| **Bottom band (~32%)** | Editorial **headline**, **subline**, small **footer**—magazine-style over cream gradient. Driven by composition props. |

**Composition:** `CompanyConsoleAd` in `src/Root.tsx` — **30 fps**, **1080×1920**, duration from `COMPOSITION_DURATION_FRAMES` in `src/outroConstants.ts`.

## Typography (match or swap)

- **Display / hero:** **Fraunces** (`@remotion/google-fonts/Fraunces`) — weights 500–800, editorial serif.
- **UI / body in the mock:** **Inter** (`@remotion/google-fonts/Inter`) — weights 400–600.

To rebrand: load different Google fonts in `CompanyConsoleAd.tsx` the same way and update `fontFamily` on the headline, subline, and shell UI blocks.

## Copy & inputs (per company)

`Root.tsx` **`defaultProps`** (or Remotion **input props**):

- **`headline`** — main bottom-band line.
- **`subline`** — supporting paragraph; use `\n\n` for a second paragraph break.
- **`prompt`** — fake command-line / chat string for the typing animation in the tour.

Logo file: **`public/Skills-Keychain-letterpress.png`** (replace with your asset; keep name or pass `letterpressFile` if you extend `LogoOutroCanvas` props through the composition).

## Timing knobs (global beat)

All in **`src/outroConstants.ts`**:

- **`TOP_BAND_FRACTION`** — product vs editorial split.
- **`OUTRO_CONSOLE_FADE_*` / `OUTRO_LOGO_FADE_*`** — handoff from UI to logo.
- **`MASCOT_SPIN_HOLD_FRAMES`** — how long the logo stays **after** full fade-in (dolly zoom runs across this span in `LogoOutro.tsx`).
- **`COMPOSITION_DURATION_FRAMES`** — total length (includes a short tail after the hold).

Adjust tour beats in **`src/tourTimeline.ts`** and UI chrome in **`src/WorkspaceTourViews.tsx`** (labels, nav order, colors) to mirror *your* product shell, not “Company console” literally.

## Commands

```bash
cd web/remotion-company-console-ad
npm install   # if needed
npm run dev   # Remotion Studio — src/index.ts entry
npm run build # render CompanyConsoleAd to out/
```

## Checklist: adapt for another company

1. Replace **`defaultProps`** headline / subline / prompt.
2. Swap **`public/Skills-Keychain-letterpress.png`** (or wire a new static file name).
3. Retheme **`WorkspaceTourViews.tsx`** / **`CompanyConsoleAd.tsx`** colors (`ND`, `PANEL`, `CREAM`, etc.) to brand.
4. Edit **`tourTimeline.ts`** path and frame ranges so the cursor story matches your narrative.
5. Tune **`outroConstants.ts`** if you need a longer/shorter outro or different split.
6. In **`LogoOutro.tsx`**, adjust zoom end scale (`1.28`) or easing if the logo feel should change.

## Key files (quick map)

- `src/Root.tsx` — composition id, fps, size, default props.
- `src/CompanyConsoleAd.tsx` — full layout, tour shell, editorial band, opacity handoff.
- `src/LogoOutro.tsx` — black field, centered logo, intro scale + dolly zoom.
- `src/outroConstants.ts` — duration and outro frame numbers.
- `src/tourTimeline.ts` — cursor path and page timing.
- `src/WorkspaceTourViews.tsx` — mock workspace pages and chrome.

This skill does **not** replace Remotion docs; it ties **this repo’s** structure to a repeatable workflow for **similar** B2B product spots.

## See also

For workflows outside this template (e.g. broader creative/media pipelines), agents may reference **[Nous Hermes skills](https://github.com/NousResearch/hermes-agent/tree/main/skills)** and adapt as needed.
