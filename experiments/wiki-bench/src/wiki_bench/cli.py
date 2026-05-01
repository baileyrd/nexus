"""CLI entry point.

    python -m wiki_bench run --pipeline stuff --n-questions 50
    python -m wiki_bench run --pipeline rag   --n-questions 50
    python -m wiki_bench run --pipeline wiki  --n-questions 50
    python -m wiki_bench summary --results-dir results/
"""
from __future__ import annotations

import argparse
import re
from pathlib import Path

PIPELINE_CHOICES = ["stuff", "stuff-nocache", "rag", "wiki"]


def _load_dotenv() -> None:
    """Load .env if present. Soft dependency on python-dotenv."""
    try:
        from dotenv import load_dotenv  # type: ignore
        load_dotenv()
    except ImportError:
        pass


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="wiki-bench")
    sub = p.add_subparsers(dest="cmd", required=True)

    run_p = sub.add_parser("run", help="Run a pipeline against HotpotQA")
    run_p.add_argument("--pipeline", choices=PIPELINE_CHOICES, required=True)
    run_p.add_argument("--n-questions", type=int, default=50)
    run_p.add_argument("--seed", type=int, default=42)
    run_p.add_argument("--model", default="claude-sonnet-4-6")
    run_p.add_argument("--judge-model", default="claude-haiku-4-5")
    run_p.add_argument("--out-dir", type=Path, default=Path("results"))
    run_p.add_argument(
        "--wiki-dir", type=Path, default=None,
        help="Wiki page directory (wiki pipeline only). Reused across runs for free re-queries.",
    )

    dry_p = sub.add_parser(
        "dry-run",
        help="Estimate cost without calling the API. Builds prompts, counts chars, projects $.",
    )
    dry_p.add_argument(
        "--pipeline",
        choices=PIPELINE_CHOICES + ["all"],
        default="all",
        help="Which pipeline(s) to estimate. 'all' runs each in turn.",
    )
    dry_p.add_argument("--n-questions", type=int, default=20)
    dry_p.add_argument("--seed", type=int, default=42)
    dry_p.add_argument("--model", default="claude-sonnet-4-6")
    dry_p.add_argument("--judge-model", default="claude-haiku-4-5")
    dry_p.add_argument("--out-dir", type=Path, default=Path("results"))
    dry_p.add_argument(
        "--synthetic", action="store_true",
        help="Use a synthetic corpus calibrated to HotpotQA shape (no network).",
    )

    sum_p = sub.add_parser("summary", help="Aggregate JSONL results into a table")
    sum_p.add_argument("--results-dir", type=Path, default=Path("results"))
    sum_p.add_argument("--model", default="claude-sonnet-4-6",
                       help="Model the answer pipelines used (for cost rates).")
    sum_p.add_argument("--judge-model", default="claude-haiku-4-5",
                       help="Model the judge used (for cost rates).")

    args = p.parse_args(argv)
    _load_dotenv()

    if args.cmd == "run":
        from .runner import run as do_run
        do_run(
            pipeline_name=args.pipeline,
            n_questions=args.n_questions,
            out_dir=args.out_dir,
            model=args.model,
            judge_model=args.judge_model,
            seed=args.seed,
            wiki_dir=args.wiki_dir,
        )
        return 0

    if args.cmd == "dry-run":
        from .runner import dry_run_estimate, format_dry_run

        pipelines = PIPELINE_CHOICES if args.pipeline == "all" else [args.pipeline]
        estimates = []
        for name in pipelines:
            estimates.append(dry_run_estimate(
                pipeline_name=name,
                n_questions=args.n_questions,
                out_dir=args.out_dir,
                model=args.model,
                judge_model=args.judge_model,
                seed=args.seed,
                synthetic=args.synthetic,
            ))
        print(format_dry_run(estimates))
        return 0

    if args.cmd == "summary":
        from .metrics import compare, format_table, summarize
        from .runner import load_run

        results_dir: Path = args.results_dir
        if not results_dir.exists():
            print(f"results dir not found: {results_dir}")
            return 1

        # File pattern: <pipeline>_<seed>_<n>.jsonl
        pat = re.compile(r"^(?P<pipeline>[a-z\-]+)_(?P<seed>\d+)_(?P<n>\d+)\.jsonl$")
        summaries = []
        for jsonl in sorted(results_dir.glob("*.jsonl")):
            m = pat.match(jsonl.name)
            if not m:
                continue
            pipeline = m.group("pipeline")
            run = load_run(jsonl, pipeline)
            summaries.append(summarize(run, model=args.model, judge_model=args.judge_model))

        if not summaries:
            print(f"no result JSONLs found in {results_dir}")
            return 1

        compare(summaries)
        print(format_table(summaries))
        return 0

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
