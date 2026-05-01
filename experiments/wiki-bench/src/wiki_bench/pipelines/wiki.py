"""Karpathy-style LLM-maintained wiki.

Three operations:
    ingest:   for each source, write a summary page; cross-link related pages.
    query:    BM25-retrieve top-k wiki pages; LLM synthesizes a cited answer.
    lint:     (deferred) find contradictions / orphans / stale claims.

Idempotent: if `wiki_dir/` already has *.md from a previous run, ingest skips
the API calls and loads from disk. Re-running the same corpus is free.
"""
from __future__ import annotations

import json
import re
from pathlib import Path

from .base import Pipeline
from ..types import Answer, Document, Question


INGEST_SUMMARIZE_PROMPT = """Write a wiki page synthesizing the source below.

Format:
# <Title>

<3–6 paragraphs, leading with the entity / claim being described. Preserve
concrete facts (names, dates, numbers). Use [[Page Name]] markdown wikilinks
for any concept that likely warrants its own page. Do not invent facts not in
the source.>

SOURCE TITLE: {title}
SOURCE TEXT:
{text}
"""

INGEST_CROSSLINK_PROMPT = """A new wiki page was just written. For each existing
candidate page below, decide whether to add a cross-reference link in that
existing page pointing to the new page. Output a JSON array of patches:

[{{"page_id": "<id>", "link_text": "<short reason>"}}, ...]

If no patches are warranted, output [].

Output ONLY the JSON array, no commentary, no markdown fence.

NEW PAGE TITLE: {new_title}
NEW PAGE TEXT:
{new_text}

CANDIDATE EXISTING PAGES:
{candidates}
"""

QUERY_PROMPT = """Answer using these wiki pages. Cite page titles in brackets.
If the pages don't answer the question, say so. Be concise — short answers
preferred for factual questions.

PAGES:
{pages}

QUESTION: {question}
"""

_TOKEN_RE = re.compile(r"\w+")


def _tokenize(text: str) -> list[str]:
    return [t.lower() for t in _TOKEN_RE.findall(text)]


def _safe_bm25(tokenized: list[list[str]]):
    """Return BM25Okapi or None if the corpus is degenerate (all empty / all
    identical vocab, which crashes BM25Okapi with ZeroDivisionError)."""
    from rank_bm25 import BM25Okapi
    if not tokenized or all(not toks for toks in tokenized):
        return None
    try:
        return BM25Okapi(tokenized)
    except (ZeroDivisionError, ValueError):
        return None


def _slug(s: str) -> str:
    s = re.sub(r"[^\w\-]+", "_", s.strip())
    return re.sub(r"_+", "_", s).strip("_") or "untitled"


def _parse_json_array(text: str) -> list[dict]:
    """Parse a JSON array, tolerating ```json fences and stray prose."""
    s = text.strip()
    s = re.sub(r"^```(?:json)?\s*", "", s)
    s = re.sub(r"\s*```$", "", s)
    # Find the first '[' and last ']' to be lenient with prose wrapping.
    lo = s.find("[")
    hi = s.rfind("]")
    if lo == -1 or hi == -1 or hi < lo:
        return []
    try:
        out = json.loads(s[lo : hi + 1])
        return out if isinstance(out, list) else []
    except json.JSONDecodeError:
        return []


