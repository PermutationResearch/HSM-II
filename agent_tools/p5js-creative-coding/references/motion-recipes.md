# Motion Recipes

- **Noise drift**: sample `noise(x * s, y * s, t)` for smooth vector drift.
- **Orbital swarm**: phase-offset circles with shared angular velocity and subtle jitter.
- **Pulse field**: modulate size/alpha by sine + local noise for breathing effects.
- **Trail persistence**: use translucent background clears (`background(0, 12)`) for temporal smear.
- **Interaction blend**: lerp target toward mouse/touch to avoid snapping artifacts.
