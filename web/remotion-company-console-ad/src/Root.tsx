import React from "react";
import { Composition } from "remotion";
import { CompanyConsoleAd } from "./CompanyConsoleAd";
import { COMPOSITION_DURATION_FRAMES } from "./outroConstants";

export const RemotionRoot: React.FC = () => {
  return (
    <Composition
      id="CompanyConsoleAd"
      component={CompanyConsoleAd}
      durationInFrames={COMPOSITION_DURATION_FRAMES}
      fps={30}
      width={1080}
      height={1920}
      defaultProps={{
        headline: "Put your company on intelligent autopilot.",
        subline:
          "AI agents carry the work—tasks, memory, goals, and playbooks in one intelligence layer.\n\nYou stay CEO: step in from the inbox when judgment matters.",
        prompt:
          "Have agents run this initiative—sync goals, memory, and tasks so nothing lives in a silo",
      }}
    />
  );
};