class WikiPipeline(Pipeline):
    name = "wiki"

    def __init__(self, client, wiki_dir: Path, k: int = 5, crosslink_n: int = 8):
        super().__init__(client)
        self.wiki_dir = Path(wiki_dir)
        self.k = k
        self.crosslink_n = crosslink_n
        self._pages: list[tuple[str, str]] = []  # (page_id, full_markdown)
        self._bm25 = None

    def ingest(self, docs: list[Document]) -> None:
        self.wiki_dir.mkdir(parents=True, exist_ok=True)

        existing = sorted(self.wiki_dir.glob("*.md"))
        if existing:
            # Idempotent reload: pages already on disk → skip API calls.
            self._pages = [(p.stem, p.read_text()) for p in existing]
            self._bm25 = _safe_bm25([_tokenize(text) for _, text in self._pages])
            if self._bm25 is None:
                # Pages exist but tokenize empty / degenerate. Wipe and re-ingest.
                for p in existing:
                    p.unlink()
                self._pages = []
            else:
                return

        # Deterministic ingest order.
        ordered = sorted(docs, key=lambda d: d.doc_id)

        for d in ordered:
            page_id = _slug(d.title)
            page_path = self.wiki_dir / f"{page_id}.md"

            resp = self.client.complete(
                system=None,
                messages=[{
                    "role": "user",
                    "content": INGEST_SUMMARIZE_PROMPT.format(title=d.title, text=d.text),
                }],
                max_tokens=800,
            )
            self.ingest_usage += resp.usage
            page_path.write_text(resp.text)
            self._pages.append((page_id, resp.text))

            # Cross-link pass: only if there are prior pages.
            prior = self._pages[:-1]
            if len(prior) < 2:
                # BM25 over a single doc is degenerate; skip crosslink until
                # there's a real candidate set. The cost saving is negligible.
                continue
            tokenized = [_tokenize(text) for _, text in prior]
            bm25 = _safe_bm25(tokenized)
            if bm25 is None:
                continue
            scores = bm25.get_scores(_tokenize(resp.text))
            top_idx = sorted(range(len(scores)), key=lambda i: scores[i], reverse=True)[
                : self.crosslink_n
            ]
            candidates = [prior[i] for i in top_idx]
            cand_blob = "\n\n".join(
                f"## {pid}\n{txt[:600]}" for pid, txt in candidates
            )

            cl = self.client.complete(
                system=None,
                messages=[{
                    "role": "user",
                    "content": INGEST_CROSSLINK_PROMPT.format(
                        new_title=d.title,
                        new_text=resp.text[:1500],
                        candidates=cand_blob,
                    ),
                }],
                max_tokens=400,
            )
            self.ingest_usage += cl.usage

            patches = _parse_json_array(cl.text)
            for p in patches:
                if not isinstance(p, dict):
                    continue
                pid = p.get("page_id")
                link_text = p.get("link_text", "")
                if not pid or pid not in {existing_id for existing_id, _ in prior}:
                    continue
                target = self.wiki_dir / f"{pid}.md"
                if not target.exists():
                    continue
                # Append a "Related" line idempotently. Reload the in-memory copy.
                stamp = f"\n\n_Related: [[{d.title}]] — {link_text}_\n"
                with target.open("a") as fp:
                    fp.write(stamp)
            # Refresh in-memory pages from disk for any patched entries.
            self._pages = [(p.stem, p.read_text()) for p in sorted(self.wiki_dir.glob("*.md"))]

        self._bm25 = _safe_bm25([_tokenize(text) for _, text in self._pages])

    def query(self, q: Question) -> Answer:
        if self._bm25 is None:
            # Degenerate corpus (e.g. dry-run with all-identical placeholders)
            # — skip retrieval, send the question with a stub.
            return Answer(
                qid=q.qid, pipeline=self.name,
                text="(no pages indexed)", citations=[],
            )
        scores = self._bm25.get_scores(_tokenize(q.text))
        top_idx = sorted(range(len(scores)), key=lambda i: scores[i], reverse=True)[: self.k]
        retrieved = [self._pages[i] for i in top_idx]

        pages_md = "\n\n---\n\n".join(text for _, text in retrieved)
        resp = self.client.complete(
            system=None,
            messages=[{
                "role": "user",
                "content": QUERY_PROMPT.format(pages=pages_md, question=q.text),
            }],
            max_tokens=256,
        )
        return Answer(
            qid=q.qid,
            pipeline=self.name,
            text=resp.text,
            citations=[pid for pid, _ in retrieved],
            usage=resp.usage,
        )
