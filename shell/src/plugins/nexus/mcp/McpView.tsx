import { useMcpStore, type McpServerEntry, type ServerState, type ServerStatus } from './mcpStore'
import { Icon } from '../../../icons'

interface McpViewProps {
  onRefresh: () => void
  onConnect: (name: string) => void
  onDisconnect: (name: string) => void
  onExpand: (name: string) => void
  onCallTool: (server: string, tool: string) => void
}

/**
 * Sidebar listing of MCP servers configured in `mcp.toml`. Each row
 * collapses by default to name + command + status pill. Click to
 * expand: shell fires off three list_* IPC calls in parallel, which
 * also triggers an auto-connect kernel-side; tools / resources /
 * prompts populate inline. Connect / Disconnect buttons let the user
 * warm up a server without expanding, or tear one down explicitly.
 *
 * The kernel doesn't emit topic events for MCP state, so the shell
 * tracks status locally based on the outcome of its own invokes.
 */
export function McpView({ onRefresh, onConnect, onDisconnect, onExpand, onCallTool }: McpViewProps) {
  const loading = useMcpStore((s) => s.loading)
  const loadError = useMcpStore((s) => s.loadError)
  const servers = useMcpStore((s) => s.servers)
  const expandedName = useMcpStore((s) => s.expandedName)
  const state = useMcpStore((s) => s.state)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--background-primary)',
        color: 'var(--text-normal)',
        fontFamily: 'var(--font-interface)',
        fontSize: 'var(--ui-size, 13px)',
      }}
    >
      <Header onRefresh={onRefresh} loading={loading} count={servers.length} />
      <div style={{ flex: '1 1 auto', overflow: 'auto' }}>
        {loadError ? (
          <Centered colour="var(--risk)">{loadError}</Centered>
        ) : loading && servers.length === 0 ? (
          <Centered colour="var(--text-faint)">Loading…</Centered>
        ) : servers.length === 0 ? (
          <Centered colour="var(--text-faint)">
            No MCP servers. Add a <code>[servers.&lt;name&gt;]</code> entry to <code>.forge/mcp.toml</code>.
          </Centered>
        ) : (
          servers.map((srv) => (
            <ServerRow
              key={srv.name}
              server={srv}
              state={state[srv.name]}
              expanded={srv.name === expandedName}
              onToggle={() => onExpand(srv.name)}
              onConnect={() => onConnect(srv.name)}
              onDisconnect={() => onDisconnect(srv.name)}
              onCallTool={(tool) => onCallTool(srv.name, tool)}
            />
          ))
        )}
      </div>
    </div>
  )
}

interface HeaderProps {
  onRefresh: () => void
  loading: boolean
  count: number
}

function Header({ onRefresh, loading, count }: HeaderProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 10px',
        borderBottom: '1px solid var(--divider-color)',
        background: 'var(--background-secondary)',
        flex: '0 0 auto',
      }}
    >
      <span
        style={{
          flex: '1 1 auto',
          color: 'var(--text-muted)',
          fontSize: 11,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
        }}
      >
        MCP servers {count > 0 ? `(${count})` : ''}
      </span>
      <button
        type="button"
        aria-label="Refresh MCP servers"
        title="Reload mcp.toml"
        onClick={onRefresh}
        disabled={loading}
        onMouseEnter={(e) => {
          if (!loading) (e.currentTarget as HTMLButtonElement).style.background = 'var(--background-modifier-hover)'
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        }}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 24,
          height: 24,
          padding: 0,
          border: 0,
          background: 'transparent',
          color: 'var(--text-muted)',
          cursor: loading ? 'default' : 'pointer',
          borderRadius: 'var(--radius-s)',
          opacity: loading ? 0.5 : 1,
        }}
      >
        <Icon name="refresh" size={14} />
      </button>
    </div>
  )
}

interface ServerRowProps {
  server: McpServerEntry
  state: ServerState | undefined
  expanded: boolean
  onToggle: () => void
  onConnect: () => void
  onDisconnect: () => void
  onCallTool: (tool: string) => void
}

