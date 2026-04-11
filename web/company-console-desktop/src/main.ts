import { app, BrowserWindow, Menu, shell } from "electron";
import { spawn, spawnSync, type ChildProcess } from "child_process";
import * as fs from "fs";
import * as net from "net";
import * as os from "os";
import * as path from "path";

const children: ChildProcess[] = [];

function getRepoRoot(): string {
  // dist/ → company-console-desktop → web → repo root (contains Cargo.toml)
  return path.resolve(__dirname, "..", "..", "..");
}

function devCompanyConsoleRoot(): string {
  return path.resolve(__dirname, "..", "..", "company-console");
}

/**
 * GUI-launched Electron often has no shell `export OPENROUTER_API_KEY=…`.
 * Read repo + company-console `.env*` so the spawned Next server can run LLM routes.
 */
function readOpenRouterEnvFromDotenv(repoRoot: string, companyConsoleRoot: string): Record<string, string> {
  const out: Record<string, string> = {};
  const files = [
    path.join(repoRoot, ".env"),
    path.join(repoRoot, ".env.local"),
    path.join(companyConsoleRoot, ".env"),
    path.join(companyConsoleRoot, ".env.local"),
  ];
  const strip = (s: string) => {
    let v = s.replace(/\r$/, "").trim();
    if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) v = v.slice(1, -1);
    return v;
  };
  for (const fp of files) {
    if (!fs.existsSync(fp)) continue;
    const text = fs.readFileSync(fp, "utf8");
    for (const line of text.split("\n")) {
      const t = line.trim();
      if (!t || t.startsWith("#")) continue;
      const m = /^(?:export\s+)?(OPENROUTER_API_KEY|HSM_OPENROUTER_API_KEY|OPENROUTER_API_BASE)\s*=\s*(.*)$/.exec(t);
      if (!m) continue;
      const val = strip(m[2] ?? "");
      if (!val) continue;
      if (m[1] === "HSM_OPENROUTER_API_KEY") out.OPENROUTER_API_KEY = val;
      else out[m[1]] = val;
    }
  }
  return out;
}

/** Directory whose cwd we use for Next: either `.next/standalone` or the app root for `next start`. */
function getUiRoot(): string {
  if (app.isPackaged) {
    return path.join(process.resourcesPath, "ui");
  }
  return path.join(devCompanyConsoleRoot(), ".next", "standalone");
}

