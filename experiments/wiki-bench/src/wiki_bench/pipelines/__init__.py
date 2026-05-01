"""Pipeline implementations.

Every pipeline implements `Pipeline` from `.base`: ingest(docs) once,
then query(question) -> Answer N times.
"""
from .base import Pipeline
from .rag import RagPipeline
from .stuff import StuffPipeline
from .wiki import WikiPipeline

__all__ = ["Pipeline", "StuffPipeline", "RagPipeline", "WikiPipeline"]
