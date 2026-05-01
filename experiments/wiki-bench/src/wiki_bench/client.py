"""Anthropic SDK wrapper that records token usage and supports prompt caching.

The wrapper exists so every pipeline funnels through one place that:
- captures input/output/cache_creation/cache_read tokens uniformly,
- sets `cache_control` blocks deterministically when asked,
- can be stubbed in tests without monkey-patching the SDK.

No network calls happen until `complete()` is invoked.

`DryRunClient` is a drop-in replacement that estimates tokens from char counts
and records what *would* have been sent. Used by the `--dry-run` CLI mode to
project cost before spending real budget.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from .types import TokenUsage


# ~3.5 chars/token for English is a slight over-count vs Claude's actual
# tokenizer, biasing estimates slightly high (safer for budget planning).
CHARS_PER_TOKEN = 3.5


@dataclass
class LLMResponse:
    text: str
    usage: TokenUsage
    stop_reason: str | None = None


class LLMClient:
    def __init__(self, model: str = "claude-sonnet-4-6", api_key: str | None = None):
        self.model = model
        self._api_key = api_key
        self._client: Any = None  # lazy: anthropic.Anthropic instantiated on first call

    def _ensure_client(self) -> Any:
        if self._client is None:
            import anthropic  # local import so tests don't require network/install
            self._client = anthropic.Anthropic(api_key=self._api_key)
        return self._client

    def complete(
        self,
        system: str | list[dict[str, Any]] | None,
        messages: list[dict[str, Any]],
        max_tokens: int = 1024,
        cache_system: bool = False,
    ) -> LLMResponse:
        """Single non-streaming completion.

        Args:
            system: Plain string, or a list of content blocks. If `cache_system`
                is True and `system` is a string, the wrapper wraps it in one
                block with `cache_control: ephemeral`.
            messages: Standard Anthropic message list.
            max_tokens: passed through.
            cache_system: convenience flag for the common "cache the whole system
                prompt" pattern used by stuff-mode.

        Returns:
            LLMResponse with text and usage broken out by cache state.
        """
        client = self._ensure_client()

        if system is None:
            system_param: list[dict[str, Any]] | str = []
        elif isinstance(system, str):
            if cache_system and system:
                system_param = [{
                    "type": "text",
                    "text": system,
                    "cache_control": {"type": "ephemeral"},
                }]
            else:
                system_param = system
        else:
            # already a list of blocks; honor it as-is.
            system_param = system

        kwargs: dict[str, Any] = {
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": messages,
        }
        if system_param:
            kwargs["system"] = system_param

        resp = client.messages.create(**kwargs)

        text_parts = [b.text for b in resp.content if getattr(b, "type", None) == "text"]
        text = "".join(text_parts)

        u = resp.usage
        usage = TokenUsage(
            input_tokens=getattr(u, "input_tokens", 0) or 0,
            output_tokens=getattr(u, "output_tokens", 0) or 0,
            cache_creation_tokens=getattr(u, "cache_creation_input_tokens", 0) or 0,
            cache_read_tokens=getattr(u, "cache_read_input_tokens", 0) or 0,
        )

        return LLMResponse(text=text, usage=usage, stop_reason=resp.stop_reason)


def _approx_tokens(text: str) -> int:
    return max(1, int(len(text) / CHARS_PER_TOKEN))


@dataclass
class DryRunCall:
    system_chars: int
    messages_chars: int
    max_tokens: int
    cache_system: bool


class DryRunClient:
    """Pipeline-compatible client that estimates tokens without calling the API.

    Records every call so the runner can report (a) total estimated tokens and
    (b) per-pipeline shape for cost projection.
    """

    def __init__(self, model: str = "claude-sonnet-4-6", api_key: str | None = None):
        self.model = model
        self.calls: list[DryRunCall] = []

    def complete(
        self,
        system: str | list[dict[str, Any]] | None,
        messages: list[dict[str, Any]],
        max_tokens: int = 1024,
        cache_system: bool = False,
    ) -> LLMResponse:
        if system is None:
            sys_chars = 0
        elif isinstance(system, str):
            sys_chars = len(system)
        else:
            sys_chars = sum(len(b.get("text", "")) for b in system)

        msg_chars = 0
        for m in messages:
            c = m.get("content", "")
            if isinstance(c, str):
                msg_chars += len(c)
            else:
                for block in c:
                    if isinstance(block, dict):
                        msg_chars += len(block.get("text", ""))

        self.calls.append(DryRunCall(
            system_chars=sys_chars,
            messages_chars=msg_chars,
            max_tokens=max_tokens,
            cache_system=cache_system,
        ))

        # Output: assume ~half of max_tokens, capped — typical Claude responses
        # to short factual questions are 50-150 tokens.
        est_output = min(max_tokens, 150)
        # Input: system + messages, estimated.
        est_input = _approx_tokens(" " * (sys_chars + msg_chars))

        usage = TokenUsage(input_tokens=est_input, output_tokens=est_output)
        # Return a placeholder body that's distinguishable per call. Wiki ingest
        # writes this to disk and then BM25-indexes it; identical empty bodies
        # would cause a degenerate IDF computation.
        idx = len(self.calls)
        # Each placeholder must produce a distinctive token set so BM25 over
        # many dry-run pages has non-degenerate IDF. We emit per-call unique
        # tokens as well as some shared scaffolding.
        unique_tokens = " ".join(f"u{idx}_{k}" for k in range(20))
        placeholder = (
            f"# Synthetic page {idx}\n"
            f"Dry-run placeholder body for call number {idx}. {unique_tokens} "
            f"Topic of page {idx} discusses entity_{idx} and concept_{idx}."
        )
        return LLMResponse(
            text=placeholder,
            usage=usage,
            stop_reason="end_turn",
        )
