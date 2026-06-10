# Capability Inventory

> Auto-generated from `nexus_kernel::Capability::ALL` + `nexus_security::risk::risk_level`. Do not edit by hand — regenerate via `scripts/check_ipc_drift.sh`.

Filed under [BL-137](../PRDs/backlog/BL-137.md).

This is the canonical surface used at install time and at every kernel-mediated
operation. ADR 0002 and ADR 0022 carry the rationale; this file is the live
mirror.

| String | Variant | Risk |
|--------|---------|------|
| `fs.read` | `FsRead` | Low |
| `fs.write` | `FsWrite` | Medium |
| `fs.read.external` | `FsReadExternal` | **High** |
| `fs.write.external` | `FsWriteExternal` | **High** |
| `net.http` | `NetHttp` | **High** |
| `net.http.localhost` | `NetHttpLocalhost` | Medium |
| `process.spawn` | `ProcessSpawn` | **High** |
| `kv.read` | `KvRead` | Low |
| `kv.write` | `KvWrite` | Low |
| `ipc.call` | `IpcCall` | **High** |
| `db.query` | `DbQuery` | Medium |
| `db.write` | `DbWrite` | Medium |
| `events.publish` | `EventsPublish` | Medium |
| `ui.notify` | `UiNotify` | Low |
| `ai.chat` | `AiChat` | Medium |
| `ai.index` | `AiIndex` | Low |
| `ai.session.read` | `AiSessionRead` | Low |
| `ai.session.write` | `AiSessionWrite` | Low |
| `ai.config.write` | `AiConfigWrite` | **High** |
| `ai.activity.write` | `AiActivityWrite` | Medium |
| `ai.tools.write` | `AiToolsWrite` | Medium |
| `ai.tools.mcp` | `AiToolsMcp` | Medium |
| `audio.record` | `AudioRecord` | **High** |
| `audio.synthesize` | `AudioSynthesize` | Low |
| `ai.runtime.submit` | `AiRuntimeSubmit` | Medium |
| `ai.runtime.control` | `AiRuntimeControl` | Medium |
| `ai.runtime.observe` | `AiRuntimeObserve` | Low |
| `notifications.inbox.read` | `NotificationsInboxRead` | Low |
| `notifications.inbox.write` | `NotificationsInboxWrite` | Low |
| `protocol.host.contribute` | `ProtocolHostContribute` | **High** |
| `security.write` | `SecurityWrite` | **High** |
| `security.audit.write` | `SecurityAuditWrite` | **High** |
| `security.audit.read` | `SecurityAuditRead` | Medium |
| `network.bind` | `NetworkBind` | **High** |

_Total: 34 capabilities._
