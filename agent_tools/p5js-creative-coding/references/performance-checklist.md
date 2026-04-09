# Performance Checklist

- Pre-allocate arrays; avoid per-frame object creation.
- Cap particle counts by canvas area (`N ~ width * height * k`).
- Use simple blend modes unless artistically required.
- Limit expensive calls (`noiseDetail`, `loadPixels`, per-pixel loops).
- Degrade detail on resize for small/mobile screens.
- Keep deterministic seed path for reproducible debugging.
