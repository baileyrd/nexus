// shell/src/host/sandbox/index.ts
//
// WI-30b — public barrel for the sandbox RPC machinery. The
// orchestrator + iframe adapter (WI-30a/30d) and the guest bridge
// (WI-30d) consume from here; everything else in the shell should
// stay ignorant of sandbox internals.

export {
  SANDBOX_PROTOCOL_VERSION,
  isRpcEnvelope,
  isIpcErrorEnvelope,
  makeEvent,
  makeRequest,
  makeResponse,
  makeErrorResponse,
  makeHandshakeHello,
  makeHandshakeAccept,
  makeHandshakeReject,
  type HandshakeAccept,
  type HandshakeHello,
  type HandshakeReject,
  type HandshakePayload,
  type RpcDirection,
  type RpcEnvelope,
  type RpcErrorEnvelope,
  type RpcErrorKind,
  type RpcKind,
  type RpcResponseError,
  type SandboxProtocolVersion,
} from '@nexus/extension-api'

export {
  SANDBOX_METHOD_NAMES,
  SANDBOX_REJECTED_METHODS,
  type SandboxMethodCatalog,
  type SandboxMethodName,
  type NotificationShape,
  type StatusBarItemConfig,
  type ActivityBarItemConfig,
} from './methodCatalog'

export {
  METHOD_CAPABILITY_MAP,
  checkCapability,
  requiredCapabilityFor,
} from './capabilityGuard'

export {
  SandboxRouter,
  type SandboxPort,
  type SandboxRouterOptions,
} from './router'
