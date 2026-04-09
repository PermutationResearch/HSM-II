"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Eye, File, Folder, FolderPlus, PencilLine, Trash2 } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { AgentWorkspaceTaskHistory } from "@/app/components/console/AgentWorkspaceTaskHistory";
import { WorkspaceNewIssueDialog } from "@/app/components/console/WorkspaceNewIssueDialog";
import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { ScrollArea } from "@/app/components/ui/scroll-area";
import { Skeleton } from "@/app/components/ui/skeleton";
import { Textarea } from "@/app/components/ui/textarea";
export type WorkspaceListEntry = {
  name: string;
  path: string;
  kind: "file" | "dir";
  size_bytes?: number | null;
  modified_at?: string | null;
};

function normalizeListEntry(x: unknown): WorkspaceListEntry | null {
  if (!x || typeof x !== "object") return null;
  const o = x as Record<string, unknown>;
  const name = typeof o.name === "string" ? o.name : "";
  const path = typeof o.path === "string" ? o.path : "";
  const kind = o.kind === "dir" || o.kind === "file" ? o.kind : null;
  if (!name || !path || !kind) return null;
  const size_bytes =
    typeof o.size_bytes === "number" && Number.isFinite(o.size_bytes) ? o.size_bytes : null;
  const modified_at = typeof o.modified_at === "string" ? o.modified_at : null;
  return { name, path, kind, size_bytes, modified_at };
}

function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function isMarkdownPath(path: string | null): boolean {
  return !!path && /\.md$/i.test(path);
}

function isTextRenderable(path: string | null): boolean {
  if (!path) return false;
  return /\.(md|txt|json|ts|tsx|js|jsx|rs|py|toml|yaml|yml|html|css|sql)$/i.test(path);
}

/** File path must live under `agents/<pack>/…` (not the folder itself). Case-insensitive so roster `ceo` matches on-disk `CEO`. */
function pathUnderAgentRoot(filePath: string, rootPrefix: string): boolean {
  const f = filePath.replace(/^\/+/, "").replace(/\/+$/, "").toLowerCase();
  const r = rootPrefix.replace(/^\/+/, "").replace(/\/+$/, "").toLowerCase();
  if (!f || f === r) return false;
  return f.startsWith(`${r}/`);
}

/** Same-origin: flat proxy (avoids nested `/api/company/companies/.../delete` 404). Direct Rust: full path. */
function workspaceFileDeleteRequest(apiBase: string, companyId: string, relPath: string) {
  const useFlatProxy = !(apiBase ?? "").trim();
  const url = useFlatProxy
    ? "/api/company/delete-workspace-file"
    : companyOsUrl(apiBase, `/api/company/companies/${companyId}/workspace/file/delete`);
  const body = useFlatProxy ? JSON.stringify({ companyId, path: relPath }) : JSON.stringify({ path: relPath });
  return { url, body };
}

/** POST delete first (proxy-friendly); on 404/405 fall back to DELETE `.../workspace/file?path=` (older binaries). */
async function deleteWorkspaceFileRel(
  apiBase: string,
  companyId: string,
  relPath: string,
): Promise<Response> {
  const { url, body } = workspaceFileDeleteRequest(apiBase, companyId, relPath);
  let r = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body,
  });
  if (r.ok) return r;
  if (r.status !== 404 && r.status !== 405) return r;
  const delUrl = companyOsUrl(
    apiBase,
    `/api/company/companies/${companyId}/workspace/file?path=${encodeURIComponent(relPath)}`,
  );
  return fetch(delUrl, { method: "DELETE", headers: { Accept: "application/json" } });
}

function timeAgo(iso: string | null | undefined): string {
  if (!iso) return "—";
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return "—";
  const sec = Math.floor((Date.now() - t) / 1000);
  if (sec < 10) return "just now";
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 48) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 14) return `${day}d ago`;
  return new Date(iso).toLocaleDateString();
}

type Props = {
  apiBase: string;
  companyId: string;
  /** Pack folder id under `agents/` (matches `company_agents.name`). */
  agentPackName: string;
  /** Label for the new-issue modal checkbox, e.g. Corey. */
  assigneeDisplayName: string;
  /** Company issue key prefix (e.g. COM) for task id column. */
  issueKeyPrefix: string;
};

