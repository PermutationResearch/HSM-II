"use client";

import { useMemo, useState } from "react";
import { ChevronDown, Loader2, Send, Store } from "lucide-react";

import { Button } from "@/app/components/ui/button";
import { Input } from "@/app/components/ui/input";
import { Label } from "@/app/components/ui/label";
import { Textarea } from "@/app/components/ui/textarea";
import { cn } from "@/app/lib/utils";
import {
  companiesShInstallPath,
  isPaperclipPack,
  type CompaniesShItem,
} from "../../ui/src/hooks/useCompaniesShCatalog";

type Props = {
  items: CompaniesShItem[];
  loading: boolean;
  error: string | null;
  postgresConfigured: boolean;
  onCreateFromCatalog: (item: CompaniesShItem) => Promise<void>;
  setCoErr: (msg: string | null) => void;
};

export function PackMarketplacePanel({
  items,
  loading,
  error,
  postgresConfigured,
  onCreateFromCatalog,
  setCoErr,
}: Props) {
  const [scope, setScope] = useState<"all" | "paperclip">("paperclip");
  const [q, setQ] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [adding, setAdding] = useState<string | null>(null);

  const [ideaTitle, setIdeaTitle] = useState("");
  const [ideaSummary, setIdeaSummary] = useState("");
  const [ideaContact, setIdeaContact] = useState("");
  const [ideaLink, setIdeaLink] = useState("");
  const [ideaStatus, setIdeaStatus] = useState<string | null>(null);
  const [ideaSending, setIdeaSending] = useState(false);

  const filtered = useMemo(() => {
    const scoped = scope === "paperclip" ? items.filter(isPaperclipPack) : items;
    const t = q.trim().toLowerCase();
    if (!t) return scoped;
    return scoped.filter((it) => {
      const hay = `${it.name} ${it.slug} ${it.repo} ${it.tagline ?? ""} ${it.description ?? ""} ${it.category ?? ""}`.toLowerCase();
      return hay.includes(t);
    });
  }, [items, scope, q]);

  const submitIdea = async () => {
    setIdeaStatus(null);
    setIdeaSending(true);
    try {
      const r = await fetch("/api/companies-sh/submit-idea", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          title: ideaTitle,
          summary: ideaSummary,
          contact: ideaContact,
          link: ideaLink,
        }),
      });
      const j = (await r.json()) as {
        accepted?: boolean;
        error?: string;
        message?: string;
        fallback_url?: string;
        stored?: string;
      };
      if (!r.ok) {
        setIdeaStatus(j.error ?? `HTTP ${r.status}`);
        return;
      }
      if (j.accepted) {
        setIdeaStatus(`Recorded. Operators can review: ${j.stored ?? "log file"}.`);
        setIdeaTitle("");
        setIdeaSummary("");
        setIdeaContact("");
        setIdeaLink("");
        return;
      }
      if (j.fallback_url) {
        setIdeaStatus(j.message ?? "Open the contributing guide to propose your pack for the public directory.");
        window.open(j.fallback_url, "_blank", "noopener,noreferrer");
      } else {
        setIdeaStatus(j.message ?? "Could not store submission.");
      }
    } catch (e) {
      setIdeaStatus(e instanceof Error ? e.message : String(e));
    } finally {
      setIdeaSending(false);
    }
  };

  return (
    <div className="space-y-8">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <p className="flex items-center gap-2 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-[#8B949E]">
            <Store className="h-4 w-4 text-[#58a6ff]/90" aria-hidden />
            Pack marketplace
          </p>
          <h2 className="mt-1 text-base font-semibold text-white">What each template is for</h2>
          <p className="mt-2 max-w-3xl text-sm leading-relaxed text-gray-500">
            Browse the same directory as the workspace picker. Each card summarizes the pack from{" "}
            <a className="text-accent hover:underline" href="https://companies.sh/" target="_blank" rel="noreferrer">
              companies.sh
            </a>
            . Add a workspace to install into Company OS (with{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">HSM_COMPANY_PACK_INSTALL_ROOT</code>, the real{" "}
            <code className="rounded bg-white/5 px-1 font-mono text-[11px]">npx companies.sh add</code> flow runs, then
            agents import from disk).
          </p>
        </div>
        <div className="flex flex-col gap-2 sm:items-end">
          <div className="flex flex-wrap gap-1">
            <Button
              type="button"
              size="sm"
              variant={scope === "paperclip" ? "default" : "outline"}
              className={cn(scope !== "paperclip" && "border-line")}
              onClick={() => setScope("paperclip")}
            >
              Paperclip packs
            </Button>
            <Button
              type="button"
              size="sm"
              variant={scope === "all" ? "default" : "outline"}
              className={cn(scope !== "all" && "border-line")}
              onClick={() => setScope("all")}
            >
              All directory
            </Button>
          </div>
          <Input
            className="w-full min-w-[200px] max-w-xs border-line bg-ink sm:w-64"
            placeholder="Search name, category, description…"
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
      </div>

      {loading ? (
        <p className="text-sm text-gray-500">Loading directory…</p>
      ) : error ? (
        <p className="text-sm text-amber-200/90">{error}</p>
      ) : (
        <ul className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {filtered.slice(0, 120).map((it) => {
            const key = `${it.repo}/${it.slug}`;
            const open = expanded === key;
            const path = companiesShInstallPath(it);
            const githubBrowseUrl = `https://github.com/${it.repo}/tree/main/${it.slug}`;
            const pc = isPaperclipPack(it);
            return (
              <li
                key={key}
                className="flex flex-col rounded-xl border border-line bg-panel p-4 shadow-sm ring-1 ring-white/5"
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-medium text-white">{it.name}</span>
                      {pc ? (
                        <span className="rounded border border-[#58a6ff]/35 bg-[#58a6ff]/10 px-1.5 py-px font-mono text-[9px] font-semibold uppercase tracking-wide text-[#58a6ff]">
                          Paperclip
                        </span>
                      ) : null}
                    </div>
                    {it.tagline ? (
                      <p className="mt-1 text-sm leading-snug text-gray-400">{it.tagline}</p>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    className="shrink-0 rounded border border-line px-2 py-1 text-[11px] text-gray-400 hover:bg-white/5 hover:text-white"
                    onClick={() => setExpanded(open ? null : key)}
                    aria-expanded={open}
                  >
                    <ChevronDown className={cn("inline h-3.5 w-3.5 transition-transform", open && "rotate-180")} />
                    <span className="sr-only">Toggle details</span>
                  </button>
                </div>
                <p className="mt-2 font-mono text-[10px] uppercase tracking-wide text-gray-600">{path}</p>
                <div className="mt-1 flex flex-wrap gap-2 text-[11px] text-gray-500">
                  {it.category ? <span className="rounded bg-white/5 px-1.5 py-0.5">{it.category}</span> : null}
                  {it.installs ? <span>{it.installs} installs</span> : null}
                  {it.githubStars ? <span>★ {it.githubStars}</span> : null}
                </div>
                {it.techStack && it.techStack.length > 0 ? (
                  <p className="mt-2 text-[11px] text-gray-500">
                    Stack: {it.techStack.slice(0, 6).join(" · ")}
                    {it.techStack.length > 6 ? "…" : ""}
                  </p>
                ) : null}
                {open && it.description ? (
                  <p className="mt-3 border-t border-line/60 pt-3 text-sm leading-relaxed text-gray-400">{it.description}</p>
                ) : null}
                <div className="mt-4 flex flex-wrap gap-2 border-t border-line/40 pt-3">
                  <Button
                    type="button"
                    size="sm"
                    disabled={!postgresConfigured || adding === key}
                    onClick={async () => {
                      if (!postgresConfigured) {
                        setCoErr("Configure PostgreSQL for Company OS before adding a workspace from a pack.");
                        return;
                      }
                      setAdding(key);
                      setCoErr(null);
                      try {
                        await onCreateFromCatalog(it);
                      } finally {
                        setAdding(null);
                      }
                    }}
                  >
                    {adding === key ? (
                      <>
                        <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
                        Adding…
                      </>
                    ) : (
                      "Add workspace from pack"
                    )}
                  </Button>
                  <a
                    className="inline-flex items-center text-xs text-accent hover:underline"
                    href={githubBrowseUrl}
                    target="_blank"
                    rel="noreferrer"
                  >
                    View on GitHub
                  </a>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {!loading && !error && filtered.length > 120 ? (
        <p className="text-xs text-gray-500">Showing 120 of {filtered.length} — refine search.</p>
      ) : null}

      <div className="rounded-xl border border-dashed border-line bg-black/20 p-5">
        <h3 className="text-sm font-semibold text-white">Propose a new company pack</h3>
        <p className="mt-1 max-w-3xl text-xs leading-relaxed text-gray-500">
          Describe a template you would like listed (industry, agents, workflows). If this server has{" "}
          <code className="rounded bg-white/5 px-1 font-mono text-[10px]">HSM_COMPANY_PACK_SUBMISSIONS_DIR</code>, your note
          is appended for operators. Otherwise we open the{" "}
          <a
            className="text-accent hover:underline"
            href="https://github.com/paperclipai/companies/blob/main/CONTRIBUTING.md"
            target="_blank"
            rel="noreferrer"
          >
            Paperclip contributing guide
          </a>{" "}
          so you can follow the upstream process for the public directory.
        </p>
        <div className="mt-4 grid max-w-xl gap-3">
          <div className="space-y-1">
            <Label className="text-xs text-gray-400">Working title</Label>
            <Input
              className="border-line bg-ink"
              value={ideaTitle}
              onChange={(e) => setIdeaTitle(e.target.value)}
              placeholder="e.g. Boutique hotel operations"
            />
          </div>
          <div className="space-y-1">
            <Label className="text-xs text-gray-400">What it should do</Label>
            <Textarea
              className="min-h-[100px] border-line bg-ink text-sm"
              value={ideaSummary}
              onChange={(e) => setIdeaSummary(e.target.value)}
              placeholder="Roles, skills, example workflows, integrations…"
            />
          </div>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <Label className="text-xs text-gray-400">Contact (optional)</Label>
              <Input
                className="border-line bg-ink"
                value={ideaContact}
                onChange={(e) => setIdeaContact(e.target.value)}
                placeholder="email or @handle"
              />
            </div>
            <div className="space-y-1">
              <Label className="text-xs text-gray-400">Link (optional)</Label>
              <Input
                className="border-line bg-ink"
                value={ideaLink}
                onChange={(e) => setIdeaLink(e.target.value)}
                placeholder="Brief, repo, or doc URL"
              />
            </div>
          </div>
          <Button type="button" size="sm" disabled={ideaSending} onClick={() => void submitIdea()} className="w-fit gap-1">
            {ideaSending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Send className="h-4 w-4" />}
            Send idea
          </Button>
          {ideaStatus ? <p className="text-xs text-gray-400">{ideaStatus}</p> : null}
        </div>
      </div>
    </div>
  );
}
