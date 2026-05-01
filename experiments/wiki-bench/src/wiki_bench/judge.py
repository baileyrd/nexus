"""LLM-as-judge for HotpotQA short-answer correctness.

HotpotQA gold answers are short strings ("Yes", "1962", "Steve Wozniak"). String
match is too brittle (capitalization, articles, equivalent phrasings), so we
ask a cheap model to grade equivalence.

Sanity check before trusting numbers: hand-grade a random ~30-sample slice and
measure judge agreement. If agreement is < 90%, tighten the rubric.
"""
from __future__ import annotations

import json
import re

from .client import LLMClient
from .types import Answer, JudgeResult, Question

JUDGE_SYSTEM = """You grade short-answer QA. Given a question, a gold answer,
and a predicted answer, decide whether the prediction is factually equivalent
to the gold.

Rules:
- Allow paraphrase, capitalization differences, articles, and equivalent units.
- Reject hallucinations and answers that contain the right info but contradict
  it elsewhere.
- For multi-fact answers, score partial credit in [0, 1].

Output ONLY a single-line JSON object, no markdown fence:
{"correct": <bool>, "score": <0..1>, "reasoning": "<one sentence>"}
"""

USER_TEMPLATE = """Question: {question}
Gold: {gold}
Prediction: {prediction}
"""


def _parse_judge_json(text: str) -> dict:
    s = text.strip()
    s = re.sub(r"^```(?:json)?\s*", "", s)
    s = re.sub(r"\s*```$", "", s)
    lo = s.find("{")
    hi = s.rfind("}")
    if lo == -1 or hi == -1 or hi < lo:
        raise ValueError("no JSON object in judge output")
    return json.loads(s[lo : hi + 1])


def judge_answer(client: LLMClient, q: Question, ans: Answer) -> JudgeResult:
    user = USER_TEMPLATE.format(
        question=q.text, gold=q.gold_answer, prediction=ans.text or "(empty)"
    )
    resp = client.complete(
        system=JUDGE_SYSTEM,
        messages=[{"role": "user", "content": user}],
        max_tokens=200,
    )

    try:
        parsed = _parse_judge_json(resp.text)
        correct = bool(parsed.get("correct", False))
        score = float(parsed.get("score", 0.0))
        reasoning = str(parsed.get("reasoning", ""))[:500]
    except (ValueError, json.JSONDecodeError, TypeError):
        correct = False
        score = 0.0
        reasoning = f"judge output unparseable: {resp.text[:160]!r}"

    return JudgeResult(
        qid=q.qid,
        pipeline=ans.pipeline,
        correct=correct,
        score=max(0.0, min(1.0, score)),
        reasoning=reasoning,
        judge_usage=resp.usage,
    )
