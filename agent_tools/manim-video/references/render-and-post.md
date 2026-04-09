# Render and Post

- Preview with low-quality flags during iteration.
- Final render with locked frame rate and pixel dimensions.
- Store render profile constants in code to avoid accidental drift.
- Export sidecar metadata: title, duration, scene index, revision hash.
- Validate final output for:
  - readable text at mobile width
  - no clipped mobjects
  - transition timing coherence
