<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useData, useRoute, withBase } from "vitepress";

const route = useRoute();
const { site } = useData();

/** VitePress `route.path` in the browser is the full pathname (includes `site.base`). Map keys omit base. */
function stripSiteBase(pathname: string): string {
  const base = site.value.base || "/";
  if (base === "/") return pathname;
  const withSlash = base.endsWith("/") ? base : `${base}/`;
  if (pathname === withSlash.slice(0, -1) || pathname === withSlash) return "/";
  if (pathname.startsWith(withSlash)) {
    const rest = pathname.slice(withSlash.length);
    return rest ? `/${rest.replace(/^\/+/, "")}` : "/";
  }
  const noTrail = withSlash.replace(/\/$/, "");
  if (pathname === noTrail) return "/";
  if (pathname.startsWith(`${noTrail}/`)) {
    const rest = pathname.slice(noTrail.length);
    return rest || "/";
  }
  return pathname;
}
const map = ref<Record<string, string> | null>(null);
const status = ref("");
const loading = ref(true);

onMounted(async () => {
  try {
    const res = await fetch(withBase("/route-to-llm.json"));
    if (res.ok) {
      map.value = await res.json();
    }
  } catch {
    map.value = null;
  } finally {
    loading.value = false;
  }
});

function normalizePath(p: string): string {
  if (p.length > 1 && p.endsWith("/")) return p.slice(0, -1);
  return p;
}

const llmPath = computed(() => {
  if (!map.value) return null;
  const raw = stripSiteBase(route.path);
  const n = normalizePath(raw);
  const candidates = [raw, n, raw + "/", n + "/"];
  for (const c of candidates) {
    if (map.value[c]) return map.value[c];
  }
  return null;
});

const llmAbsoluteUrl = computed(() => {
  if (!llmPath.value) return null;
  const p = llmPath.value.startsWith("/") ? llmPath.value : `/${llmPath.value}`;
  if (typeof window !== "undefined") {
    return new URL(withBase(p), window.location.origin).href;
  }
  return p;
});

async function copyForLlm() {
  status.value = "";
  if (!llmPath.value) {
    status.value = "No export for this page";
    return;
  }
  const url = withBase(llmPath.value.startsWith("/") ? llmPath.value : `/${llmPath.value}`);
  try {
    const res = await fetch(url);
    if (!res.ok) {
      status.value = `Could not load (${res.status})`;
      return;
    }
    const text = await res.text();
    await navigator.clipboard.writeText(text);
    status.value = "Copied markdown";
    setTimeout(() => {
      status.value = "";
    }, 2200);
  } catch (e) {
    status.value = "Copy failed (clipboard or network)";
    console.error(e);
  }
}
</script>

<template>
  <div class="copy-for-llm-bar">
    <button
      type="button"
      class="copy-for-llm-btn"
      :disabled="loading || !llmPath"
      :aria-disabled="loading || !llmPath"
      aria-label="Copy this page as markdown for an LLM"
      @click="copyForLlm"
    >
      Copy page (MD)
    </button>
    <a
      v-if="llmAbsoluteUrl"
      class="copy-for-llm-link"
      :href="llmAbsoluteUrl"
      target="_blank"
      rel="noopener noreferrer"
      title="Open plain markdown in a new tab (for crawlers or manual fetch)"
    >
      Open .md
    </a>
    <span v-if="status" class="copy-for-llm-status" role="status">{{ status }}</span>
  </div>
</template>
