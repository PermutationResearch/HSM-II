import type { SopExampleDocument } from "@/app/lib/sop-examples-types";

/** Minimal English stopwords for token overlap (length ≥ 4 elsewhere). */
const STOP = new Set([
  "that",
  "this",
  "with",
  "from",
  "have",
  "been",
  "were",
  "they",
  "will",
  "would",
  "could",
  "should",
  "about",
  "which",
  "their",
  "there",
  "these",
  "those",
  "what",
  "when",
  "where",
  "while",
  "without",
  "within",
  "your",
  "also",
  "into",
  "only",
  "more",
  "most",
  "some",
  "such",
  "than",
  "then",
  "them",
  "very",
  "just",
  "like",
  "make",
  "each",
  "other",
  "many",
  "must",
]);

function stripMarkdownish(s: string): string {
  return s
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`[^`]+`/g, " ")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[^a-z0-9\s]+/gi, " ");
}

export function extractSignificantTokens(text: string, minLen = 4): Set<string> {
  const flat = stripMarkdownish(text).toLowerCase();
  const out = new Set<string>();
  for (const raw of flat.split(/[^a-z0-9]+/)) {
    if (raw.length < minLen) continue;
    if (STOP.has(raw)) continue;
    out.add(raw);
  }
  return out;
}

/** Flatten playbook text for overlap checks. */
export function sopDocumentToPlainText(doc: SopExampleDocument): string {
  const parts: string[] = [
    doc.title,
    doc.tab_label,
    doc.summary,
    doc.department ?? "",
    doc.interaction_log.description,
  ];
  for (const p of doc.phases) {
    parts.push(
      p.name,
      p.sop_logic,
      ...(p.actions ?? []),
      ...(p.company_os ?? []),
      p.resolution ?? "",
      p.escalation ?? "",
    );
  }
  for (const e of doc.interaction_log.suggested_events) {
    parts.push(e.action, e.subject_hint ?? "", e.payload_summary ?? "");
  }
  return parts.join("\n");
}

export type VisionLintLevel = "info" | "warn" | "ok";

export type VisionLintMessage = {
  level: VisionLintLevel;
  text: string;
};

export type VisionLintResult = {
  messages: VisionLintMessage[];
  /** Portion of distinct vision tokens that appear in playbook text (0–1). */
  coverage: number | null;
  visionTokenCount: number;
  matchedTokenCount: number;
  /** Whether visions.md was non-empty on disk */
  hadVisionsFile: boolean;
  /** Whether YC-Bench profile text (strategy + controller prompt) was non-empty */
  hadYcBenchProfile: boolean;
  /** Whether shared context (API) contributed */
  hadContextMarkdown: boolean;
};

/**
 * Heuristic alignment: significant-token coverage between vision corpus and playbook wording.
 * Not semantic understanding — surfaces “did you reference the same vocabulary as the vision?”
 */
export function lintPlaybookAgainstVision(
  visionCorpus: string,
  doc: SopExampleDocument,
  opts?: { hadVisionsFile: boolean; hadYcBenchProfile?: boolean; hadContextMarkdown: boolean },
): VisionLintResult {
  const hadVisionsFile = opts?.hadVisionsFile ?? false;
  const hadYcBenchProfile = opts?.hadYcBenchProfile ?? false;
  const hadContextMarkdown = opts?.hadContextMarkdown ?? false;
  const corpus = visionCorpus.trim();
  const playbook = sopDocumentToPlainText(doc).toLowerCase();

  if (!corpus) {
    return {
      messages: [
        {
          level: "info",
          text:
            "No vision text yet. Add visions.md at the pack root (under hsmii_home), ensure the YC-Bench profile loads (company context + agents/skills), and/or set Shared context (API) on the company.",
        },
      ],
      coverage: null,
      visionTokenCount: 0,
      matchedTokenCount: 0,
      hadVisionsFile,
      hadYcBenchProfile,
      hadContextMarkdown,
    };
  }

  const visionTokens = extractSignificantTokens(corpus);
  if (visionTokens.size < 4) {
    return {
      messages: [
        {
          level: "info",
          text: "Vision corpus is very short — add a few concrete nouns and outcomes so alignment checks are meaningful.",
        },
      ],
      coverage: null,
      visionTokenCount: visionTokens.size,
      matchedTokenCount: 0,
      hadVisionsFile,
      hadYcBenchProfile,
      hadContextMarkdown,
    };
  }

  let matched = 0;
  for (const w of visionTokens) {
    if (playbook.includes(w)) matched++;
  }
  const coverage = visionTokens.size > 0 ? matched / visionTokens.size : 0;

  const messages: VisionLintMessage[] = [];

  if (matched >= 3 && coverage >= 0.12) {
    messages.push({
      level: "ok",
      text: `Several vision terms appear in this playbook (${matched} / ${visionTokens.size} significant tokens). Good lexical tie-in.`,
    });
  } else if (coverage < 0.1 && visionTokens.size >= 10) {
    messages.push({
      level: "warn",
      text: "Low overlap between playbook wording and vision tokens — consider naming outcomes, constraints, or products from visions.md, the YC-Bench profile, or Shared context in the title or steps.",
    });
  } else if (coverage < 0.18 && visionTokens.size >= 8) {
    messages.push({
      level: "info",
      text: "Moderate overlap with vision — add explicit references to priorities or non-goals from the vision corpus if this playbook is mission-critical.",
    });
  } else {
    messages.push({
      level: "info",
      text: `Vision coverage ${(coverage * 100).toFixed(0)}% (${matched} / ${visionTokens.size} tokens). Adjust wording if this playbook should mirror specific vision language.`,
    });
  }

  return {
    messages,
    coverage,
    visionTokenCount: visionTokens.size,
    matchedTokenCount: matched,
    hadVisionsFile,
    hadYcBenchProfile,
    hadContextMarkdown,
  };
}
