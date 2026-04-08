import React from "react";
import {
  Easing,
  Img,
  interpolate,
  staticFile,
  useCurrentFrame,
} from "remotion";
import {
  MASCOT_SPIN_HOLD_FRAMES,
  OUTRO_LOGO_FADE_END,
  OUTRO_LOGO_FADE_START,
} from "./outroConstants";

const DEFAULT_LETTERPRESS = "Skills-Keychain-letterpress.png";

/** Pure black behind `Skills-Keychain-letterpress.png` (matches raster). */
const LOGO_OUTRO_SECTION_BG = {
  backgroundColor: "#000000",
} as const;

export type LogoOutroCanvasProps = {
  width: number;
  height: number;
  /** File in `public/` (default: Skills-Keychain-letterpress.png). */
  letterpressFile?: string;
};

/** Full top band (#000) + centered metallic mark from `Skills-Keychain-letterpress.png`. */
export const LogoOutroCanvas: React.FC<LogoOutroCanvasProps> = ({
  width,
  height,
  letterpressFile = DEFAULT_LETTERPRESS,
}) => {
  const src = staticFile(letterpressFile);
  const frame = useCurrentFrame();
  const w = Math.max(16, Math.round(width));
  const h = Math.max(16, Math.round(height));

  const fadeIn = Math.max(0, frame - OUTRO_LOGO_FADE_START);
  const scaleIntro = interpolate(
    fadeIn,
    [0, Math.max(1, OUTRO_LOGO_FADE_END - OUTRO_LOGO_FADE_START)],
    [0.94, 1],
    {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
      easing: Easing.out(Easing.cubic),
    }
  );

  /** Dolly zoom on the mark for the outro hold (clipped by section `overflow: hidden`). */
  const cameraZoom =
    frame < OUTRO_LOGO_FADE_END
      ? 1
      : interpolate(
          frame - OUTRO_LOGO_FADE_END,
          [0, Math.max(1, MASCOT_SPIN_HOLD_FRAMES)],
          [1, 1.28],
          {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
            easing: Easing.inOut(Easing.cubic),
          }
        );

  const logoScale = scaleIntro * cameraZoom;

  /** PNG is 1024×771 — use most of the band; `contain` preserves aspect. */
  const logoMaxW = Math.min(w * 0.98, w - 24);
  const logoMaxH = h * 0.94;

  return (
    <div
      style={{
        width: w,
        height: h,
        position: "relative",
        overflow: "hidden",
        ...LOGO_OUTRO_SECTION_BG,
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: `${Math.max(8, h * 0.03)}px ${Math.max(12, w * 0.03)}px`,
          boxSizing: "border-box",
        }}
      >
        <div
          style={{
            maxWidth: logoMaxW,
            maxHeight: logoMaxH,
            transform: `scale(${logoScale})`,
            transformOrigin: "center center",
          }}
        >
          <Img
            src={src}
            style={{
              display: "block",
              maxWidth: "100%",
              maxHeight: logoMaxH,
              width: "auto",
              height: "auto",
              objectFit: "contain",
              objectPosition: "center center",
            }}
          />
        </div>
      </div>
    </div>
  );
};
