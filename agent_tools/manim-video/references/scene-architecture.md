# Scene Architecture

- One scene class per conceptual unit.
- Extract repeated objects into factory functions.
- Keep animation orchestration linear and readable.
- Prefer `VGroup` composition over ad-hoc coordinate duplication.
- Build with explicit anchors (`to_edge`, `next_to`, `align_to`) for stable layout.
