"""HotpotQA loader.

We use the `distractor` setting: each example carries 10 paragraphs (2 supporting
+ 8 distractors). The benchmark corpus for a run is the union of paragraphs
across the sampled questions, so the wiki/RAG pipelines have realistic noise.

Cached samples land in `data/hotpot_sample_{seed}_{n}.json` so multiple pipeline
runs over the same (seed, n) reuse byte-identical inputs.

For environments without HuggingFace access (CI, sandboxes), `load_synthetic`
returns a corpus calibrated to HotpotQA's typical paragraph + question shape.
Use it for cost projections, NOT for measuring quality.
"""
from __future__ import annotations

import json
import random
import re
from dataclasses import asdict
from pathlib import Path

from .types import Document, Question


def _slug(s: str) -> str:
    s = re.sub(r"[^\w\-]+", "_", s.strip())
    return re.sub(r"_+", "_", s).strip("_") or "untitled"


def _cache_path(data_dir: Path, seed: int, n: int, split: str) -> Path:
    return data_dir / f"hotpot_{split}_{seed}_{n}.json"


def load_hotpotqa(
    n_questions: int = 50,
    split: str = "validation",
    seed: int = 42,
    data_dir: Path | None = None,
) -> tuple[list[Document], list[Question]]:
    """Load a deterministic sample of HotpotQA.

    On first call: downloads via `datasets`, samples `n_questions` examples,
    flattens paragraphs into a unique-by-(title) doc list, and caches the
    result to `data_dir/hotpot_<split>_<seed>_<n>.json`.

    Subsequent calls with the same (seed, n, split) load from cache without
    network access.
    """
    data_dir = (data_dir or Path("data")).resolve()
    data_dir.mkdir(parents=True, exist_ok=True)
    cache = _cache_path(data_dir, seed, n_questions, split)

    if cache.exists():
        payload = json.loads(cache.read_text())
        docs = [Document(**d) for d in payload["documents"]]
        questions = [
            Question(
                qid=q["qid"],
                text=q["text"],
                gold_answer=q["gold_answer"],
                supporting_doc_ids=tuple(q["supporting_doc_ids"]),
            )
            for q in payload["questions"]
        ]
        return docs, questions

    from datasets import load_dataset  # local import keeps test imports cheap
    ds = load_dataset("hotpot_qa", "distractor", split=split)

    rng = random.Random(seed)
    idxs = rng.sample(range(len(ds)), n_questions)

    docs: dict[str, Document] = {}
    questions: list[Question] = []

    for i in idxs:
        ex = ds[int(i)]
        ctx_titles: list[str] = ex["context"]["title"]
        ctx_sents: list[list[str]] = ex["context"]["sentences"]

        for title, sents in zip(ctx_titles, ctx_sents):
            doc_id = _slug(title)
            if doc_id in docs:
                continue
            text = " ".join(sents).strip()
            docs[doc_id] = Document(doc_id=doc_id, title=title, text=text)

        sup_titles = ex["supporting_facts"]["title"]
        seen: set[str] = set()
        sup_doc_ids: list[str] = []
        for t in sup_titles:
            sid = _slug(t)
            if sid in docs and sid not in seen:
                seen.add(sid)
                sup_doc_ids.append(sid)

        questions.append(Question(
            qid=str(ex["id"]),
            text=ex["question"],
            gold_answer=ex["answer"],
            supporting_doc_ids=tuple(sup_doc_ids),
        ))

    payload = {
        "split": split,
        "seed": seed,
        "n_questions": n_questions,
        "documents": [asdict(d) for d in docs.values()],
        "questions": [
            {
                "qid": q.qid,
                "text": q.text,
                "gold_answer": q.gold_answer,
                "supporting_doc_ids": list(q.supporting_doc_ids),
            }
            for q in questions
        ],
    }
    cache.write_text(json.dumps(payload, indent=2, ensure_ascii=False))
    return list(docs.values()), questions


# Calibrated to HotpotQA distractor stats: ~10 paragraphs per question, mostly
# unique across questions (some entity reuse), ~100-word paragraphs.
_SYNTH_PARAGRAPH = (
    "Notable historical figure {entity} was born in the early period and is "
    "best known for contributions to the field of {field}. {entity} worked at "
    "{org} between specific years and authored several landmark works that "
    "influenced subsequent thinkers. Critics have noted that {entity}'s style "
    "shifted markedly after a pivotal event, drawing from earlier traditions "
    "while introducing distinctly modern elements. Later in life, {entity} "
    "retired to a quiet town and continued to correspond with peers including "
    "other notable figures in {field}, leaving behind an extensive archive of "
    "letters and unpublished essays that scholars continue to examine. The "
    "legacy of {entity} remains a subject of debate among historians of {field}."
)
_SYNTH_FIELDS = ["physics", "literature", "music", "philosophy", "biology", "economics"]
_SYNTH_ORGS = ["the University of Cambridge", "the Royal Society", "Bell Labs",
               "the Sorbonne", "MIT", "the Vienna Circle"]


def load_synthetic(
    n_questions: int = 20,
    seed: int = 42,
    paragraphs_per_question: int = 10,
) -> tuple[list[Document], list[Question]]:
    """Generate a corpus calibrated to HotpotQA distractor shape. No network."""
    rng = random.Random(seed)
    docs: dict[str, Document] = {}
    questions: list[Question] = []

    for i in range(n_questions):
        sup_titles: list[str] = []
        for j in range(paragraphs_per_question):
            entity = f"Entity_{i:03d}_{j:02d}"
            field = rng.choice(_SYNTH_FIELDS)
            org = rng.choice(_SYNTH_ORGS)
            text = _SYNTH_PARAGRAPH.format(entity=entity, field=field, org=org)
            doc_id = entity
            if doc_id not in docs:
                docs[doc_id] = Document(doc_id=doc_id, title=entity, text=text)
            if j < 2:  # first two paragraphs are "supporting"
                sup_titles.append(doc_id)
        questions.append(Question(
            qid=f"synth_{i:03d}",
            text=(
                f"In what field did {sup_titles[0]} work, "
                f"and which organization did {sup_titles[1]} attend?"
            ),
            gold_answer="varies by entity",
            supporting_doc_ids=tuple(sup_titles),
        ))

    return list(docs.values()), questions
