"use client";

import { Cable, ShieldCheck } from "lucide-react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/app/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/app/components/ui/card";
import { Input } from "@/app/components/ui/input";
import { Skeleton } from "@/app/components/ui/skeleton";
import { useWorkspace } from "@/app/context/WorkspaceContext";
import { CompanyCredentialsPanel, PROVIDER_PRESETS } from "@/app/components/workspace/CompanyCredentialsPanel";
import { companyOsUrl } from "@/app/lib/company-api-url";
import { useBrowserProviders, useCompanyConnectors, useCompanyCredentials, useCompanyProfile, useConnectorTemplates, useWorkflowPacks } from "@/app/lib/hsm-queries";
import { useState } from "react";

export default function WorkspaceCredentialsPage() {
  const qc = useQueryClient();
  const { apiBase, companyId } = useWorkspace();
  const [openapiUrl, setOpenapiUrl] = useState("");
  const [templateKey, setTemplateKey] = useState("github");
  const { data: credentials = [], isLoading, error } = useCompanyCredentials(apiBase, companyId);
  const { data: browserProviders = [] } = useBrowserProviders(apiBase, companyId);
  const { data: connectors = [] } = useCompanyConnectors(apiBase, companyId);
  const { data: templates = [] } = useConnectorTemplates(apiBase, undefined, companyId);
  const { data: profile } = useCompanyProfile(apiBase, companyId);
  const { data: workflowPacks = [] } = useWorkflowPacks(apiBase, companyId);
  const refreshProfile = useMutation({
    mutationFn: async () => {
      if (!companyId) throw new Error("company missing");
      const r = await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/profile`), {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ infer_defaults: true }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hsm", "company-profile", apiBase, companyId] });
      void qc.invalidateQueries({ queryKey: ["hsm", "connector-templates", apiBase] });
      void qc.invalidateQueries({ queryKey: ["hsm", "workflow-packs", apiBase, companyId] });
    },
  });

  const importOpenApi = useMutation({
    mutationFn: async () => {
      if (!companyId || !openapiUrl.trim()) throw new Error("OpenAPI URL required");
      const r = await fetch(companyOsUrl(apiBase, "/api/company/connectors/openapi/import"), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          provider_key: "openapi",
          connector_key: `openapi_${Date.now()}`,
          spec_url: openapiUrl.trim(),
          max_operations: 24,
        }),
      });
      const j = (await r.json().catch(() => ({}))) as { error?: string; template?: { provider_key: string; connector_key: string } };
      if (!r.ok) throw new Error(j.error ?? `${r.status}`);
      if (j.template && companyId) {
        await fetch(companyOsUrl(apiBase, `/api/company/companies/${companyId}/connectors`), {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            connector_key: j.template.connector_key,
            label: `Imported ${j.template.connector_key}`,
            provider_key: j.template.provider_key,
            auth_mode: "api_key",
            policy: {},
            metadata: { source: "openapi_import", spec_url: openapiUrl.trim() },
          }),
        });
      }
    },
    onSuccess: () => {
      setOpenapiUrl("");
      void qc.invalidateQueries({ queryKey: ["hsm", "connectors", apiBase, companyId] });
    },
  });

  if (!companyId) {
    return <p className="pc-page-desc">Select a company in the header.</p>;
  }

  if (error) {
    return (
      <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-4 text-sm">
        {error instanceof Error ? error.message : String(error)}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="space-y-2 border-b border-admin-border pb-4">
        <p className="pc-page-eyebrow">Workspace</p>
        <h1 className="pc-page-title">Credentials</h1>
        <p className="max-w-3xl text-sm leading-relaxed text-muted-foreground">
          Manage the company-level API keys and connection secrets used by operators, agents, and MCP-style tools.
        </p>
        {profile ? (
          <div className="flex flex-wrap items-center gap-2">
            <p className="font-mono text-[11px] text-muted-foreground">
              Profile: {profile.business_model} · {profile.size_tier} · compliance {profile.compliance_level}
            </p>
            <Button size="xs" variant="outline" onClick={() => refreshProfile.mutate()} disabled={refreshProfile.isPending}>
              {refreshProfile.isPending ? "Refreshing…" : "Re-infer profile"}
            </Button>
          </div>
        ) : null}
      </div>

      {isLoading ? <Skeleton className="h-80 rounded-3xl" /> : <CompanyCredentialsPanel apiBase={apiBase} companyId={companyId} credentials={credentials} />}

      {!isLoading && credentials.length === 0 && browserProviders.length === 0 ? (
        <div className="rounded-xl border border-amber-500/35 bg-amber-500/10 px-4 py-3 text-xs text-amber-100">
          No credential rows returned yet. If this persists after refresh, restart/rebuild <span className="font-mono">hsm_console</span> so
          credentials endpoints are available.
        </div>
      ) : null}

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Cloud browser providers</CardTitle>
          <CardDescription>
            Firecrawl, Browser Use, Browserbase, and xAI controls surfaced for operator setup.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 md:grid-cols-2">
          {browserProviders.map((provider) => (
            <div key={provider.key} className="rounded-2xl border border-admin-border bg-black/10 px-4 py-3">
              <div className="flex items-center justify-between gap-2">
                <p className="text-sm font-medium text-foreground">{provider.label}</p>
                <span
                  className={`rounded-full px-2 py-0.5 text-[10px] font-medium ${
                    provider.configured
                      ? "bg-emerald-500/15 text-emerald-300"
                      : "bg-muted text-muted-foreground"
                  }`}
                >
                  {provider.configured ? "Configured" : "Missing key"}
                </span>
              </div>
              <p className="mt-1 font-mono text-[10px] text-muted-foreground">{provider.api_base}</p>
              {provider.credential_preview ? (
                <p className="mt-2 text-[11px] text-muted-foreground">Saved: {provider.credential_preview}</p>
              ) : null}
              {(provider.prompt_cache_enabled || provider.thinking_prefill_enabled) ? (
                <p className="mt-2 text-[11px] text-muted-foreground">
                  {provider.prompt_cache_enabled ? "Prompt cache on" : "Prompt cache off"} ·{" "}
                  {provider.thinking_prefill_enabled ? "Thinking prefill on" : "Thinking prefill off"}
                </p>
              ) : null}
            </div>
          ))}
        </CardContent>
      </Card>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Connector control plane</CardTitle>
          <CardDescription>Connect once, route auth and policy across MCP + REST tools.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-2 md:grid-cols-2">
            {connectors.map((connector) => (
              <div key={connector.id} className="rounded-xl border border-admin-border bg-black/10 px-3 py-2">
                <p className="text-sm font-medium text-foreground">{connector.label}</p>
                <p className="font-mono text-[10px] text-muted-foreground">
                  {connector.connector_key} · {connector.provider_key}
                </p>
                <p className="mt-1 text-[11px] text-muted-foreground">
                  status: {connector.status}
                  {connector.credential_provider_key ? ` · credential: ${connector.credential_provider_key}` : ""}
                </p>
              </div>
            ))}
          </div>
          {connectors.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              No connectors yet. Saving credentials auto-creates matching connectors for fast setup.
            </p>
          ) : null}
        </CardContent>
      </Card>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Adaptive workflow packs</CardTitle>
          <CardDescription>
            Packs are tailored by company size/type so onboarding stays fast for any company.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
          {workflowPacks.map((pack) => (
            <div key={pack.key} className="rounded-xl border border-admin-border bg-black/10 px-3 py-2">
              <p className="text-sm font-medium text-foreground">{pack.label}</p>
              <p className="mt-1 text-[11px] text-muted-foreground">
                risk {pack.default_risk} · automation {pack.automation_limit}
              </p>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Templates + OpenAPI import</CardTitle>
          <CardDescription>Bootstrap common SaaS connectors or import any OpenAPI surface.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap gap-2">
            {templates.map((template) => (
              <button
                key={template.key}
                type="button"
                className={`rounded-full border px-3 py-1 text-xs ${
                  templateKey === template.key ? "border-primary bg-primary/10 text-primary" : "border-admin-border text-muted-foreground"
                }`}
                onClick={() => setTemplateKey(template.key)}
              >
                {template.label} {template.recommendation === "must_have" ? "• must-have" : ""}
              </button>
            ))}
          </div>
          <div className="flex flex-wrap gap-2">
            <Input
              className="max-w-xl border-admin-border bg-black/20 font-mono text-xs"
              value={openapiUrl}
              onChange={(e) => setOpenapiUrl(e.target.value)}
              placeholder="https://api.example.com/openapi.json"
            />
            <Button size="sm" onClick={() => importOpenApi.mutate()} disabled={!openapiUrl.trim() || importOpenApi.isPending}>
              {importOpenApi.isPending ? "Importing..." : "Import OpenAPI"}
            </Button>
          </div>
          {importOpenApi.isError ? (
            <p className="text-xs text-destructive">{importOpenApi.error instanceof Error ? importOpenApi.error.message : "Import failed"}</p>
          ) : null}
        </CardContent>
      </Card>

      <Card className="border-admin-border bg-card/80">
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <CardTitle className="text-base">Connection policy</CardTitle>
            <ShieldCheck className="h-4 w-4 text-muted-foreground" />
          </div>
          <CardDescription>What this workspace treats as first-class company connectors.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {PROVIDER_PRESETS.map((provider) => (
            <div key={provider.key} className="rounded-2xl border border-admin-border bg-black/10 px-4 py-3">
              <div className="flex items-center gap-2">
                <Cable className="h-4 w-4 text-muted-foreground" />
                <p className="text-sm font-medium text-foreground">{provider.label}</p>
              </div>
              <p className="mt-1 font-mono text-[10px] text-muted-foreground">{provider.envVar}</p>
              <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">{provider.description}</p>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