export function AgentWorkspacePanel({
  apiBase,
  companyId,
  agentPackName,
  assigneeDisplayName,
  issueKeyPrefix,
}: Props) {
  const rootPrefix = `agents/${agentPackName}`;
  const [browsePath, setBrowsePath] = useState(rootPrefix);
  const [listLoading, setListLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [entries, setEntries] = useState<WorkspaceListEntry[]>([]);
  const [hsmiiHomeHint, setHsmiiHomeHint] = useState<string | null>(null);

  const [openFilePath, setOpenFilePath] = useState<string | null>(null);
  const [editorContent, setEditorContent] = useState("");
  const [baselineContent, setBaselineContent] = useState("");
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [issueOpen, setIssueOpen] = useState(false);
  const [readerMode, setReaderMode] = useState<"preview" | "edit">("preview");
  const [workspaceMode, setWorkspaceMode] = useState<"review" | "manage">("review");
  const [filterQuery, setFilterQuery] = useState("");
  const autoDescendTried = useRef(false);

  useEffect(() => {
    setBrowsePath(rootPrefix);
    setOpenFilePath(null);
    setEditorContent("");
    setBaselineContent("");
    setFileError(null);
    setSaveState("idle");
    setReaderMode("preview");
    setWorkspaceMode("review");
    setFilterQuery("");
    autoDescendTried.current = false;
  }, [rootPrefix]);

  const dirty =
    openFilePath !== null && !fileLoading && !fileError && editorContent !== baselineContent;

  const loadList = useCallback(async () => {
    setListLoading(true);
    setListError(null);
    try {
      const qs = new URLSearchParams();
      if (browsePath) qs.set("path", browsePath);
      const r = await fetch(
        companyOsUrl(apiBase, `/api/company/companies/${companyId}/workspace/list?${qs.toString()}`),
      );
      const j = (await r.json().catch(() => ({}))) as {
        error?: string;
        entries?: unknown[];
        hsmii_home?: string;
      };
      if (!r.ok) {
        throw new Error(j.error ?? `${r.status}`);
      }
      setHsmiiHomeHint(typeof j.hsmii_home === "string" ? j.hsmii_home : null);
      const raw = Array.isArray(j.entries) ? j.entries : [];
      const normalized = raw.map(normalizeListEntry).filter((x): x is WorkspaceListEntry => x !== null);
      normalized.sort((a, b) => {
        if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
        const ta = a.modified_at ? new Date(a.modified_at).getTime() : 0;
        const tb = b.modified_at ? new Date(b.modified_at).getTime() : 0;
        if (ta !== tb) return tb - ta;
        return a.name.localeCompare(b.name);
      });
      setEntries(normalized);
    } catch (e) {
      setEntries([]);
      setListError(e instanceof Error ? e.message : String(e));
    } finally {
      setListLoading(false);
    }
  }, [apiBase, companyId, browsePath]);

  useEffect(() => {
    void loadList();
  }, [loadList]);

  /** If pack root has no files but a `workspace/` dir (common layout), open it so AGENTS.md-style trees match Paperclip. */
  useEffect(() => {
    if (listLoading || listError || browsePath !== rootPrefix || autoDescendTried.current) return;
    const filesAtRoot = entries.filter((e) => e.kind === "file");
    const workspaceDir = entries.find((e) => e.kind === "dir" && e.name.toLowerCase() === "workspace");
    if (filesAtRoot.length === 0 && workspaceDir) {
      autoDescendTried.current = true;
      setBrowsePath(workspaceDir.path);
    }
  }, [listLoading, listError, browsePath, rootPrefix, entries]);

  const goUp = useCallback(() => {
    setBrowsePath((p) => {
      const t = p.replace(/\/+$/, "");
      if (!t || t.length <= rootPrefix.length) return rootPrefix;
      const i = t.lastIndexOf("/");
      return i <= 0 ? rootPrefix : t.slice(0, i);
    });
    setOpenFilePath(null);
    setEditorContent("");
    setBaselineContent("");
  }, [rootPrefix]);

  const goRootAgent = useCallback(() => {
    setBrowsePath(rootPrefix);
    setOpenFilePath(null);
    setEditorContent("");
    setBaselineContent("");
  }, [rootPrefix]);

  const openFile = useCallback(
    async (relPath: string) => {
      setOpenFilePath(relPath);
      setFileLoading(true);
      setFileError(null);
      setSaveState("idle");
      setReaderMode(isMarkdownPath(relPath) ? "preview" : "edit");
      try {
        const r = await fetch(
          companyOsUrl(
            apiBase,
            `/api/company/companies/${companyId}/workspace/file?path=${encodeURIComponent(relPath)}`,
          ),
        );
        const j = (await r.json().catch(() => ({}))) as { error?: string; content?: string };
        if (!r.ok) throw new Error(j.error ?? `${r.status}`);
        const c = typeof j.content === "string" ? j.content : "";
        setEditorContent(c);
        setBaselineContent(c);
      } catch (e) {
        setEditorContent("");
        setBaselineContent("");
        setFileError(e instanceof Error ? e.message : String(e));
      } finally {
        setFileLoading(false);
      }
    },
    [apiBase, companyId],
  );

  const saveFile = useCallback(async () => {
    if (!openFilePath) return;
    setSaveState("saving");
    try {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/workspace/file`), {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: openFilePath, content: editorContent }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      setBaselineContent(editorContent);
      setSaveState("saved");
      setTimeout(() => setSaveState("idle"), 2000);
      void loadList();
    } catch (e) {
      setSaveState("error");
      setFileError(e instanceof Error ? e.message : String(e));
    }
  }, [apiBase, companyId, openFilePath, editorContent, loadList]);

  const newFile = useCallback(async () => {
    const name = window.prompt(`New file name (created under ${browsePath || rootPrefix})`, "notes.md");
    if (!name || !name.trim()) return;
    const base = browsePath.trim() || rootPrefix;
    const rel = `${base.replace(/\/+$/, "")}/${name.trim().replace(/^\/+/, "")}`;
    if (!pathUnderAgentRoot(rel, rootPrefix)) {
      setListError("Path must stay under this agent folder.");
      return;
    }
    try {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/workspace/file`), {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: rel, content: "" }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      void loadList();
      void openFile(rel);
    } catch (e) {
      setListError(e instanceof Error ? e.message : String(e));
    }
  }, [apiBase, companyId, browsePath, rootPrefix, loadList, openFile]);

  const newFolder = useCallback(async () => {
    const name = window.prompt(`New folder name (under ${browsePath || rootPrefix})`, "workspace");
    if (!name || !name.trim()) return;
    const base = browsePath.trim() || rootPrefix;
    const rel = `${base.replace(/\/+$/, "")}/${name.trim().replace(/^\/+/, "").replace(/\/+$/, "")}`;
    if (!pathUnderAgentRoot(rel, rootPrefix)) {
      setListError("Path must stay under this agent folder.");
      return;
    }
    try {
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/workspace/mkdir`), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: rel }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      void loadList();
      setBrowsePath(rel);
      setOpenFilePath(null);
      setEditorContent("");
      setBaselineContent("");
    } catch (e) {
      setListError(e instanceof Error ? e.message : String(e));
    }
  }, [apiBase, companyId, browsePath, rootPrefix, loadList]);

  /** POST `/workspace/file/delete` — avoids HTTP 405 when DELETE is stripped (proxy / old hsm_console). */
  const deleteFileAtPath = useCallback(
    async (relPath: string) => {
      if (!pathUnderAgentRoot(relPath, rootPrefix)) {
        setListError("Can only delete files under this agent workspace tree.");
        return;
      }
      if (!window.confirm(`Delete this file permanently?\n\n${relPath}`)) return;
      setListError(null);
      setFileError(null);
      try {
        const r = await deleteWorkspaceFileRel(apiBase, companyId, relPath);
        const raw = await r.text();
        let j = {} as { error?: string };
        try {
          j = raw ? (JSON.parse(raw) as { error?: string }) : {};
        } catch {
          j = {};
        }
        if (!r.ok) {
          if (r.status === 405) {
            throw new Error(
              "This API build does not allow workspace file delete (HTTP 405). Rebuild and restart hsm_console from the repo: cargo run -p hyper-stigmergy --bin hsm_console",
            );
          }
          const msg = typeof j.error === "string" && j.error.trim() ? j.error.trim() : `delete ${r.status}`;
          if (r.status === 404 && !j.error) {
            throw new Error(
              `${msg} — workspace delete may be missing on the server. Rebuild hsm_console, or use POST …/workspace/file/delete / DELETE …/workspace/file from a current binary.`,
            );
          }
          throw new Error(msg);
        }
        if (openFilePath === relPath) {
          setOpenFilePath(null);
          setEditorContent("");
          setBaselineContent("");
          setSaveState("idle");
        }
        void loadList();
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setListError(msg);
        if (openFilePath === relPath) setFileError(msg);
      }
    },
    [apiBase, companyId, rootPrefix, loadList, openFilePath],
  );

  const deleteOpenFile = useCallback(async () => {
    if (!openFilePath) return;
    await deleteFileAtPath(openFilePath);
  }, [openFilePath, deleteFileAtPath]);

  const folderLabel = browsePath === rootPrefix ? "workspace" : browsePath.replace(/^.*\//, "") || browsePath;
  const selectedFileName = openFilePath?.split("/").pop() ?? null;
  const canPreview = isTextRenderable(openFilePath);
  const filteredEntries = useMemo(() => {
    const q = filterQuery.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((e) => e.name.toLowerCase().includes(q) || e.path.toLowerCase().includes(q));
  }, [entries, filterQuery]);
  const fileMeta = useMemo(
    () => entries.find((entry) => entry.path === openFilePath) ?? null,
    [entries, openFilePath],
  );

  return (
    <div className="space-y-6">
      <AgentWorkspaceTaskHistory
        apiBase={apiBase}
        companyId={companyId}
        agentPersonaName={agentPackName}
        issueKeyPrefix={issueKeyPrefix}
      />

      <WorkspaceNewIssueDialog
        open={issueOpen}
        onOpenChange={setIssueOpen}
        apiBase={apiBase}
        companyId={companyId}
        workspacePaths={openFilePath ? [openFilePath] : []}
        assigneeDisplayName={assigneeDisplayName}
        assigneePersona={agentPackName}
      />

      <div className="rounded-2xl border border-admin-border bg-card/40 px-4 py-3">
        <p className="text-sm font-semibold text-foreground">Agent drive</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Files created by this agent are stored under <span className="font-mono text-foreground">{rootPrefix}</span>.
          Use review mode for a clean read-only document viewer.
        </p>
      </div>

      {hsmiiHomeHint ? (
        <p className="font-mono text-[10px] text-muted-foreground truncate" title={hsmiiHomeHint}>
          {hsmiiHomeHint}
        </p>
      ) : null}

      <p
        className="text-[11px] leading-snug text-muted-foreground font-mono truncate"
        title={browsePath}
      >
        {browsePath.split("/").filter(Boolean).join(" > ")}
      </p>

      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-admin-border pb-3">
        <div className="flex min-w-0 items-center gap-2 text-sm font-medium">
          <Folder className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
          <span className="truncate font-mono text-xs">{folderLabel}</span>
          <span className="truncate text-xs text-muted-foreground">({browsePath})</span>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            size="sm"
            variant={workspaceMode === "review" ? "default" : "outline"}
            onClick={() => {
              setWorkspaceMode("review");
              setReaderMode("preview");
            }}
          >
            Review mode
          </Button>
          <Button
            type="button"
            size="sm"
            variant={workspaceMode === "manage" ? "default" : "outline"}
            onClick={() => setWorkspaceMode("manage")}
          >
            Manage mode
          </Button>
          <Button type="button" variant="secondary" size="sm" onClick={goUp} disabled={browsePath === rootPrefix}>
            Up
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={goRootAgent}>
            Agent root
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={() => void loadList()} disabled={listLoading}>
            Refresh
          </Button>
          {workspaceMode === "manage" ? (
            <>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled
                title="Binary upload is not implemented in company-console yet"
              >
                Upload
              </Button>
              <Button type="button" variant="outline" size="sm" onClick={() => void newFolder()}>
                <FolderPlus className="mr-1 h-3.5 w-3.5" aria-hidden />
                New folder
              </Button>
              <Button type="button" size="sm" onClick={() => void newFile()}>
                New file
              </Button>
            </>
          ) : null}
        </div>
      </div>

      <Input
        value={filterQuery}
        onChange={(ev) => setFilterQuery(ev.target.value)}
        placeholder="Filter files by name or path…"
        className="max-w-md font-mono text-xs"
      />

      {listError ? <p className="text-sm text-destructive">{listError}</p> : null}

      <div className="grid gap-4 xl:grid-cols-[minmax(0,0.92fr)_minmax(0,1.08fr)]">
        <div className="rounded-2xl border border-admin-border bg-black/10">
          <div className="flex gap-2 border-b border-admin-border bg-muted/30 px-3 py-2 font-mono text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
            <div className="grid min-w-0 flex-1 grid-cols-[1fr_88px_100px] gap-2">
              <span>Name</span>
              <span className="text-right">Size</span>
              <span>Modified</span>
            </div>
            <div className="flex w-10 shrink-0 items-center justify-center text-[9px]" title="Remove file">
              Del
            </div>
          </div>
          <ScrollArea className="h-[min(52vh,480px)]">
            {listLoading ? (
              <div className="space-y-2 p-3">
                <Skeleton className="h-8 w-full" />
                <Skeleton className="h-8 w-full" />
              </div>
            ) : filteredEntries.length === 0 ? (
              <div className="space-y-2 p-4 text-sm text-muted-foreground">
                <p className="font-medium text-foreground">No files here yet</p>
                <ul className="list-inside list-disc space-y-1 text-xs">
                  <li>
                    Company record has <span className="font-mono">hsmii_home</span> pointing at your Paperclip
                    pack root (the folder that contains <span className="font-mono">agents/</span>).
                  </li>
                  <li>
                    On disk, this agent&apos;s folder must be{" "}
                    <span className="font-mono text-foreground">{rootPrefix}</span> (matches roster{" "}
                    <span className="font-mono">company_agents.name</span>).
                  </li>
                  <li>
                    Run <span className="font-mono">POST …/import-paperclip-home</span> if agents were never
                    imported from the pack.
                  </li>
                </ul>
              </div>
            ) : (
              <ul>
                {filteredEntries.map((e) => (
                  <li
                    key={e.path}
                    className="flex items-stretch gap-2 border-b border-admin-border/60 px-3 py-2 last:border-b-0"
                  >
                    <button
                      type="button"
                      className="grid min-w-0 flex-1 grid-cols-[1fr_88px_100px] gap-2 py-0.5 text-left text-sm hover:bg-muted/50"
                      onClick={() => {
                        if (e.kind === "dir") {
                          setBrowsePath(e.path);
                          setOpenFilePath(null);
                          setEditorContent("");
                          setBaselineContent("");
                        } else {
                          void openFile(e.path);
                        }
                      }}
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        {e.kind === "dir" ? (
                          <Folder className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
                        ) : (
                          <File className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
                        )}
                        <span className="truncate font-mono text-xs">{e.name}</span>
                      </span>
                      <span className="text-right font-mono text-[11px] text-muted-foreground">
                        {e.kind === "dir" ? "—" : formatBytes(e.size_bytes)}
                      </span>
                      <span className="font-mono text-[11px] text-muted-foreground">{timeAgo(e.modified_at)}</span>
                    </button>
                    <div className="flex w-10 shrink-0 items-center justify-center">
                      {workspaceMode === "manage" && e.kind === "file" ? (
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon-xs"
                          className="text-muted-foreground hover:text-destructive"
                          aria-label={`Delete ${e.name}`}
                          title="Delete file"
                          onClick={(ev) => {
                            ev.preventDefault();
                            ev.stopPropagation();
                            void deleteFileAtPath(e.path);
                          }}
                        >
                          <Trash2 className="h-3.5 w-3.5" aria-hidden />
                        </Button>
                      ) : null}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </ScrollArea>
        </div>

        <div className="flex min-h-[420px] flex-col gap-3">
          {openFilePath ? (
            <>
              <div className="rounded-2xl border border-admin-border bg-black/10 p-3">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="inline-flex items-center gap-2 rounded-full border border-admin-border bg-card px-3 py-1.5">
                      <File className="h-3.5 w-3.5 text-muted-foreground" />
                      <span className="max-w-[260px] truncate font-medium text-foreground">{selectedFileName}</span>
                    </div>
                    <p className="mt-2 break-all font-mono text-[10px] text-muted-foreground">{openFilePath}</p>
                    <div className="mt-2 flex flex-wrap gap-3 font-mono text-[10px] text-muted-foreground">
                      <span>{fileMeta ? formatBytes(fileMeta.size_bytes) : "—"}</span>
                      <span>{fileMeta?.modified_at ? timeAgo(fileMeta.modified_at) : "—"}</span>
                      <span>{dirty ? "Unsaved changes" : "Saved to disk"}</span>
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      size="sm"
                      variant={readerMode === "preview" ? "default" : "outline"}
                      disabled={!canPreview}
                      onClick={() => setReaderMode("preview")}
                    >
                      <Eye className="mr-1.5 h-3.5 w-3.5" />
                      Preview
                    </Button>
                    {workspaceMode === "manage" ? (
                      <Button
                        type="button"
                        size="sm"
                        variant={readerMode === "edit" ? "default" : "outline"}
                        onClick={() => setReaderMode("edit")}
                      >
                        <PencilLine className="mr-1.5 h-3.5 w-3.5" />
                        Edit
                      </Button>
                    ) : null}
                  </div>
                </div>
              </div>
              {fileLoading ? (
                <Skeleton className="min-h-[320px] w-full rounded-2xl" />
              ) : fileError ? (
                <p className="text-sm text-destructive">{fileError}</p>
              ) : readerMode === "edit" && workspaceMode === "manage" ? (
                <Textarea
                  className="min-h-[340px] flex-1 rounded-2xl border-admin-border bg-card font-mono text-xs"
                  value={editorContent}
                  onChange={(ev) => {
                    setEditorContent(ev.target.value);
                    setSaveState("idle");
                  }}
                  spellCheck={false}
                />
              ) : (
                <div className="min-h-[340px] rounded-[28px] border border-[#d7cfbf]/80 bg-[#f8f4ea] shadow-[0_1px_0_rgba(255,255,255,0.55)_inset]">
                  <div className="border-b border-[#ddd4c4] px-5 py-3">
                    <p className="text-sm font-semibold text-[#1f1a14]">{selectedFileName}</p>
                    <p className="mt-1 text-[11px] text-[#6f6658]">
                      Minimal document viewer for reviewing agent-created files.
                    </p>
                  </div>
                  <ScrollArea className="h-[min(60vh,560px)]">
                    <div className="px-5 py-5 text-[#211d17]">
                      {isMarkdownPath(openFilePath) ? (
                        <article className="prose prose-sm max-w-none prose-headings:text-[#1f1a14] prose-p:text-[#3c352c] prose-strong:text-[#1f1a14] prose-code:text-[#6f3f15] prose-pre:bg-[#ece4d7] prose-pre:text-[#2c241b] prose-li:text-[#3c352c]">
                          <ReactMarkdown remarkPlugins={[remarkGfm]}>{editorContent}</ReactMarkdown>
                        </article>
                      ) : (
                        <pre className="whitespace-pre-wrap font-sans text-[14px] leading-7 text-[#2f281f]">
                          {editorContent}
                        </pre>
                      )}
                    </div>
                  </ScrollArea>
                </div>
              )}
              <div className="flex flex-wrap items-center gap-2">
                {workspaceMode === "manage" ? (
                  <>
                    <Button
                      type="button"
                      size="sm"
                      variant="secondary"
                      disabled={!openFilePath || fileLoading}
                      title={!openFilePath ? "Select a file in the list first" : undefined}
                      onClick={() => setIssueOpen(true)}
                    >
                      Create issue
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="destructive"
                      disabled={!openFilePath || fileLoading}
                      title={!openFilePath ? "Select a file first" : "Remove file from disk under hsmii_home"}
                      onClick={() => void deleteOpenFile()}
                    >
                      <Trash2 className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                      Delete file
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      disabled={!openFilePath || fileLoading || saveState === "saving" || !dirty}
                      onClick={() => void saveFile()}
                    >
                      {saveState === "saving" ? "Saving…" : "Save"}
                    </Button>
                    {saveState === "saved" ? (
                      <span className="text-xs text-emerald-600 dark:text-emerald-400">Saved</span>
                    ) : null}
                    {saveState === "error" ? (
                      <span className="text-xs text-destructive">Save failed</span>
                    ) : null}
                  </>
                ) : (
                  <span className="text-xs text-muted-foreground">
                    Review mode is read-only. Switch to Manage mode to edit, delete, or create files.
                  </span>
                )}
              </div>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">Select a file to view and edit (UTF-8 text).</p>
          )}
        </div>
      </div>
    </div>
  );
}
