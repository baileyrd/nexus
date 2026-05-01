"""Classic RAG with BM25 retrieval.

Why BM25 and not embeddings:
- One fewer model in the loop = one fewer confound.
- No additional API spend during ingest or query.
- BM25 is a strong baseline on multi-hop QA; if the wiki beats RAG-BM25, it
  also beats most embedding-RAG implementations on the same metric.
"""
from __future__ import annotations

import re

from .base import Pipeline
from ..types import Answer, Document, Question


SYSTEM_TEMPLATE = """You are answering questions using retrieved excerpts.
Cite the chunk titles. If the retrieved text doesn't answer the question, say so.
Be concise — short answers preferred for factual questions.

RETRIEVED:
{chunks}
"""

_TOKEN_RE = re.compile(r"\w+")


def _tokenize(text: str) -> list[str]:
    return [t.lower() for t in _TOKEN_RE.findall(text)]


def _chunk(text: str, max_tokens: int) -> list[str]:
    """Split on whitespace, glue back into roughly-fixed-size chunks."""
    words = text.split()
    if not words:
        return []
    out: list[str] = []
    for i in range(0, len(words), max_tokens):
        out.append(" ".join(words[i : i + max_tokens]))
    return out


class RagPipeline(Pipeline):
    name = "rag"

    def __init__(self, client, k: int = 5, chunk_tokens: int = 500):
        super().__init__(client)
        self.k = k
        self.chunk_tokens = chunk_tokens
        self._chunks: list[tuple[str, str]] = []  # (chunk_id, text)
        self._bm25 = None

    def ingest(self, docs: list[Document]) -> None:
        from rank_bm25 import BM25Okapi
        chunks: list[tuple[str, str]] = []
        for d in sorted(docs, key=lambda x: x.doc_id):
            for i, c in enumerate(_chunk(d.text, self.chunk_tokens)):
                chunks.append((f"{d.title}#{i}", c))
        self._chunks = chunks
        self._bm25 = BM25Okapi([_tokenize(text) for _, text in chunks])

    def query(self, q: Question) -> Answer:
        assert self._bm25 is not None, "ingest() must be called before query()"
        scores = self._bm25.get_scores(_tokenize(q.text))
        top_idx = sorted(range(len(scores)), key=lambda i: scores[i], reverse=True)[: self.k]
        retrieved = [self._chunks[i] for i in top_idx]

        chunks_md = "\n\n".join(f"## {cid}\n{ctext}" for cid, ctext in retrieved)
        system = SYSTEM_TEMPLATE.format(chunks=chunks_md)
        resp = self.client.complete(
            system=system,
            messages=[{"role": "user", "content": q.text}],
            max_tokens=256,
        )
        return Answer(
            qid=q.qid,
            pipeline=self.name,
            text=resp.text,
            citations=[cid for cid, _ in retrieved],
            usage=resp.usage,
        )