/** All `hsm_console` executables under `target/` (host + cross-target layouts). */
function collectHsmBinariesUnderTarget(repoRoot: string): string[] {
  const targetRoot = path.join(repoRoot, "target");
  const out: string[] = [];
  const tryPush = (p: string) => {
    if (fs.existsSync(p)) out.push(p);
  };
  tryPush(path.join(targetRoot, "release", "hsm_console"));
  tryPush(path.join(targetRoot, "debug", "hsm_console"));
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(targetRoot, { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    const n = e.name;
    if (n === "release" || n === "debug" || n === "tmp" || n.startsWith(".")) continue;
    tryPush(path.join(targetRoot, n, "release", "hsm_console"));
    tryPush(path.join(targetRoot, n, "debug", "hsm_console"));
  }
  return out;
}

function pickPreferredBinary(paths: string[]): string | null {
  if (!paths.length) return null;
  const rel = paths.filter((p) => p.includes(`${path.sep}release${path.sep}`));
  const pool = rel.length ? rel : paths;
  let best = pool[0]!;
  let bestM = 0;
  for (const p of pool) {
    try {
      const m = fs.statSync(p).mtimeMs;
      if (m >= bestM) {
        bestM = m;
        best = p;
      }
    } catch {
      /* skip */
    }
  }
  return best;
}

function findOnPath(exeName: string): string | null {
  const cmd = process.platform === "win32" ? "where" : "which";
  const r = spawnSync(cmd, [exeName], { encoding: "utf8" });
  if (r.status !== 0 || !r.stdout) return null;
  const line = r.stdout.trim().split(/\r?\n/)[0]?.trim();
  if (!line || !fs.existsSync(line)) return null;
  return line;
}

/**
 * Absolute path to `hsm_console`, or `null` if we should try `cargo run` / show an error.
 */
function resolveHsmConsoleExecutable(repoRoot: string): string | null {
  if (process.env.HSM_CONSOLE_BIN?.trim()) {
    const p = process.env.HSM_CONSOLE_BIN.trim();
    if (!fs.existsSync(p)) {
      throw new Error(`HSM_CONSOLE_BIN is set but file does not exist:\n${p}`);
    }
    return p;
  }
  if (app.isPackaged) {
    const bundled = path.join(process.resourcesPath, "hsm_console");
    return fs.existsSync(bundled) ? bundled : null;
  }
  const fromTarget = pickPreferredBinary(collectHsmBinariesUnderTarget(repoRoot));
  if (fromTarget) return fromTarget;
  const cargoBin = path.join(os.homedir(), ".cargo", "bin", "hsm_console");
  if (fs.existsSync(cargoBin)) return cargoBin;
  const which = findOnPath("hsm_console");
  if (which) return which;
  return null;
}

function spawnHsmConsole(repoRoot: string, apiPort: number): { proc: ChildProcess; viaCargo: boolean } {
  const exe = resolveHsmConsoleExecutable(repoRoot);
  if (exe) {
    return {
      proc: spawn(exe, ["--port", String(apiPort), "--host", "127.0.0.1"], {
        cwd: repoRoot,
        env: { ...process.env },
        stdio: "inherit",
      }),
      viaCargo: false,
    };
  }
  const noCargo = process.env.HSM_DESKTOP_NO_CARGO === "1";
  const cargoToml = path.join(repoRoot, "Cargo.toml");
  if (!app.isPackaged && !noCargo && fs.existsSync(cargoToml)) {
    return {
      proc: spawn(
        "cargo",
        [
          "run",
          "-p",
          "hyper-stigmergy",
          "--bin",
          "hsm_console",
          "--",
          "--port",
          String(apiPort),
          "--host",
          "127.0.0.1",
        ],
        {
          cwd: repoRoot,
          env: { ...process.env },
          stdio: "inherit",
        }
      ),
      viaCargo: true,
    };
  }
  const tried = [
    path.join(repoRoot, "target", "release", "hsm_console"),
    path.join(repoRoot, "target", "debug", "hsm_console"),
    "`target/<triple>/release/hsm_console` (if you use `--target`)",
    path.join(os.homedir(), ".cargo", "bin", "hsm_console"),
    "`hsm_console` on PATH",
  ].join("\n  ");
  throw new Error(
    `hsm_console binary not found.\n\n` +
      `Repo root (resolved): ${repoRoot}\n\n` +
      `Tried / expected:\n  ${tried}\n\n` +
      `Fix one of:\n` +
      `  cargo build -p hyper-stigmergy --bin hsm_console\n` +
      `  export HSM_CONSOLE_BIN=/path/to/hsm_console\n` +
      `  Or run from dev without a pre-built binary: unset HSM_DESKTOP_NO_CARGO (default) so the app runs "cargo run …" (slower first launch).\n`
  );
}

function findFreePort(startPort: number): Promise<number> {
  return new Promise((resolve, reject) => {
    const tryPort = (p: number) => {
      const s = net.createServer();
      s.once("error", (err: NodeJS.ErrnoException) => {
        if (err.code === "EADDRINUSE") {
          s.close(() => tryPort(p + 1));
        } else {
          reject(err);
        }
      });
      s.listen(p, "127.0.0.1", () => {
        const addr = s.address();
        const port = typeof addr === "object" && addr ? addr.port : p;
        s.close(() => resolve(port));
      });
    };
    tryPort(startPort);
  });
}

async function waitForOk(url: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastErr = "";
  while (Date.now() < deadline) {
    try {
      const r = await fetch(url, { signal: AbortSignal.timeout(2000) });
      if (r.ok) return;
      lastErr = `${r.status}`;
    } catch (e) {
      lastErr = e instanceof Error ? e.message : String(e);
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`Timeout waiting for ${url} (last: ${lastErr})`);
}

async function assertPortFree(port: number, label: string): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const s = net.createServer();
    s.once("error", (err: NodeJS.ErrnoException) => {
      if (err.code === "EADDRINUSE") {
        reject(
          new Error(
            `${label} port ${port} is already in use.\n` +
              `Stop the existing process on :${port} and retry.`
          )
        );
      } else {
        reject(err);
      }
    });
    s.once("listening", () => s.close(() => resolve()));
    s.listen(port, "127.0.0.1");
  });
}

/**
 * If `envName` is unset, bind-scan upward from `fallback` (matches README: first free port from base).
 * If set, require that exact port to be free.
 */
async function resolveListenPort(envName: string, fallback: number, label: string): Promise<number> {
  const raw = process.env[envName]?.trim();
  if (!raw) {
    return findFreePort(fallback);
  }
  const n = Number.parseInt(raw, 10);
  const port = Number.isFinite(n) && n > 0 && n <= 65535 ? n : fallback;
  await assertPortFree(port, label);
  return port;
}

function pushChild(cp: ChildProcess | null): void {
  if (cp) children.push(cp);
}

function shutdownChildren(): void {
  for (const c of children) {
    try {
      c.kill("SIGTERM");
    } catch {
      /* ignore */
    }
  }
  children.length = 0;
}

