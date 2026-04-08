/** Top band matches layout `height: 68%` of the composition. */
export const TOP_BAND_FRACTION = 0.68;

/** Console + warp fade out in the top band only. */
export const OUTRO_CONSOLE_FADE_START = 270;
export const OUTRO_CONSOLE_FADE_END = 302;

/** SVG logo layer fades in (same band, crossfade with console). */
export const OUTRO_LOGO_FADE_START = 276;
export const OUTRO_LOGO_FADE_END = 302;

/** Yaw starts when the logo layer begins appearing. */
export const OUTRO_SPIN_START = OUTRO_LOGO_FADE_START;

/** Fully visible logo keeps the gentle tilt/spin for 5s @ 30fps after fade-in completes. */
export const MASCOT_SPIN_HOLD_FRAMES = 150;
export const OUTRO_MASCOT_END = OUTRO_LOGO_FADE_END + MASCOT_SPIN_HOLD_FRAMES;

export const COMPOSITION_DURATION_FRAMES = OUTRO_MASCOT_END + 24;
