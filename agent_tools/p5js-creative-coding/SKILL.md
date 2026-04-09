---
name: p5js-creative-coding
description: Design and implement expressive p5.js sketches for generative art, interactive visuals, data-driven motion, and creative coding experiments. Use when users ask for p5.js sketches, generative visuals, interactive canvas demos, artful particles/noise fields, or creative coding prototypes.
license: MIT
---

# p5.js Creative Coding

Build production-ready creative coding sketches with clear controls, stable performance, and intentional composition.

## When to Use

- User asks for a p5.js sketch, animation, or interactive visual.
- You need a fast canvas prototype for motion/art direction.
- You want parameterized generative visuals with reproducible seeds.

## Workflow

1. Define visual intent in one sentence (mood + motion + interaction).
2. Pick primitives (`points`, `lines`, `circles`, `text`, shader-like patterns).
3. Choose time source (`frameCount`, `millis`, eased progress, noise domain).
4. Add interaction (`mouse`, `touch`, keyboard toggles) only if it improves control.
5. Add deterministic seed and a compact control panel (`URL params` or constants).
6. Optimize draw loop (avoid allocations, cap particles, throttle expensive passes).

## Required Implementation Standards

- Include `setup()` + `draw()` with clear state ownership.
- Expose top-level tunables (`const CONFIG = { ... }`).
- Support high-DPI safely (`pixelDensity(Math.min(window.devicePixelRatio, 2))`).
- Handle resize (`windowResized` + `resizeCanvas`).
- Keep per-frame complexity predictable.
- Add brief comments for non-obvious math.

## Preferred Patterns

- Layered composition: background field, mid-detail, highlight accents.
- Noise-based motion for natural continuity.
- Palette discipline: 3-5 core colors with alpha variation.
- Optional deterministic export mode via a fixed seed.

## References

- `references/composition-patterns.md`
- `references/motion-recipes.md`
- `references/performance-checklist.md`
