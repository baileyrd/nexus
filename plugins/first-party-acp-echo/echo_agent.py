#!/usr/bin/env python3
"""Minimal ACP echo agent for BL-144 reference plugin testing.

Implements the bare-minimum line-delimited JSON-RPC 2.0 protocol the
`nexus-acp` host crate's [`AcpClient::initialize`] handshake requires:

* On `initialize`, respond with a capabilities object identifying the
  agent and advertising the single `"echo"` capability.
* On `propose`, echo the proposal back as a JSON-RPC notification
  (`agent/output`) so callers can verify the agent-pushed-event fan-out
  through `com.nexus.acp.agent.output` on the kernel bus.
* On `accept` / `reject` / any other method, return a no-op success.

Pure stdio, no external deps — runs against the system `python3`.
"""

import json
import sys


def _write(msg: dict) -> None:
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def _reply(req_id, result: dict | None = None, error: dict | None = None) -> None:
    msg: dict = {"jsonrpc": "2.0", "id": req_id}
    if error is not None:
        msg["error"] = error
    else:
        msg["result"] = result if result is not None else {}
    _write(msg)


def _notify(method: str, params: dict | None = None) -> None:
    _write({"jsonrpc": "2.0", "method": method, "params": params or {}})


def main() -> None:
    for raw_line in sys.stdin:
        line = raw_line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = msg.get("method")
        req_id = msg.get("id")
        if method == "initialize":
            _reply(
                req_id,
                {
                    "agentInfo": {"name": "echo", "version": "0.1.0"},
                    "capabilities": ["echo"],
                },
            )
        elif method == "exit":
            return
        elif method == "propose":
            params = msg.get("params") or {}
            _notify("agent/output", {"echoed": params})
            _reply(req_id, {"proposalId": "echo-1", "status": "queued"})
        elif method in ("accept", "reject"):
            _reply(req_id, {"status": "acknowledged"})
        elif req_id is not None:
            # Unknown method — JSON-RPC `method not found`.
            _reply(req_id, error={"code": -32601, "message": f"unknown method: {method}"})


if __name__ == "__main__":
    main()
