"""Sanity tests that the skeleton imports cleanly and the interfaces line up.

No API calls. These run with `pytest` after `pip install -e .`.
"""
from __future__ import annotations

import pytest

from wiki_bench.client import LLMClient
from wiki_bench.pipelines import Pipeline, RagPipeline, StuffPipeline, WikiPipeline
from wiki_bench.types import Document, Question, TokenUsage


def test_token_usage_addition_and_cost():
    a = TokenUsage(input_tokens=1000, output_tokens=500)
    b = TokenUsage(input_tokens=200, cache_read_tokens=800)
    c = a + b
    assert c.input_tokens == 1200
    assert c.output_tokens == 500
    assert c.cache_read_tokens == 800

    cost = c.cost_usd("claude-sonnet-4-6")
    assert cost > 0
    # Sanity: output is the dominant rate; doubling output tokens should raise cost.
    cost2 = TokenUsage(output_tokens=c.output_tokens * 2).cost_usd("claude-sonnet-4-6")
    assert cost2 > cost / 2


def test_pipelines_subclass_base():
    for cls in (StuffPipeline, RagPipeline, WikiPipeline):
        assert issubclass(cls, Pipeline)
        assert cls.name in {"stuff", "rag", "wiki"}


def test_corpus_types_are_hashable():
    d = Document(doc_id="x::0", title="X", text="...")
    q = Question(qid="q1", text="?", gold_answer="42", supporting_doc_ids=("x::0",))
    # Frozen dataclasses → usable as dict keys / set members.
    assert {d, d}.__len__() == 1
    assert {q}.__contains__(q)


def test_client_constructs_without_network():
    # __init__ must NOT make a network call or import anthropic;
    # the SDK is lazy-loaded inside complete().
    client = LLMClient(model="claude-sonnet-4-6", api_key="not-a-real-key")
    assert client.model == "claude-sonnet-4-6"
    assert client._client is None
