/**
 * Helpers for wiring an LLM to json-render (system prompt + user envelope).
 * Server-only usage recommended (import from Route Handlers / Server Actions).
 */

import { buildUserPrompt } from "@json-render/core";
import type { Spec } from "@json-render/core";
import { hsmGenUiCatalogPrompt } from "./hsm-catalog";

/** Full system message: catalog + guardrails. */
export function hsmGenUiSystemMessage(): string {
  return hsmGenUiCatalogPrompt();
}

/** User turn wrapper (truncation + optional current spec for edits). */
export function hsmGenUiUserMessage(userText: string, currentSpec?: Spec | null): string {
  return buildUserPrompt({
    prompt: userText,
    currentSpec: currentSpec ?? null,
  });
}
