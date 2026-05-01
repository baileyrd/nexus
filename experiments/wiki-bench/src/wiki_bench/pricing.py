"""Anthropic API pricing.

USD per 1M tokens. Update against https://docs.claude.com/en/docs/about-claude/pricing
when prices change. Kept as a single dict so cost calculations have one source
of truth.

Models we use:
- claude-sonnet-4-6: default for answer pipelines
- claude-haiku-4-5: default for judging (cheap, accurate enough for binary correctness)
- claude-opus-4-7: higher-stakes runs / spot checks
"""
from __future__ import annotations

# input | output | cache_write (5min ephemeral) | cache_read
_RATES: dict[str, dict[str, float]] = {
    "claude-sonnet-4-6": {"in": 3.00,  "out": 15.00, "cache_w": 3.75,  "cache_r": 0.30},
    "claude-haiku-4-5":  {"in": 1.00,  "out":  5.00, "cache_w": 1.25,  "cache_r": 0.10},
    "claude-opus-4-7":   {"in": 15.00, "out": 75.00, "cache_w": 18.75, "cache_r": 1.50},
}


def rate(model: str) -> dict[str, float] | None:
    """Per-1M-token rates for a model. Returns None for unknown models."""
    return _RATES.get(model)


def known_models() -> list[str]:
    return list(_RATES.keys())
