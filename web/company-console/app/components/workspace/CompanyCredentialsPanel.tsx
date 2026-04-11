"use client";

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, KeyRound, PlugZap } from "lucide-react";
import { Badge } from "@/app/components/ui/badge";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Input } from "@/app/components/ui/input";
import { Textarea } from "@/app/components/ui/textarea";
import type { HsmCompanyCredential } from "@/app/lib/hsm-api-types";
import { companyOsUrl } from "@/app/lib/company-api-url";

type ProviderPreset = {
  key: string;
  label: string;
  envVar: string;
  description: string;
  category: string;
};

export const PROVIDER_PRESETS: ProviderPreset[] = [
  { key: "openrouter", label: "OpenRouter", envVar: "OPENROUTER_API_KEY", description: "Unified LLM gateway — qwen, mistral, gemini, and 200+ models via one key.", category: "AI" },
  { key: "openai", label: "OpenAI", envVar: "OPENAI_API_KEY", description: "Reasoning, embeddings, structured LLM tasks.", category: "AI" },
  { key: "anthropic", label: "Anthropic", envVar: "ANTHROPIC_API_KEY", description: "Long-form agent work and structured operator flows.", category: "AI" },
  { key: "github", label: "GitHub", envVar: "GITHUB_TOKEN", description: "Repo, PR, issue, and CI automation.", category: "Dev" },
  { key: "slack", label: "Slack", envVar: "SLACK_BOT_TOKEN", description: "Notifications, triage, and operator escalations.", category: "Comms" },
  { key: "gmail", label: "Gmail Business", envVar: "GMAIL_OAUTH_TOKEN", description: "Business inbox triage, draft replies, and owner-confirm send flows.", category: "Comms" },
  { key: "microsoft_graph", label: "Microsoft 365 Mail", envVar: "M365_GRAPH_TOKEN", description: "Outlook/Exchange inbox automation with human approval.", category: "Comms" },
  { key: "imap_smtp", label: "IMAP/SMTP", envVar: "IMAP_SMTP_PASSWORD", description: "Generic business email mailbox for draft-first response handling.", category: "Comms" },
  { key: "notion", label: "Notion", envVar: "NOTION_API_KEY", description: "Docs, wiki, CRM, and SOP sync.", category: "Knowledge" },
  { key: "linear", label: "Linear", envVar: "LINEAR_API_KEY", description: "Roadmap, issue sync, and eng execution.", category: "PM" },
  { key: "google_ads", label: "Google Ads", envVar: "GOOGLE_ADS_API_KEY", description: "Campaign retrieval and spend operations.", category: "Growth" },
  { key: "meta_ads", label: "Meta Ads", envVar: "META_ACCESS_TOKEN", description: "Creative, campaign, and ad-account reporting.", category: "Growth" },
  { key: "youtube", label: "YouTube", envVar: "YOUTUBE_API_KEY", description: "Video metadata extraction and channel ops automation.", category: "Growth" },
  { key: "vimeo", label: "Vimeo", envVar: "VIMEO_ACCESS_TOKEN", description: "Video library extraction and publishing workflows.", category: "Growth" },
  { key: "tiktok_ads", label: "TikTok Ads", envVar: "TIKTOK_ADS_TOKEN", description: "Creative performance workflows and ads operations.", category: "Growth" },
  { key: "stripe", label: "Stripe", envVar: "STRIPE_SECRET_KEY", description: "Revenue, subscriptions, refunds, and payment ops.", category: "Finance" },
  { key: "firecrawl", label: "Firecrawl", envVar: "FIRECRAWL_API_KEY", description: "Web extraction for market and competitor research.", category: "Research" },
  { key: "browserbase", label: "Browserbase", envVar: "BROWSERBASE_API_KEY", description: "Cloud browser automation sessions for tool-driven workflows.", category: "Research" },
  { key: "browser_use", label: "Browser Use", envVar: "BROWSER_USE_API_KEY", description: "High-level browser task provider for managed cloud browsing.", category: "Research" },
  { key: "tavily", label: "TAVILY", envVar: "TAVILY_API_KEY", description: "Search + web retrieval for external intelligence.", category: "Research" },
  { key: "xai", label: "xAI", envVar: "XAI_API_KEY", description: "xAI model access with optional prompt-cache and thinking prefill controls.", category: "AI" },
  { key: "resend", label: "Resend", envVar: "RESEND_API_KEY", description: "Transactional comms and outbound operator mail.", category: "Comms" },
];

