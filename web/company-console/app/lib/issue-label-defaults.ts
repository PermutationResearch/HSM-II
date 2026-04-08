/** Must match `DEFAULTS` in `seed_default_issue_labels` (Rust). Used when the seed endpoint is missing (404). */
export const ISSUE_LABEL_SEED_DEFAULTS: { slug: string; display_name: string; sort_order: number }[] = [
  { slug: "bug", display_name: "Bug", sort_order: 10 },
  { slug: "feature", display_name: "Feature", sort_order: 20 },
  { slug: "chore", display_name: "Chore", sort_order: 30 },
  { slug: "docs", display_name: "Docs", sort_order: 40 },
  { slug: "infra", display_name: "Infra", sort_order: 50 },
  { slug: "customer", display_name: "Customer", sort_order: 60 },
  { slug: "security", display_name: "Security", sort_order: 70 },
  { slug: "data", display_name: "Data", sort_order: 80 },
  { slug: "design", display_name: "Design", sort_order: 90 },
  { slug: "research", display_name: "Research", sort_order: 100 },
];
