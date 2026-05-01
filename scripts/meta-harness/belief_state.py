"""
Harness-side belief summaries for meta-harness NDJSON scoring.

This is a deliberate, low-dimensional *logging* model (not production control):
Beta pseudo-counts updated from stream events, aligned with the “Bayesian
control layer” story in Papamarkou et al., “Position: Agentic AI systems should be
making Bayes-consistent decisions” (SSRN 6143772 / HAL hal-05480691).

See docs/META_HARNESS_BELIEF_STATE.md for semantics and limitations.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field


@dataclass
class BetaBelief:
    """Beta–Bernoulli conjugate with scalar pseudo-count updates."""

    alpha: float = 1.0
    beta: float = 1.0

    def observe_success(self, weight: float = 1.0) -> None:
        w = max(0.0, float(weight))
        if w:
            self.alpha += w

    def observe_failure(self, weight: float = 1.0) -> None:
        w = max(0.0, float(weight))
        if w:
            self.beta += w

    def mean(self) -> float:
        return self.alpha / (self.alpha + self.beta)

    def entropy_nats(self) -> float:
        """Bernoulli entropy of the posterior mean (diagnostic, not exact predictive)."""
        p = self.mean()
        eps = 1e-9
        p = min(1.0 - eps, max(eps, p))
        return -(p * math.log(p) + (1.0 - p) * math.log(1.0 - p))


def extra_tool_voi_proxy(entropy_nats: float, tool_calls: int, cap: float = 12.0) -> float:
    """
    Cheap *proxy* for value of one more tool call: high when uncertain and under a soft budget.
    Not calibrated VoI; for smoke / regression logging only.
    """
    if entropy_nats <= 0.0:
        return 0.0
    headroom = max(0.0, 1.0 - min(float(tool_calls) / cap, 1.0))
    return float(entropy_nats * headroom)


@dataclass
class HarnessBeliefV1:
    """
    Online-ish updates: stream errors and heavy tool use nudge failure mass;
    terminal block adds success/failure from finalize and answer mass.
    """

    task_success: BetaBelief = field(default_factory=BetaBelief)

    def on_error_event(self) -> None:
        self.task_success.observe_failure(2.0)

    def on_tool_event(self, tool_calls_so_far: int) -> None:
        if tool_calls_so_far > 10:
            self.task_success.observe_failure(0.45)
        elif tool_calls_so_far > 8:
            self.task_success.observe_failure(0.35)
        elif tool_calls_so_far > 5:
            self.task_success.observe_failure(0.12)

    def apply_terminal_evidence(
        self,
        *,
        finalized: bool,
        error: bool,
        tool_calls: int,
        streamed_chars: int,
        final_answer_len: int,
    ) -> None:
        """Aggregate end-of-turn evidence (stream errors already updated online)."""
        text_n = max(streamed_chars, final_answer_len)
        if error:
            self.task_success.observe_failure(0.6)
        if finalized:
            self.task_success.observe_success(1.2)
            if text_n >= 50:
                self.task_success.observe_success(1.8)
            elif text_n > 0:
                self.task_success.observe_success(0.6)
        else:
            self.task_success.observe_failure(1.4)
        if tool_calls == 0 and not error:
            # No tools at all is weak evidence against “rich agentic turn” (harness prior).
            self.task_success.observe_failure(0.25)
        if tool_calls > 0:
            self.task_success.observe_success(0.35)

    def to_jsonable(self, tool_calls: int) -> dict:
        ts = self.task_success
        ent = ts.entropy_nats()
        mean = ts.mean()
        voi = extra_tool_voi_proxy(ent, tool_calls)
        return {
            "model_id": "harness_beta_task_success_v1",
            "task_success": {
                "alpha": round(ts.alpha, 4),
                "beta": round(ts.beta, 4),
                "posterior_mean": round(mean, 4),
                "entropy_nats": round(ent, 4),
            },
            "voi_proxy": {
                "extra_tool_nats": round(voi, 4),
                "information_saturated": bool(ent < 0.22 or mean > 0.92 or mean < 0.08),
            },
            "disclaimer": "Harness log only; not wired to live routing or promoted HsmRunnerConfig.",
        }