function ServerRow({ server, state, expanded, onToggle, onConnect, onDisconnect, onCallTool }: ServerRowProps) {
  const status: ServerStatus = server.disabled ? 'idle' : state?.status ?? 'idle'
  const busy = status === 'connecting' || status === 'disconnecting'
  return (
    <div style={{ borderBottom: '1px solid var(--divider-color)' }}>
      <div
        onClick={onToggle}
        role="button"
        aria-expanded={expanded}
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 4,
          padding: '8px 10px',
          cursor: 'pointer',
          background: expanded ? 'var(--background-secondary)' : 'transparent',
        }}
        onMouseEnter={(e) => {
          if (!expanded) (e.currentTarget as HTMLDivElement).style.background = 'var(--background-modifier-hover)'
        }}
        onMouseLeave={(e) => {
          if (!expanded) (e.currentTarget as HTMLDivElement).style.background = 'transparent'
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span
            aria-hidden
            style={{
              display: 'inline-flex',
              transition: 'transform 80ms',
              transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)',
              color: 'var(--text-faint)',
            }}
          >
            <Icon name="chev" size={12} />
          </span>
          <span
            style={{
              flex: '1 1 auto',
              fontWeight: 500,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={server.name}
          >
            {server.name}
          </span>
          <StatusPill status={status} disabled={server.disabled} error={state?.error ?? null} />
          {!server.disabled ? (
            <ConnectButton
              connected={status === 'connected'}
              busy={busy}
              onConnect={(e) => {
                e.stopPropagation()
                onConnect()
              }}
              onDisconnect={(e) => {
                e.stopPropagation()
                onDisconnect()
              }}
            />
          ) : null}
        </div>
        <div
          style={{
            paddingLeft: 18,
            color: 'var(--text-faint)',
            fontSize: 11,
            fontFamily: 'var(--font-monospace, monospace)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
          title={[server.command, ...server.args].join(' ')}
        >
          {server.command}
          {server.args.length > 0 ? ' ' + server.args.join(' ') : ''}
        </div>
      </div>
      {expanded ? <ExpandedPanel state={state} onCallTool={onCallTool} /> : null}
    </div>
  )
}

interface ConnectButtonProps {
  connected: boolean
  busy: boolean
  onConnect: (e: React.MouseEvent) => void
  onDisconnect: (e: React.MouseEvent) => void
}

function ConnectButton({ connected, busy, onConnect, onDisconnect }: ConnectButtonProps) {
  const label = connected ? 'Disconnect' : 'Connect'
  const onClick = connected ? onDisconnect : onConnect
  return (
    <button
      type="button"
      title={busy ? 'Working…' : label}
      aria-label={label}
      onClick={onClick}
      disabled={busy}
      onMouseEnter={(e) => {
        if (!busy) (e.currentTarget as HTMLButtonElement).style.background = 'var(--background-modifier-hover)'
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = 'var(--background-secondary)'
      }}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        padding: '0 6px',
        height: 20,
        background: 'var(--background-secondary)',
        color: connected ? 'var(--interactive-accent)' : 'var(--text-muted)',
        border: '1px solid var(--divider-color)',
        cursor: busy ? 'default' : 'pointer',
        borderRadius: 'var(--radius-s)',
        fontSize: 10,
        opacity: busy ? 0.5 : 1,
        flex: '0 0 auto',
      }}
    >
      {label}
    </button>
  )
}

function StatusPill({ status, disabled, error }: { status: ServerStatus; disabled: boolean; error: string | null }) {
  if (disabled) return <Pill bg="var(--text-faint)" label="disabled" title="disabled = true in mcp.toml" />
  switch (status) {
    case 'idle':
      return <Pill bg="var(--background-primary)" border label="idle" title="Not connected" />
    case 'connecting':
      return <Pill bg="var(--interactive-accent)" label="…" title="Connecting" />
    case 'connected':
      return <Pill bg="var(--ok)" label="up" title="Connected" />
    case 'disconnecting':
      return <Pill bg="var(--interactive-accent)" label="…" title="Disconnecting" />
    case 'error':
      return <Pill bg="var(--risk)" label="error" title={error ?? 'Error'} />
  }
}

function Pill({ bg, label, title, border }: { bg: string; label: string; title: string; border?: boolean }) {
  return (
    <span
      title={title}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        padding: '1px 6px',
        borderRadius: 999,
        fontSize: 10,
        background: bg,
        color: border ? 'var(--text-faint)' : 'var(--background-primary)',
        border: border ? '1px solid var(--divider-color)' : 'none',
        flex: '0 0 auto',
      }}
    >
      {label}
    </span>
  )
}

