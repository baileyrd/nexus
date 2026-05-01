# wiki-bench

Benchmarks three approaches to giving an LLM access to a corpus, on the
HotpotQA multi-hop QA dataset:

1. **stuff** — dump the entire corpus into the system prompt every query
   (with and without prompt caching).
2. **rag** — chunk the corpus, BM25-index it, retrieve top-k chunks per query.
3. **wiki** — Karpathy-style: LLM ingests each source into cross-linked
   markdown pages once, retrieves pages at query time.

## Goals

- Numbers for input/output/cache tokens, $, latency, and answer correctness
  per pipeline, on a fixed corpus + question set.
- Clear breakeven analysis: at what query count does the wiki's upfront ingest
  cost amortize?
- Honest accounting: prompt caching radically changes the math, so every run
  reports both cached and uncached cost where applicable.

## Status

Skeleton only. Interfaces and orchestration are wired; pipeline bodies and the
LLM client wrapper raise `NotImplementedError`. No API calls are made yet.

## Layout

```
src/wiki_bench/
├── types.py            # Document, Question, Answer, TokenUsage, RunResult, JudgeResult
├── client.py           # Anthropic SDK wrapper; tracks usage; toggleable prompt caching
├── corpus.py           # HotpotQA loader (datasets library)
├── pipelines/
│   ├── base.py         # Pipeline ABC: ingest(docs) + query(q) -> Answer
│   ├── stuff.py        # Concatenate-everything baseline
│   ├── rag.py          # BM25 chunk retrieval
│   └── wiki.py         # Karpathy-style ingest + page retrieval
├── judge.py            # LLM-as-judge for answer correctness
├── metrics.py          # Aggregate RunResults into a summary table
├── runner.py           # Orchestration: load corpus, ingest, query, judge, persist
└── cli.py              # python -m wiki_bench run --pipeline stuff
```

## Running (once implemented)

```bash
pip install -e .
cp .env.example .env  # add your ANTHROPIC_API_KEY
python -m wiki_bench run --pipeline stuff --n-questions 50
python -m wiki_bench run --pipeline rag   --n-questions 50
python -m wiki_bench run --pipeline wiki  --n-questions 50
python -m wiki_bench summary
```

Results land as JSONL in `results/`, one line per question per pipeline,
crash-safe.

## Methodology notes

- **Corpus**: HotpotQA dev split, distractor setting. Each question comes with
  ~10 paragraphs (2 supporting, 8 distractors). The corpus for a run is the
  union across the sampled questions.
- **Judging**: LLM-as-judge (Haiku by default) compares predicted vs gold
  answer. Binary correctness + a `[0,1]` partial-credit score. We sanity-check
  judge agreement on a random sample by hand.
- **Caching**: stuff-mode runs both with and without `cache_control` on the
  system prompt so the cached/uncached economics are visible.
- **Determinism**: temperature 0; fixed seed for question sampling.