export function CompanyCredentialsPanel({
  apiBase,
  companyId,
  credentials,
}: {
  apiBase: string;
  companyId: string;
  credentials: HsmCompanyCredential[];
}) {
  const qc = useQueryClient();
  const [selectedKey, setSelectedKey] = useState<string>(PROVIDER_PRESETS[0]?.key ?? "openai");
  const [secretValue, setSecretValue] = useState("");
  const [notes, setNotes] = useState("");
  const selectedProvider = PROVIDER_PRESETS.find((provider) => provider.key === selectedKey) ?? PROVIDER_PRESETS[0];
  const existing = credentials.find((credential) => credential.provider_key === selectedKey) ?? null;

  function formatCredentialError(raw: unknown): string {
    const msg = raw instanceof Error ? raw.message : String(raw ?? "unknown error");
    if (msg.includes("404")) {
      return "Credentials API is not available on the running hsm_console yet. Rebuild/restart backend and refresh.";
    }
    return msg;
  }

  const saveCredential = useMutation({
    mutationFn: async () => {
      if (!selectedProvider || !secretValue.trim()) throw new Error("API key required");
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/credentials`), {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          provider_key: selectedProvider.key,
          label: selectedProvider.label,
          env_var: selectedProvider.envVar,
          secret_value: secretValue.trim(),
          notes: notes.trim() || undefined,
        }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setSecretValue("");
      void qc.invalidateQueries({ queryKey: ["hsm", "company-credentials", apiBase, companyId] });
    },
  });

  const deleteCredential = useMutation({
    mutationFn: async () => {
      if (!selectedProvider) throw new Error("provider missing");
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/credentials`), {
        method: "DELETE",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ provider_key: selectedProvider.key }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      return j;
    },
    onSuccess: () => {
      setSecretValue("");
      setNotes("");
      void qc.invalidateQueries({ queryKey: ["hsm", "company-credentials", apiBase, companyId] });
    },
  });

  return (
    <Card className="border-admin-border bg-card/80">
      <CardHeader className="pb-3">
        <div className="flex flex-wrap items-center gap-2">
          <CardTitle className="text-base">Credentials</CardTitle>
          <Badge variant="outline" className="font-mono text-[10px]">
            {credentials.length} connected
          </Badge>
        </div>
        <CardDescription>
          Connect the API keys your operators and MCP-style tools need to run the company.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4 lg:grid-cols-[1.15fr_0.85fr]">
        <div className="grid gap-2 sm:grid-cols-2">
          {PROVIDER_PRESETS.map((provider) => {
            const credential = credentials.find((item) => item.provider_key === provider.key);
            const active = provider.key === selectedKey;
            return (
              <button
                key={provider.key}
                type="button"
                onClick={() => {
                  setSelectedKey(provider.key);
                  setNotes(credential?.notes ?? "");
                  setSecretValue("");
                }}
                className={`rounded-2xl border px-3 py-3 text-left transition-colors ${
                  active
                    ? "border-primary/60 bg-primary/10"
                    : "border-admin-border bg-black/10 hover:bg-white/5"
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span className="inline-flex h-8 w-8 items-center justify-center rounded-full bg-white/5 text-muted-foreground">
                      <PlugZap className="h-4 w-4" />
                    </span>
                    <div>
                      <p className="text-sm font-medium text-foreground">{provider.label}</p>
                      <p className="font-mono text-[10px] text-muted-foreground">{provider.envVar}</p>
                    </div>
                  </div>
                  {credential ? (
                    <CheckCircle2 className="h-4 w-4 text-emerald-400" />
                  ) : (
                    <KeyRound className="h-4 w-4 text-muted-foreground" />
                  )}
                </div>
                <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">{provider.description}</p>
              </button>
            );
          })}
        </div>

        <div className="rounded-2xl border border-admin-border bg-black/15 p-4">
          <div className="flex items-start justify-between gap-2">
            <div>
              <p className="text-sm font-medium text-foreground">{selectedProvider.label}</p>
              <p className="mt-1 text-[11px] text-muted-foreground">{selectedProvider.description}</p>
            </div>
            <Badge variant={existing ? "default" : "outline"} className="text-[10px]">
              {existing ? "Connected" : "Not connected"}
            </Badge>
          </div>
          <div className="mt-4 space-y-3">
            <div className="rounded-xl border border-admin-border/80 bg-black/10 p-3">
              <p className="font-mono text-[10px] uppercase tracking-wide text-muted-foreground">Environment variable</p>
              <p className="mt-1 font-mono text-xs text-foreground">{selectedProvider.envVar}</p>
              {existing ? (
                <p className="mt-2 text-[11px] text-muted-foreground">
                  Stored as {existing.masked_preview}
                  {existing.notes ? ` · ${existing.notes}` : ""}
                </p>
              ) : (
                <p className="mt-2 text-[11px] text-muted-foreground">No saved key for this workspace yet.</p>
              )}
            </div>
            <Input
              className="h-9 border-admin-border bg-black/20 font-mono text-xs"
              placeholder={`Paste ${selectedProvider.label} key`}
              value={secretValue}
              onChange={(e) => setSecretValue(e.target.value)}
            />
            <Textarea
              className="min-h-20 border-admin-border bg-black/20 text-xs"
              placeholder="Optional notes: account, region, owner, or MCP mapping"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
            />
            <div className="flex flex-wrap gap-2">
              <Button size="sm" disabled={!secretValue.trim() || saveCredential.isPending} onClick={() => saveCredential.mutate()}>
                {saveCredential.isPending ? "Saving..." : existing ? "Update credential" : "Connect credential"}
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={!existing || deleteCredential.isPending}
                onClick={() => deleteCredential.mutate()}
              >
                {deleteCredential.isPending ? "Removing..." : "Disconnect"}
              </Button>
            </div>
            {saveCredential.isError ? (
              <p className="text-[11px] text-destructive">
                {formatCredentialError(saveCredential.error)}
              </p>
            ) : null}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