function ExpandedPanel({ state, onCallTool }: { state: ServerState | undefined; onCallTool: (tool: string) => void }) {
  if (!state) {
    return (
      <div style={{ padding: '8px 28px 12px', color: 'var(--text-faint)', fontSize: 11 }}>
        Connecting…
      </div>
    )
  }
  if (state.error && state.status === 'error') {
    return (
      <div
        style={{
          padding: '8px 10px 10px 28px',
          background: 'var(--background-secondary)',
          color: 'var(--risk)',
          fontSize: 11,
          lineHeight: 1.4,
        }}
      >
        {state.error}
      </div>
    )
  }
  if (state.loadingDetails) {
    return (
      <div style={{ padding: '8px 28px 12px', color: 'var(--text-faint)', fontSize: 11 }}>
        Loading capabilities…
      </div>
    )
  }
  return (
    <div
      style={{
        padding: '8px 10px 10px 28px',
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
        background: 'var(--background-secondary)',
      }}
    >
      <Section title="Tools" count={state.tools?.length ?? 0}>
        {(state.tools ?? []).map((t) => (
          <CapabilityRow
            key={t.name}
            name={t.name}
            description={t.description}
            onAction={() => onCallTool(t.name)}
            actionLabel="Call"
          />
        ))}
      </Section>
      <Section title="Resources" count={state.resources?.length ?? 0}>
        {(state.resources ?? []).map((r) => (
          <CapabilityRow key={r.uri} name={r.name || r.uri} description={r.description} />
        ))}
      </Section>
      <Section title="Prompts" count={state.prompts?.length ?? 0}>
        {(state.prompts ?? []).map((p) => (
          <CapabilityRow key={p.name} name={p.name} description={p.description} />
        ))}
      </Section>
    </div>
  )
}

function Section({ title, count, children }: { title: string; count: number; children: React.ReactNode }) {
  return (
    <div>
      <div
        style={{
          fontSize: 10,
          textTransform: 'uppercase',
          letterSpacing: '0.04em',
          color: 'var(--text-faint)',
          marginBottom: 4,
        }}
      >
        {title} ({count})
      </div>
      {count === 0 ? (
        <div style={{ color: 'var(--text-faint)', fontSize: 11, fontStyle: 'italic' }}>None.</div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>{children}</div>
      )}
    </div>
  )
}

function CapabilityRow({
  name,
  description,
  onAction,
  actionLabel,
}: {
  name: string
  description: string
  onAction?: () => void
  actionLabel?: string
}) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: onAction ? '1fr auto' : '1fr',
        gap: 6,
        alignItems: 'start',
        padding: '4px 8px',
        background: 'var(--background-primary)',
        border: '1px solid var(--divider-color)',
        borderRadius: 'var(--radius-s)',
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 1, minWidth: 0 }}>
        <span
          style={{
            fontFamily: 'var(--font-monospace, monospace)',
            fontSize: 11,
            color: 'var(--text-normal)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
          title={name}
        >
          {name}
        </span>
        {description ? (
          <span style={{ fontSize: 11, color: 'var(--text-faint)', lineHeight: 1.35 }}>
            {description}
          </span>
        ) : null}
      </div>
      {onAction ? (
        <button
          type="button"
          onClick={onAction}
          title={actionLabel}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            padding: '0 6px',
            height: 20,
            background: 'var(--background-secondary)',
            color: 'var(--text-muted)',
            border: '1px solid var(--divider-color)',
            borderRadius: 'var(--radius-s)',
            fontSize: 10,
            cursor: 'pointer',
            flex: '0 0 auto',
            alignSelf: 'flex-start',
          }}
        >
          {actionLabel}
        </button>
      ) : null}
    </div>
  )
}

function Centered({ colour, children }: { colour: string; children: React.ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        height: '100%',
        padding: 16,
        textAlign: 'center',
        color: colour,
        fontSize: 12,
        lineHeight: 1.4,
      }}
    >
      {children}
    </div>
  )
}
