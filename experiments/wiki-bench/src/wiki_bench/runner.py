"""Orchestration: load corpus, ingest, query, judge, persist.

Results are appended to JSONL after every question so a Ctrl-C (or a hard fail
in the middle of run #87/100) doesn't lose the previous 86. Re-running with
the same `--out-dir` and corpus seed picks up where it left off — already-
judged qids are skipped on resume.
"""
from __future__ import annotations

import json
import time
from dataclasses import asdict
from pathlib import Path

from tqdm import tqdm

from .client import DryRunClient, LLMClient
from .corpus import load_hotpotqa, load_synthetic
from .judge import judge_answer
from .pipelines import Pipeline, RagPipeline, StuffPipeline, WikiPipeline
from .pricing import rate as price_rate
from .types import Answer, JudgeResult, RunResult, TokenUsage


def _build_pipeline(name: str, client: LLMClient, wiki_dir: Path) -> Pipeline:
    if name == "stuff":
        return StuffPipeline(client, cache_system=True)
    if name == "stuff-nocache":
        return StuffPipeline(client, cache_system=False)
    if name == "rag":
        return RagPipeline(client, k=5, chunk_tokens=500)
    if name == "wiki":
        return WikiPipeline(client, wiki_dir=wiki_dir, k=5)
    raise ValueError(f"unknown pipeline: {name}")


def _jsonl_path(out_dir: Path, pipeline: str, seed: int, n: int) -> Path:
    return out_dir / f"{pipeline}_{seed}_{n}.jsonl"


def _load_existing(jsonl: Path) -> tuple[set[str], list[Answer], list[JudgeResult]]:
    """Read a partial JSONL, return (already-judged qids, hydrated answers, hydrated judgments)."""
    qids: set[str] = set()
    answers: list[Answer] = []
    judgments: list[JudgeResult] = []
    if not jsonl.exists():
        return qids, answers, judgments
    for line in jsonl.read_text().splitlines():
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        a = row.get("answer") or {}
        j = row.get("judgment") or {}
        qid = row.get("qid") or a.get("qid")
        if not qid:
            continue
        qids.add(qid)
        try:
            answers.append(Answer(
                qid=a["qid"], pipeline=a["pipeline"], text=a["text"],
                citations=list(a.get("citations") or []),
                usage=TokenUsage(**a.get("usage", {})),
                latency_ms=float(a.get("latency_ms", 0.0)),
            ))
            judgments.append(JudgeResult(
                qid=j["qid"], pipeline=j["pipeline"], correct=bool(j["correct"]),
                score=float(j["score"]), reasoning=str(j.get("reasoning", "")),
                judge_usage=TokenUsage(**j.get("judge_usage", {})),
            ))
        except (KeyError, TypeError, ValueError):
            # Malformed row → drop from in-memory aggregates but still skip the qid.
            continue
    return qids, answers, judgments


