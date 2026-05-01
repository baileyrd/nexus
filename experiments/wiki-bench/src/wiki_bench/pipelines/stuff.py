"""Dump-everything-into-context baseline.

Two configurations matter:
- `cache_system=True`: corpus goes in the system prompt with `cache_control:
  ephemeral`. First query pays full price; subsequent queries within ~5 min hit
  the cache at ~10% input cost. Realistic production setting.
- `cache_system=False`: every query pays the full corpus price. Worst-case
  ceiling that the wiki pipeline beats by a wide margin.
"""
from __future__ import annotations

from .base import Pipeline
from ..types import Answer, Document, Question


SYSTEM_TEMPLATE = """You are answering questions using a fixed corpus of documents.
Quote document titles when citing. If the answer isn't in the corpus, say so.
Be concise — short answers preferred for factual questions.

CORPUS:
{corpus}
"""


class StuffPipeline(Pipeline):
    name = "stuff"

    def __init__(self, client, cache_system: bool = True):
        super().__init__(client)
        self.cache_system = cache_system
        self._docs: list[Document] = []
        self._system_prompt: str = ""

    def ingest(self, docs: list[Document]) -> None:
        # Build the system prompt once, deterministically (sort by doc_id) so
        # the cache prefix is byte-stable across queries.
        self._docs = sorted(docs, key=lambda d: d.doc_id)
        self._system_prompt = SYSTEM_TEMPLATE.format(corpus=_render_corpus(self._docs))

    def query(self, q: Question) -> Answer:
        resp = self.client.complete(
            system=self._system_prompt,
            messages=[{"role": "user", "content": q.text}],
            max_tokens=256,
            cache_system=self.cache_system,
        )
        return Answer(
            qid=q.qid,
            pipeline=self.name if self.cache_system else "stuff-nocache",
            text=resp.text,
            citations=[],
            usage=resp.usage,
        )


def _render_corpus(docs: list[Document]) -> str:
    return "\n\n---\n\n".join(f"# {d.title}\n{d.text}" for d in docs)
