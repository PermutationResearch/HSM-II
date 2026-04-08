# Company console — Remotion ad

Vertical **9:16** spot (1080×1920 @ 30fps, ~11s): **Fraunces** (bold editorial serif) + **Inter**, cosmic header, cream editorial. The mock UI mirrors **company-console** nav (Dashboard, Inbox, Tasks, …) and lists **real product capabilities** (inbox, task graph, shared memory, workforce, goals sync, YC-bench, packs, playbooks) instead of generic files.

## Commands

The folder is **`web/remotion-company-console-ad` inside your HSM-II repo**, not under your home directory. From home, use the full path (adjust if your clone lives elsewhere):

```bash
cd ~/hyper-stigmergic-morphogenesisII/web/remotion-company-console-ad
```

If you are **already at the repo root** (`hyper-stigmergic-morphogenesisII`):

```bash
cd web/remotion-company-console-ad
```

Then:

```bash
npm install
npm run dev          # Remotion Studio
npm run build        # MP4 → out/company-console-ad.mp4
npm run still        # Poster frame → out/poster.png
```

**Avoid** pasting `npm install   # comment` on the same line as `cd` in a script block — run `cd` first, then `npm install`, so `package.json` is found.

## Customize copy

Edit default text in `src/Root.tsx` (`defaultProps`) or override in Studio via **Props**.

## Reference asset

The inspiration mock lives in the Cursor assets folder as `image-db157dca-4ee9-4b7f-81b8-8714f7b4b9b5.png` (not bundled here).
