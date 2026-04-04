"use client";

import { useEffect, useMemo, useRef, useState } from "react";

type CompanySkillTemplate = {
  id: string;
  slug: string;
  name: string;
  description: string;
  body: string;
  skill_path: string;
  source: string;
  updated_at: string;
};

type Props = {
  api: string;
  companyId: string;
  setCoErr: (msg: string | null) => void;
  focusSkillSlug?: string | null;
  focusSkillNonce?: number;
};

function normalizeSkillRef(raw: string): string {
  const value = raw.trim();
  if (!value) return "";
  const pathMatch = value.match(/skills\/([^/]+)\/SKILL\.md$/i);
  if (pathMatch) return pathMatch[1].trim();
  return value.replace(/^skills\//i, "").replace(/\/SKILL\.md$/i, "").replace(/\/+$/g, "").trim();
}

export function CompanySkillsPanel({
  api,
  companyId,
  setCoErr,
  focusSkillSlug = null,
  focusSkillNonce,
}: Props) {
  const [skills, setSkills] = useState<CompanySkillTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const itemRefs = useRef<Record<string, HTMLLIElement | null>>({});

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      try {
        const r = await fetch(`${api}/api/company/companies/${companyId}/skills`);
        const j = (await r.json()) as { skills?: CompanySkillTemplate[]; error?: string };
        if (!r.ok) {
          throw new Error(j.error ?? r.statusText);
        }
        if (cancelled) return;
        setSkills(Array.isArray(j.skills) ? j.skills : []);
        setExpandedId((cur) =>
          cur && (j.skills ?? []).some((skill) => skill.id === cur) ? cur : null
        );
      } catch (e) {
        if (!cancelled) {
          setSkills([]);
          setExpandedId(null);
          setCoErr(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [api, companyId, setCoErr]);

  const updatedLabel = useMemo(() => {
    if (skills.length === 0) return null;
    const latest = [...skills]
      .map((skill) => Date.parse(skill.updated_at))
      .filter((value) => Number.isFinite(value))
      .sort((a, b) => b - a)[0];
    if (!latest) return null;
    return new Date(latest).toLocaleString();
  }, [skills]);

  useEffect(() => {
    const targetSlug = normalizeSkillRef(focusSkillSlug ?? "");
    if (!targetSlug || skills.length === 0) return;
    const match = skills.find((skill) => normalizeSkillRef(skill.slug) === targetSlug);
    if (!match) return;
    setExpandedId(match.id);
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        itemRefs.current[match.id]?.scrollIntoView({ behavior: "smooth", block: "center" });
      });
    });
  }, [focusSkillNonce, focusSkillSlug, skills]);

  return (
    <details className="mb-6 rounded-lg border border-line bg-panel" open>
      <summary className="cursor-pointer list-none px-4 py-3 text-sm font-medium text-gray-200 marker:content-none [&::-webkit-details-marker]:hidden">
        <span className="text-gray-400">▸</span> Imported skill templates{" "}
        <span className="font-normal text-gray-500">
          (ready from <code className="text-xs text-accent">skills/&lt;slug&gt;/SKILL.md</code>)
        </span>
      </summary>
      <div className="space-y-4 border-t border-line px-4 py-4">
        <p className="text-sm leading-relaxed text-gray-500">
          Paperclip and <strong className="text-gray-400">companies.sh</strong> imports save each pack skill as a local
          template for this workspace. Use these when shaping new agents, copying playbooks, or checking what the pack
          brought in alongside the roster.
        </p>

        <div className="flex flex-wrap items-center gap-2 text-xs text-gray-600">
          <span>{loading ? "Refreshing templates…" : `${skills.length} template${skills.length === 1 ? "" : "s"}`}</span>
          {updatedLabel ? <span>Latest import {updatedLabel}</span> : null}
        </div>

        {skills.length === 0 ? (
          <div className="rounded-lg border border-dashed border-line/80 bg-black/20 px-4 py-3 text-sm text-gray-500">
            {loading
              ? "Loading imported skills…"
              : "No imported skill templates yet. Pick a companies.sh or Paperclip pack to populate this workspace."}
          </div>
        ) : (
          <ul className="space-y-3">
            {skills.map((skill) => {
              const open = expandedId === skill.id;
              const focused = normalizeSkillRef(skill.slug) === normalizeSkillRef(focusSkillSlug ?? "");
              return (
                <li
                  key={skill.id}
                  ref={(node) => {
                    itemRefs.current[skill.id] = node;
                  }}
                  className={`rounded-lg border bg-ink/30 ${focused ? "border-accent/70 ring-1 ring-accent/40" : "border-line"}`}
                >
                  <button
                    type="button"
                    className="flex w-full items-start justify-between gap-3 px-3 py-3 text-left"
                    onClick={() => setExpandedId((cur) => (cur === skill.id ? null : skill.id))}
                  >
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium text-white">{skill.name || skill.slug}</span>
                        <span className="rounded border border-line/80 px-1.5 py-px font-mono text-[10px] uppercase tracking-wide text-accent">
                          {skill.slug}
                        </span>
                      </div>
                      <p className="mt-1 text-xs text-gray-500">
                        {skill.skill_path} · {skill.source}
                      </p>
                      {skill.description ? (
                        <p className="mt-2 text-sm leading-relaxed text-gray-400">{skill.description}</p>
                      ) : null}
                    </div>
                    <span className="shrink-0 text-xs text-gray-500">{open ? "Hide" : "Open"}</span>
                  </button>
                  {open ? (
                    <div className="border-t border-line/60 px-3 py-3">
                      <div className="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-gray-500">
                        <span className="rounded bg-white/5 px-1.5 py-0.5 font-mono">{skill.skill_path}</span>
                        <span>Imported {new Date(skill.updated_at).toLocaleString()}</span>
                      </div>
                      <pre className="max-h-[320px] overflow-auto rounded-lg border border-line bg-black/30 px-3 py-3 font-mono text-xs leading-relaxed text-gray-300">
                        <code>{skill.body || "_No body content in SKILL.md_"}</code>
                      </pre>
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </details>
  );
}
