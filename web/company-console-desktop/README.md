# HSM Company OS (desktop)

Electron shell modeled after [paperclip-desktop](https://github.com/aronprins/paperclip-desktop): one app launches **`hsm_console`** (Rust) and the **company-console** Next.js UI, then opens a native window.

This is **not** a fork of Paperclip Desktop; it follows the same pattern (spawn API + spawn UI + BrowserWindow).

## Important: run from the **git repository root**

If you see `could not find Cargo.toml in /Users/you` or `Could not read package.json` under `/Users/you`, your shell is in your **home directory**, not the clone.

```bash
cd /Users/cno/hyper-stigmergic-morphogenesisII   # ← your actual clone path
```

Every `cargo`, `cd web/…`, and `npm` step below assumes **`Cargo.toml` is in the current directory**.

### Easiest: one script (works from anywhere)

From any directory:

```bash
bash /Users/cno/hyper-stigmergic-morphogenesisII/web/company-console-desktop/scripts/run-dev.sh
```

(Replace the path if your clone lives elsewhere.) It finds the repo root, builds company-console if needed, then runs Electron.

Or, after `cd` into `web/company-console-desktop`:

```bash
npm run dev:setup
```

## Development (manual steps)

Prerequisites: Node 20+, Rust toolchain, repo clone.

```bash
cd /path/to/hyper-stigmergic-morphogenesisII   # repo root — must contain Cargo.toml

# Terminal A — API
cargo run -p hyper-stigmergy --bin hsm_console -- --port 3847

# Terminal B — optional: run UI manually
cd web/company-console && npm install && npm run dev
```

Or let Electron start both (after a **production** Next build exists):

```bash
cd /path/to/hyper-stigmergic-morphogenesisII   # repo root
cd web/company-console && npm install && npm run build
cd ../company-console-desktop && npm install && npm run dev
```

`npm run dev` compiles `src/*.ts` and runs Electron. It starts the API by:

1. **`HSM_CONSOLE_BIN`** (if set — must exist)
2. **`target/release/hsm_console`**, **`target/debug/hsm_console`**, or **`target/<triple>/{release,debug}/hsm_console`** (cross-compile layouts)
3. **`~/.cargo/bin/hsm_console`** or **`hsm_console` on `PATH`**
4. If none of the above: **`cargo run -p hyper-stigmergy --bin hsm_console`** from the repo root (first launch can compile for several minutes). Set **`HSM_DESKTOP_NO_CARGO=1`** to disable this fallback.

The UI uses Next standalone at `web/company-console/.next/standalone/server.js`, otherwise `next start` in `web/company-console`.

## Packaged macOS build

```bash
cd /path/to/hyper-stigmergic-morphogenesisII   # repo root

cargo build --release -p hyper-stigmergy --bin hsm_console
cd web/company-console && npm install && npm run build
cd ../company-console-desktop && npm install && npm run dist:mac
```

`npm run stage` copies `target/release/hsm_console` and the Next standalone tree into `staged/`. Installers land in `dist-installer/`.

### Postgres / env

The desktop process inherits your environment (`HSM_COMPANY_OS_DATABASE_URL`, `HSMII_HOME`, etc.). Set them in your shell or macOS app environment as you would for `hsm_console`.

## Ports

The app picks free ports starting at **3847** (API) and **3050** (UI) so multiple runs are less likely to collide.

### “Unstyled” / white page / looks like raw HTML

- **Do not use `http://127.0.0.1:3847` in the browser for the product UI.** That is **`hsm_console`** (API + small HTML hint page). The **styled** Company OS is **Next.js** on **3050** (or whatever free port the desktop picked after 3050).
- The sidebar shows `apiBase` (often `3847`) in the footer — that is the **backend URL**, not where the page is served from.
- If the real UI (correct port) still has **no CSS**, open DevTools → Network and check for **`/_next/static/...css` 404**. **`npm run build` in `web/company-console` now runs `scripts/copy-standalone-assets.mjs`**, which copies `.next/static` and `public` into `.next/standalone/` (Next does not do this automatically). Rebuild after pulling. Packaged apps still use `stage-from-repo.mjs`, which merges the same folders.
