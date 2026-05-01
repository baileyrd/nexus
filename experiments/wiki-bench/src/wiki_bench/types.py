"""Shared dataclasses: corpus units, results, token accounting."""
from __future__ import annotations

from dataclasses import dataclass, field

from .pricing import rate as _rate


@dataclass(frozen=True)
class Document:
    doc_id: str
    title: str
    text: str


@dataclass(frozen=True)
class Question:
    qid: str
    text: str
    gold_answer: str
    supporting_doc_ids: tuple[str, ...]


@dataclass
class TokenUsage:
    """Anthropic-style usage. Cache fields are 0 when caching isn't used."""

    input_tokens: int = 0
    output_tokens: int = 0
    cache_creation_tokens: int = 0
    cache_read_tokens: int = 0

    def __add__(self, other: "TokenUsage") -> "TokenUsage":
        return TokenUsage(
            input_tokens=self.input_tokens + other.input_tokens,
            output_tokens=self.output_tokens + other.output_tokens,
            cache_creation_tokens=self.cache_creation_tokens + other.cache_creation_tokens,
            cache_read_tokens=self.cache_read_tokens + other.cache_read_tokens,
        )

    def cost_usd(self, model: str) -> float:
        """Approximate cost. Returns 0.0 for unknown models."""
        r = _rate(model)
        if r is None:
            return 0.0
        return (
            self.input_tokens * r["in"]
            + self.output_tokens * r["out"]
            + self.cache_creation_tokens * r["cache_w"]
            + self.cache_read_tokens * r["cache_r"]
        ) / 1_000_000


@dataclass
class Answer:
    qid: str
    pipeline: str
    text: str
    citations: list[str] = field(default_factory=list)
    usage: TokenUsage = field(default_factory=TokenUsage)
    latency_ms: float = 0.0


@dataclass
class JudgeResult:
    qid: str
    pipeline: str
    correct: bool
    score: float
    reasoning: str
    judge_usage: TokenUsage


@dataclass
class RunResult:
    pipeline: str
    answers: list[Answer]
    judgments: list[JudgeResult]
    ingest_usage: TokenUsage