def run(
    pipeline_name: str,
    n_questions: int,
    out_dir: Path,
    model: str = "claude-sonnet-4-6",
    judge_model: str = "claude-haiku-4-5",
    seed: int = 42,
    wiki_dir: Path | None = None,
) -> RunResult:
    out_dir = Path(out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    jsonl = _jsonl_path(out_dir, pipeline_name, seed, n_questions)

    docs, questions = load_hotpotqa(n_questions=n_questions, seed=seed)

    answer_client = LLMClient(model=model)
    judge_client = LLMClient(model=judge_model)

    wiki_dir = wiki_dir or (out_dir / f"wiki_{seed}_{n_questions}")
    pipeline = _build_pipeline(pipeline_name, answer_client, wiki_dir=wiki_dir)

    # Resume: skip qids already in the JSONL.
    done_qids, answers, judgments = _load_existing(jsonl)

    pipeline.ingest(docs)

    pending = [q for q in questions if q.qid not in done_qids]
    if pending:
        with jsonl.open("a", encoding="utf-8") as fp, tqdm(pending, desc=pipeline_name) as bar:
            for q in bar:
                t0 = time.monotonic()
                ans = pipeline.query(q)
                ans.latency_ms = (time.monotonic() - t0) * 1000.0
                judgment = judge_answer(judge_client, q, ans)
                fp.write(json.dumps({
                    "qid": q.qid,
                    "answer": asdict(ans),
                    "judgment": asdict(judgment),
                }, ensure_ascii=False) + "\n")
                fp.flush()
                answers.append(ans)
                judgments.append(judgment)

    return RunResult(
        pipeline=pipeline_name,
        answers=answers,
        judgments=judgments,
        ingest_usage=pipeline.ingest_usage,
    )


def dry_run_estimate(
    pipeline_name: str,
    n_questions: int,
    out_dir: Path,
    model: str = "claude-sonnet-4-6",
    judge_model: str = "claude-haiku-4-5",
    seed: int = 42,
    wiki_dir: Path | None = None,
    synthetic: bool = False,
) -> dict:
    """Run ingest + queries through a DryRunClient. No API calls.

    Returns a dict with token totals and projected cost — both uncached and,
    for stuff-mode, with realistic 5-min cache modeling (first query writes
    the cache, subsequent reads at ~10%).

    If `synthetic=True`, uses a synthetic corpus calibrated to HotpotQA shape
    instead of fetching from HuggingFace. Use for cost-only projections in
    environments without dataset access.
    """
    out_dir = Path(out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    if synthetic:
        docs, questions = load_synthetic(n_questions=n_questions, seed=seed)
    else:
        docs, questions = load_hotpotqa(n_questions=n_questions, seed=seed)

    answer_client = DryRunClient(model=model)
    wiki_dir = wiki_dir or (out_dir / f"wiki_{seed}_{n_questions}_dryrun")
    pipeline = _build_pipeline(pipeline_name, answer_client, wiki_dir=wiki_dir)

    pipeline.ingest(docs)
    n_ingest_calls = len(answer_client.calls)
    ingest_tokens_in = sum(_approx(c.system_chars + c.messages_chars) for c in answer_client.calls)
    ingest_tokens_out = sum(min(c.max_tokens, 150) for c in answer_client.calls)

    query_calls_start = len(answer_client.calls)
    for q in questions:
        pipeline.query(q)
    query_calls = answer_client.calls[query_calls_start:]
    n_query_calls = len(query_calls)
    query_tokens_in_uncached = sum(
        _approx(c.system_chars + c.messages_chars) for c in query_calls
    )
    query_tokens_out = sum(min(c.max_tokens, 150) for c in query_calls)

    # Cache modeling for stuff: if cache_system=True and the system prompt is
    # identical across queries, query 1 writes the cache; queries 2..N read.
    cached_q_calls = [c for c in query_calls if c.cache_system and c.system_chars > 0]
    if cached_q_calls:
        sys_tokens_each = _approx(cached_q_calls[0].system_chars)
        user_tokens_each_avg = (
            sum(_approx(c.messages_chars) for c in cached_q_calls) / len(cached_q_calls)
        )
        cache_creation = sys_tokens_each
        cache_read = sys_tokens_each * (len(cached_q_calls) - 1)
        non_system_in = int(user_tokens_each_avg * len(cached_q_calls))
    else:
        cache_creation = 0
        cache_read = 0
        non_system_in = query_tokens_in_uncached

    # Judge cost: one call per question. Assume input ≈ 400 tokens
    # (question + gold + prediction + system), output ≈ 80 tokens.
    judge_in = 400 * n_questions
    judge_out = 80 * n_questions

    r = price_rate(model)
    rj = price_rate(judge_model)
    if r is None or rj is None:
        raise ValueError(f"unknown model in pricing: {model} or {judge_model}")

    cost_uncached = (
        (ingest_tokens_in + query_tokens_in_uncached) * r["in"]
        + (ingest_tokens_out + query_tokens_out) * r["out"]
    ) / 1_000_000
    cost_cached = (
        ingest_tokens_in * r["in"]
        + ingest_tokens_out * r["out"]
        + cache_creation * r["cache_w"]
        + cache_read * r["cache_r"]
        + non_system_in * r["in"]
        + query_tokens_out * r["out"]
    ) / 1_000_000
    cost_judge = (judge_in * rj["in"] + judge_out * rj["out"]) / 1_000_000

    return {
        "pipeline": pipeline_name,
        "n_questions": n_questions,
        "n_ingest_calls": n_ingest_calls,
        "n_query_calls": n_query_calls,
        "ingest_tokens_in": ingest_tokens_in,
        "ingest_tokens_out": ingest_tokens_out,
        "query_tokens_in_uncached": query_tokens_in_uncached,
        "query_tokens_in_cached_avg": non_system_in,
        "query_tokens_out": query_tokens_out,
        "cache_creation_tokens": cache_creation,
        "cache_read_tokens": cache_read,
        "judge_tokens_in": judge_in,
        "judge_tokens_out": judge_out,
        "cost_usd_uncached": cost_uncached,
        "cost_usd_cached": cost_cached,
        "cost_usd_judge": cost_judge,
        "cost_usd_total_uncached": cost_uncached + cost_judge,
        "cost_usd_total_cached": cost_cached + cost_judge,
    }


def _approx(chars: int) -> int:
    from .client import CHARS_PER_TOKEN
    return max(1, int(chars / CHARS_PER_TOKEN)) if chars > 0 else 0


def format_dry_run(estimates: list[dict]) -> str:
    """Render dry-run projections as a table."""
    headers = [
        "pipeline", "n", "ingest_calls", "ingest_tok_in",
        "query_tok_in", "query_tok_out", "judge_tok",
        "cost_uncached$", "cost_cached$",
    ]
    rows = []
    for e in estimates:
        rows.append([
            e["pipeline"],
            str(e["n_questions"]),
            str(e["n_ingest_calls"]),
            f"{e['ingest_tokens_in']:,}",
            f"{e['query_tokens_in_uncached']:,}",
            f"{e['query_tokens_out']:,}",
            f"{e['judge_tokens_in'] + e['judge_tokens_out']:,}",
            f"${e['cost_usd_total_uncached']:.3f}",
            f"${e['cost_usd_total_cached']:.3f}",
        ])
    widths = [
        max(len(h), max((len(r[i]) for r in rows), default=0))
        for i, h in enumerate(headers)
    ]
    fmt = "  ".join(f"{{:<{w}}}" for w in widths)
    lines = [fmt.format(*headers), fmt.format(*["-" * w for w in widths])]
    for r in rows:
        lines.append(fmt.format(*r))
    total_uncached = sum(e["cost_usd_total_uncached"] for e in estimates)
    total_cached = sum(e["cost_usd_total_cached"] for e in estimates)
    lines.append("")
    lines.append(f"TOTAL across all pipelines: ${total_uncached:.2f} uncached / ${total_cached:.2f} cached")
    lines.append(
        "Notes: token estimates use ~3.5 chars/token (slightly biased high for safety)."
    )
    lines.append(
        "       'cached' models 5-min ephemeral cache for stuff-mode only; rag/wiki are unaffected."
    )
    return "\n".join(lines)


def load_run(jsonl: Path, pipeline_name: str) -> RunResult:
    """Hydrate a RunResult from a JSONL on disk. ingest_usage is not persisted
    in the per-question JSONL, so it comes back as zero — only meaningful when
    paired with a .summary.json sibling."""
    _, answers, judgments = _load_existing(jsonl)
    return RunResult(
        pipeline=pipeline_name,
        answers=answers,
        judgments=judgments,
        ingest_usage=TokenUsage(),
    )
