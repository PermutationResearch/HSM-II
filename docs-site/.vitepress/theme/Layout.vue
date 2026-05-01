<script setup lang="ts">
import DefaultTheme from "vitepress/theme";
import CopyForLlmBar from "./components/CopyForLlmBar.vue";
</script>

<template>
  <DefaultTheme.Layout>
    <template #doc-before>
      <div class="copy-for-llm-wrap">
        <CopyForLlmBar />
      </div>
    </template>
    <template #home-hero-after>
      <div class="copy-for-llm-wrap copy-for-llm-wrap--home">
        <CopyForLlmBar />
      </div>
    </template>
  </DefaultTheme.Layout>
</template>