async function startStack(): Promise<{ uiUrl: string }> {
  const apiPort = await resolveListenPort("HSM_DESKTOP_API_PORT", 3847, "API");
  const uiPort = await resolveListenPort("HSM_DESKTOP_UI_PORT", 3050, "UI");
  const apiBase = `http://127.0.0.1:${apiPort}`;
  const uiUrl = `http://127.0.0.1:${uiPort}`;

  const repoRoot = getRepoRoot();
  const ccRoot = devCompanyConsoleRoot();
  const openRouterFromFiles = readOpenRouterEnvFromDotenv(repoRoot, ccRoot);
  const { proc: api, viaCargo } = spawnHsmConsole(repoRoot, apiPort);
  pushChild(api);

  const healthTimeoutMs = viaCargo ? 300_000 : 90_000;
  await waitForOk(`${apiBase}/api/company/health`, healthTimeoutMs);

  const uiRoot = getUiRoot();
  const standaloneServer = path.join(uiRoot, "server.js");

  if (fs.existsSync(standaloneServer)) {
    const child = spawn(process.execPath, [standaloneServer], {
      cwd: uiRoot,
      env: {
        ...process.env,
        ...openRouterFromFiles,
        PORT: String(uiPort),
        HOSTNAME: "127.0.0.1",
        HSM_CONSOLE_URL: apiBase,
        NODE_ENV: "production",
      },
      stdio: "inherit",
    });
    pushChild(child);
  } else {
    const nextCli = path.join(ccRoot, "node_modules", "next", "dist", "bin", "next");
    if (!fs.existsSync(nextCli)) {
      shutdownChildren();
      throw new Error(
        `Company console not built.\n` +
          `  cd web/company-console && npm install && npm run build\n` +
          `Expected standalone server at:\n  ${standaloneServer}`
      );
    }
    const child = spawn(process.execPath, [nextCli, "start", "-p", String(uiPort), "-H", "127.0.0.1"], {
      cwd: ccRoot,
      env: {
        ...process.env,
        ...openRouterFromFiles,
        HSM_CONSOLE_URL: apiBase,
        NODE_ENV: "production",
      },
      stdio: "inherit",
    });
    pushChild(child);
  }

  await waitForOk(uiUrl, 120_000);
  return { uiUrl };
}

function buildMenu(win: BrowserWindow): Menu {
  const template: Electron.MenuItemConstructorOptions[] = [
    {
      label: app.name,
      submenu: [{ role: "about" }, { type: "separator" }, { role: "quit" }],
    },
    {
      label: "View",
      submenu: [
        { role: "reload" },
        { role: "forceReload" },
        { role: "toggleDevTools" },
        { type: "separator" },
        { role: "resetZoom" },
        { role: "zoomIn" },
        { role: "zoomOut" },
      ],
    },
    {
      label: "Window",
      submenu: [{ role: "minimize" }, { role: "close" }],
    },
  ];
  return Menu.buildFromTemplate(template);
}

async function createWindow(): Promise<void> {
  let uiUrl: string;
  try {
    const r = await startStack();
    uiUrl = r.uiUrl;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    const w = new BrowserWindow({ width: 720, height: 520, show: true });
    w.loadURL(
      "data:text/html;charset=utf-8," +
        encodeURIComponent(
          `<!DOCTYPE html><html><body style="font-family:system-ui;padding:24px;background:#111;color:#eee">
<h1>HSM Company OS</h1>
<pre style="white-space:pre-wrap;color:#f88">${msg.replace(/</g, "&lt;")}</pre>
</body></html>`
        )
    );
    return;
  }

  const win = new BrowserWindow({
    width: 1280,
    height: 840,
    minWidth: 900,
    minHeight: 600,
    title: "HSM Company OS",
    show: false,
    backgroundColor: "#000000",
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  win.once("ready-to-show", () => {
    win.show();
  });

  Menu.setApplicationMenu(buildMenu(win));
  win.webContents.setWindowOpenHandler(({ url }) => {
    void shell.openExternal(url);
    return { action: "deny" };
  });

  win.webContents.on("did-fail-load", (_e, code, desc, url) => {
    console.error("[electron] did-fail-load", code, desc, url);
  });

  // In desktop dev, stale cache can keep old Next chunks/styles after rebuilds.
  if (!app.isPackaged) {
    try {
      await win.webContents.session.clearCache();
    } catch {
      /* ignore cache-clear failures */
    }
    const sep = uiUrl.includes("?") ? "&" : "?";
    uiUrl = `${uiUrl}${sep}__desktop_reload=${Date.now()}`;
  }

  await win.loadURL(uiUrl);
  if (!win.isVisible()) win.show();
}

app.on("window-all-closed", () => {
  shutdownChildren();
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
  shutdownChildren();
});

app.whenReady().then(() => {
  void createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void createWindow();
  });
});
