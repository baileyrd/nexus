"""Aggregate per-question results into a per-pipeline summary."""
from __future__ import annotations

from dataclasses import dataclass, field
from statistics import mean, median

from .types import RunResult, TokenUsage


@dataclass
class Summary:
    pipeline: str
    n_questions: int
    accuracy: float
    mean_partial: float
    ingest_usage: TokenUsage
    query_usage_total: TokenUsage
    judge_usage_total: TokenUsage
    median_latency_ms: float
    p95_latency_ms: float
    cost_usd_ingest: float
    cost_usd_query_total: float
    cost_usd_judge_total: float
    cost_usd_per_query: float
    breakeven_queries_vs: dict[str, float | None] = field(default_factory=dict)


def _percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    k = max(0, min(len(s) - 1, int(round(pct / 100.0 * (len(s) - 1)))))
    return s[k]


def summarize(run: RunResult, model: str, judge_model: str) -> Summary:
    n = len(run.answers)
    if n == 0:
        return Summary(
            pipeline=run.pipeline, n_questions=0, accuracy=0.0, mean_partial=0.0,
            ingest_usage=run.ingest_usage, query_usage_total=TokenUsage(),
            judge_usage_total=TokenUsage(), median_latency_ms=0.0, p95_latency_ms=0.0,
            cost_usd_ingest=0.0, cost_usd_query_total=0.0,
            cost_usd_judge_total=0.0, cost_usd_per_query=0.0,
        )

    j_correct = [j for j in run.judgments if j.correct]
    accuracy = len(j_correct) / max(1, len(run.judgments))
    mean_partial = mean(j.score for j in run.judgments) if run.judgments else 0.0

    query_usage_total = TokenUsage()
    for a in run.answers:
        query_usage_total = query_usage_total + a.usage
    judge_usage_total = TokenUsage()
    for j in run.judgments:
        judge_usage_total = judge_usage_total + j.judge_usage

    latencies = [a.latency_ms for a in run.answers]
    median_lat = median(latencies) if latencies else 0.0
    p95_lat = _percentile(latencies, 95)

    cost_ingest = run.ingest_usage.cost_usd(model)
    cost_query = query_usage_total.cost_usd(model)
    cost_judge = judge_usage_total.cost_usd(judge_model)
    cost_per_query = cost_query / n if n else 0.0

    return Summary(
        pipeline=run.pipeline,
        n_questions=n,
        accuracy=accuracy,
        mean_partial=mean_partial,
        ingest_usage=run.ingest_usage,
        query_usage_total=query_usage_total,
        judge_usage_total=judge_usage_total,
        median_latency_ms=median_lat,
        p95_latency_ms=p95_lat,
        cost_usd_ingest=cost_ingest,
        cost_usd_query_total=cost_query,
        cost_usd_judge_total=cost_judge,
        cost_usd_per_query=cost_per_query,
    )


def compare(summaries: list[Summary]) -> list[Summary]:
    """Compute breakeven query counts pairwise.

    For pipelines A, B with ingest cost I and per-query cost q:
        breakeven N* = (I_B - I_A) / (q_A - q_B)

    Only meaningful when the sign of I and q flip (one wins on ingest, the
    other on per-query). Returns None when one pipeline strictly dominates.
    """
    for a in summaries:
        a.breakeven_queries_vs = {}
        for b in summaries:
            if a.pipeline == b.pipeline:
                continue
            num = b.cost_usd_ingest - a.cost_usd_ingest
            den = a.cost_usd_per_query - b.cost_usd_per_query
            if abs(den) < 1e-12:
                a.breakeven_queries_vs[b.pipeline] = None
                continue
            n_star = num / den
            a.breakeven_queries_vs[b.pipeline] = n_star if n_star > 0 else None
    return summaries


def format_table(summaries: list[Summary]) -> str:
    """Fixed-width text table for stdout."""
    headers = [
        "pipeline", "n", "acc", "partial",
        "ingest$", "query$/q", "judge$tot",
        "med_ms", "p95_ms",
    ]
    rows = []
    for s in summaries:
        rows.append([
            s.pipeline,
            str(s.n_questions),
            f"{s.accuracy:.2%}",
            f"{s.mean_partial:.2f}",
            f"${s.cost_usd_ingest:.3f}",
            f"${s.cost_usd_per_query:.4f}",
            f"${s.cost_usd_judge_total:.3f}",
            f"{s.median_latency_ms:.0f}",
            f"{s.p95_latency_ms:.0f}",
        ])
    widths = [max(len(h), max((len(r[i]) for r in rows), default=0)) for i, h in enumerate(headers)]
    fmt = "  ".join(f"{{:<{w}}}" for w in widths)
    lines = [fmt.format(*headers), fmt.format(*["-" * w for w in widths])]
    for r in rows:
        lines.append(fmt.format(*r))
    if any(s.breakeven_queries_vs for s in summaries):
        lines.append("")
        lines.append("Breakeven query counts (rows = A, cols = B; N* where A overtakes B):")
        names = [s.pipeline for s in summaries]
        be_widths = [max(8, max((len(n) for n in names), default=0))] + [
            max(8, len(n)) for n in names
        ]
        be_fmt = "  ".join(f"{{:<{w}}}" for w in be_widths)
        lines.append(be_fmt.format("A \\ B", *names))
        for s in summaries:
            cells = []
            for n in names:
                if n == s.pipeline:
                    cells.append("—")
                else:
                    v = s.breakeven_queries_vs.get(n)
                    cells.append("dom" if v is None else f"{v:.0f}")
            lines.append(be_fmt.format(s.pipeline, *cells))
    return "\n".join(lines)
