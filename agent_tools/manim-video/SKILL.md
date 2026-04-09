---
name: manim-video
description: Create high-clarity technical animations with Manim for math, systems, architecture, and algorithm explainers. Use when users ask for Manim scenes, educational motion graphics, equation animations, or narrated visual explainers.
license: MIT
---

# Manim Video

Build concise, high-signal Manim videos with deliberate pacing and visual hierarchy.

## Core Goal

Communicate one complex idea per scene cluster, with smooth transitions and explicit visual focus.

## Workflow

1. Write a 6-12 beat storyboard before coding.
2. Build reusable scene helpers (`title_card`, `callout`, `highlight_step`).
3. Keep camera and object motion purposeful; avoid decorative movement.
4. Use incremental revelation (`Write`, `FadeIn`, `TransformMatchingTex`) for cognition.
5. Render preview at low quality, then final in production profile.

## Implementation Standards

- Keep scene classes short and composable.
- Separate content data from animation choreography.
- Use consistent typography and spacing across scenes.
- Add deterministic timing constants for all transitions.
- Export final + thumbnail + captions metadata where possible.

## 5 Reference Expansions

- `references/storyboard-and-timing.md`
- `references/scene-architecture.md`
- `references/camera-motion-language.md`
- `references/typography-and-annotations.md`
- `references/render-and-post.md`
