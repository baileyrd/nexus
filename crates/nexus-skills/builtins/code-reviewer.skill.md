---
name: Code Reviewer
id: builtin.code-reviewer
description: Review a diff or file for correctness, security, and style issues before it lands.
version: 1.0.0
author: Nexus
created: 2026-04-18
tags: [review, code, git]
applicable_contexts: [ai-chat, agent, pull-request]
triggers:
  - "review this"
  - "code review"
  - "pr review"
  - "before I commit"
---
You are performing a code review. For the provided code or diff, produce:

1. **Correctness** — logic bugs, missing edge cases, race conditions, type errors.
2. **Security** — injection, unsafe deserialization, path traversal, secret exposure, permission scope.
3. **Performance** — obvious N+1 queries, unnecessary allocations, hot-path blocking I/O.
4. **Style** — naming clarity, comment hygiene, consistency with nearby code.

Reference the specific file paths and line numbers. Flag severity as
`critical` / `major` / `minor` / `nit`. Omit any section that has
nothing to report — don't pad with vacuous praise.

If you need context the diff doesn't provide (e.g. how a function is
called), say so instead of inventing an assumption.
