"""Pipeline ABC. The interface every approach implements."""
from __future__ import annotations

from abc import ABC, abstractmethod

from ..client import LLMClient
from ..types import Answer, Document, Question, TokenUsage


class Pipeline(ABC):
    name: str = "base"

    def __init__(self, client: LLMClient):
        self.client = client
        self.ingest_usage = TokenUsage()

    @abstractmethod
    def ingest(self, docs: list[Document]) -> None:
        """Pre-process the corpus once. Implementations:

        - StuffPipeline: trivial; just retain the docs.
        - RagPipeline: chunk and BM25-index. No LLM calls.
        - WikiPipeline: LLM-synthesizes wiki pages, builds index.
          Updates self.ingest_usage with all tokens spent.
        """

    @abstractmethod
    def query(self, q: Question) -> Answer:
        """Answer one question. The returned Answer carries usage and latency."""
