// SH-001: React error boundary for plugin slot contributions.
//
// Every top-level shell region (activityBar, statusBar, overlay, workspace)
// is wrapped in its own boundary so a render-time throw inside one plugin's
// contribution does not unmount the rest of the chrome.
//
// Errors are forwarded to `clientLogger` once it exists (SH-017); until
// then they land on `console.error` which is the same output path as before.

import { Component, type ErrorInfo, type ReactNode } from 'react'

interface Props {
  /** Identifies this boundary in logs and the fallback UI. */
  name: string
  children: ReactNode
  /** Optional custom fallback; defaults to the inline recover affordance. */
  fallback?: ReactNode
}

interface State {
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    // Forward to clientLogger once SH-017 lands; for now mirror to console.
    console.error(
      `[ErrorBoundary:${this.props.name}] Caught render error`,
      error,
      info.componentStack,
    )
  }

  private handleReload = () => {
    window.location.reload()
  }

  private handleDismiss = () => {
    this.setState({ error: null })
  }

  render() {
    const { error } = this.state
    if (!error) return this.props.children

    if (this.props.fallback !== undefined) return this.props.fallback

    return (
      <div
        role="alert"
        style={{
          padding: '12px 16px',
          background: 'var(--background-modifier-error, #3d1a1a)',
          color: 'var(--text-error, #f48771)',
          fontSize: '12px',
          fontFamily: 'var(--font-monospace, monospace)',
          borderRadius: '4px',
          display: 'flex',
          flexDirection: 'column',
          gap: '8px',
        }}
      >
        <div style={{ fontWeight: 600 }}>
          [{this.props.name}] Plugin render error
        </div>
        <div style={{ opacity: 0.8, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
          {error.message}
        </div>
        <div style={{ display: 'flex', gap: '8px' }}>
          <button
            onClick={this.handleDismiss}
            style={{
              padding: '4px 8px',
              fontSize: '11px',
              cursor: 'pointer',
              background: 'var(--interactive-normal, #3c3c3c)',
              color: 'var(--text-normal, #cccccc)',
              border: 'none',
              borderRadius: '3px',
            }}
          >
            Dismiss
          </button>
          <button
            onClick={this.handleReload}
            style={{
              padding: '4px 8px',
              fontSize: '11px',
              cursor: 'pointer',
              background: 'var(--interactive-accent, #7c3aed)',
              color: 'var(--text-on-accent, #ffffff)',
              border: 'none',
              borderRadius: '3px',
            }}
          >
            Reload window
          </button>
        </div>
      </div>
    )
  }
}
