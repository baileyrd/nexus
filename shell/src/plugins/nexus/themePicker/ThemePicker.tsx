import { useEffect, useMemo, useRef, useState } from 'react'
import {
  useThemeStore,
  type ThemeMode,
  type AvailableSnippet,
  THEME_PLUGIN_ID,
} from '../../../stores/themeStore'
import {
  useThemePickerStore,
  SWATCH_KEYS,
  type CategoryFilter,
  type ThemeCard,
  type PickerTab,
} from './themePickerStore'
import { getPickerApi } from './pickerRuntime'
import { ThemeBuilderPanel } from './ThemeBuilder'
import { builderModalWidth } from './previewTokens'

// ── Constants ─────────────────────────────────────────────────────────────────

const CATEGORY_LABELS: Record<CategoryFilter, string> = {
  all: 'All',
  light: 'Light',
  dark: 'Dark',
  sepia: 'Sepia',
  'high-contrast': 'High Contrast',
  custom: 'Custom',
}

const MODE_OPTIONS: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
]

// ── ThemePicker (entry point) ─────────────────────────────────────────────────

export function ThemePicker() {
  const visible = useThemePickerStore((s) => s.visible)
  if (!visible) return null
  return <ThemePickerModal />
}

// ── ThemePickerModal ──────────────────────────────────────────────────────────

function ThemePickerModal() {
  const close          = useThemePickerStore((s) => s.close)
  const activeTab      = useThemePickerStore((s) => s.activeTab)
  const setActiveTab   = useThemePickerStore((s) => s.setActiveTab)
  const builderDualMode = useThemePickerStore((s) => s.builderDualMode)
  const builderShowPreview = useThemePickerStore((s) => s.builderShowPreview)
  const kernelMode     = useThemeStore((s) => s.kernelMode)

  const applyMode = (mode: ThemeMode) => {
    void useThemeStore.getState().setMode(getPickerApi(), mode)
  }

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) close()
  }

  const TAB_STYLE = (active: boolean): React.CSSProperties => ({
    background: 'transparent',
    border: 0,
    borderBottom: active
      ? '2px solid var(--interactive-accent)'
      : '2px solid transparent',
    color: active ? 'var(--text-normal)' : 'var(--text-muted)',
    fontFamily: 'var(--font-interface)',
    fontSize: 13,
    fontWeight: active ? 600 : 400,
    padding: '8px 14px 6px',
    cursor: 'pointer',
    transition: 'color 100ms, border-color 100ms',
  })

  return (
    <div
      onClick={onBackdropClick}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'var(--modal-background)',
        pointerEvents: 'auto',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        paddingTop: 80,
        zIndex: 'var(--layer-modal)',
      }}
    >
      <div
        style={{
          width: builderModalWidth(activeTab, builderDualMode, builderShowPreview),
          maxWidth: '96vw',
          maxHeight: 'calc(100vh - 160px)',
          background: 'var(--background-secondary)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 'var(--radius-l)',
          boxShadow: 'var(--shadow-l)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
        role="dialog"
        aria-label="Appearance"
        aria-modal="true"
      >
        {/* ── Header: tabs + mode toggle + close ── */}
        <div
          style={{
            display: 'flex',
            alignItems: 'stretch',
            borderBottom: '1px solid var(--background-modifier-border)',
            flexShrink: 0,
            gap: 0,
          }}
        >
          {/* Tab buttons */}
          <div style={{ display: 'flex', flex: 1 }}>
            {(['themes', 'snippets', 'build'] as PickerTab[]).map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                style={TAB_STYLE(activeTab === tab)}
              >
                {tab === 'themes' ? 'Themes' : tab === 'snippets' ? 'Snippets' : 'Build'}
              </button>
            ))}
          </div>

          {/* Mode toggle */}
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              padding: '0 12px',
              gap: 0,
              borderLeft: '1px solid var(--background-modifier-border)',
            }}
          >
            <div
              style={{
                display: 'flex',
                background: 'var(--background-primary)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 'var(--radius-s)',
                overflow: 'hidden',
              }}
            >
              {MODE_OPTIONS.map(({ value, label }) => {
                const isActive = kernelMode === value
                return (
                  <button
                    key={value}
                    onClick={() => applyMode(value)}
                    style={{
                      background: isActive ? 'var(--interactive-accent)' : 'transparent',
                      color: isActive ? 'var(--text-on-accent)' : 'var(--text-muted)',
                      border: 0,
                      padding: '4px 10px',
                      fontFamily: 'var(--font-interface)',
                      fontSize: 12,
                      fontWeight: isActive ? 600 : 400,
                      cursor: 'pointer',
                      transition: 'background 100ms',
                    }}
                  >
                    {label}
                  </button>
                )
              })}
            </div>
          </div>

          {/* Close */}
          <button
            onClick={close}
            aria-label="Close"
            style={{
              background: 'transparent',
              border: 0,
              borderLeft: '1px solid var(--background-modifier-border)',
              color: 'var(--text-muted)',
              cursor: 'pointer',
              fontSize: 14,
              padding: '0 16px',
              display: 'flex',
              alignItems: 'center',
            }}
          >
            ✕
          </button>
        </div>

        {/* ── Body ── */}
        {activeTab === 'themes' ? (
          <ThemesTab />
        ) : activeTab === 'snippets' ? (
          <SnippetsPanel />
        ) : (
          <ThemeBuilderPanel />
        )}
      </div>
    </div>
  )
}

// ── ThemesTab ─────────────────────────────────────────────────────────────────

function ThemesTab() {
  const query            = useThemePickerStore((s) => s.query)
  const setQuery         = useThemePickerStore((s) => s.setQuery)
  const categoryFilter   = useThemePickerStore((s) => s.categoryFilter)
  const setCategoryFilter = useThemePickerStore((s) => s.setCategoryFilter)
  const swatchCache      = useThemePickerStore((s) => s.swatchCache)
  const setSwatchCache   = useThemePickerStore((s) => s.setSwatchCache)
  const loadingSwatches  = useThemePickerStore((s) => s.loadingSwatches)
  const setLoadingSwatches = useThemePickerStore((s) => s.setLoadingSwatches)

  const availableThemes = useThemeStore((s) => s.availableThemes)
  const activeThemeId   = useThemeStore((s) => s.activeThemeId)

  const [focusedIndex, setFocusedIndex] = useState(0)
  const searchRef = useRef<HTMLInputElement | null>(null)
  const gridRef   = useRef<HTMLDivElement | null>(null)

  const allCards: ThemeCard[] = useMemo(
    () =>
      availableThemes.map((t) => ({
        id: t.id,
        name: typeof t.name === 'string' ? t.name : t.id,
        author: typeof t.author === 'string' ? t.author : '',
        description: typeof t.description === 'string' ? t.description : '',
        category:
          typeof t.category === 'string' ? (t.category as CategoryFilter) : 'custom',
        builtin: typeof t.builtin === 'boolean' ? t.builtin : false,
        keywords: Array.isArray(t.keywords) ? (t.keywords as string[]) : [],
      })),
    [availableThemes],
  )

  const usedCategories = useMemo<CategoryFilter[]>(() => {
    const cats = new Set(allCards.map((c) => c.category))
    return (['light', 'dark', 'sepia', 'high-contrast', 'custom'] as const).filter(
      (c) => cats.has(c),
    )
  }, [allCards])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return allCards.filter((card) => {
      if (categoryFilter !== 'all' && card.category !== categoryFilter) return false
      if (!q) return true
      return (
        card.name.toLowerCase().includes(q) ||
        card.author.toLowerCase().includes(q) ||
        card.keywords.some((k) => k.toLowerCase().includes(q))
      )
    })
  }, [allCards, query, categoryFilter])

  useEffect(() => {
    setFocusedIndex((prev) => Math.max(0, Math.min(prev, filtered.length - 1)))
  }, [filtered.length])

  useEffect(() => {
    const id = requestAnimationFrame(() => searchRef.current?.focus())
    return () => cancelAnimationFrame(id)
  }, [])

  // Batch-fetch swatches once per session.
  useEffect(() => {
    if (Object.keys(swatchCache).length > 0 || allCards.length === 0) return
    let cancelled = false
    void (async () => {
      setLoadingSwatches(true)
      try {
        const api = getPickerApi()
        const entries = await Promise.all(
          allCards.map(async (card) => {
            try {
              const vars = await api.kernel.invoke<Record<string, string>>(
                THEME_PLUGIN_ID,
                'compute_variables',
                { theme_id: card.id, enabled_snippets: [] },
              )
              const subset: Record<string, string> = {}
              for (const key of SWATCH_KEYS) {
                if (vars[key]) subset[key] = vars[key]
              }
              return [card.id, subset] as const
            } catch {
              return [card.id, {}] as const
            }
          }),
        )
        if (!cancelled) setSwatchCache(Object.fromEntries(entries))
      } finally {
        if (!cancelled) setLoadingSwatches(false)
      }
    })()
    return () => { cancelled = true }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allCards.length])

  useEffect(() => {
    gridRef.current
      ?.querySelector<HTMLDivElement>(`[data-card-idx="${focusedIndex}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  }, [focusedIndex])

  const applyTheme = (themeId: string) =>
    void useThemeStore.getState().setActiveTheme(getPickerApi(), themeId)

  const close = useThemePickerStore((s) => s.close)

  const onSearchKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (filtered.length > 0) {
        setFocusedIndex((i) => Math.min(i + 1, filtered.length - 1))
        gridRef.current?.querySelector<HTMLElement>('[data-card-idx]')?.focus()
      }
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const card = filtered[focusedIndex]
      if (card) applyTheme(card.id)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      close()
    }
  }

  const onGridKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault()
      setFocusedIndex((i) => Math.min(i + 1, filtered.length - 1))
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault()
      setFocusedIndex((i) => Math.max(i - 1, 0))
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      const card = filtered[focusedIndex]
      if (card) applyTheme(card.id)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      close()
    }
  }

  return (
    <>
      {/* Search + filters */}
      <div
        style={{
          padding: '10px 16px 8px',
          borderBottom: '1px solid var(--background-modifier-border)',
          flexShrink: 0,
        }}
      >
        <input
          ref={searchRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onSearchKeyDown}
          placeholder="Search themes…"
          spellCheck={false}
          autoComplete="off"
          style={{
            width: '100%',
            background: 'var(--background-primary)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            color: 'var(--text-normal)',
            fontFamily: 'var(--font-interface)',
            fontSize: 13,
            padding: '7px 10px',
            outline: 0,
            boxSizing: 'border-box',
            marginBottom: 8,
          }}
        />
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          {(['all', ...usedCategories] as CategoryFilter[]).map((cat) => {
            const isActive = categoryFilter === cat
            return (
              <button
                key={cat}
                onClick={() => setCategoryFilter(cat)}
                style={{
                  background: isActive ? 'var(--interactive-accent)' : 'var(--background-primary)',
                  color: isActive ? 'var(--text-on-accent)' : 'var(--text-muted)',
                  border: isActive
                    ? '1px solid transparent'
                    : '1px solid var(--background-modifier-border)',
                  borderRadius: 'var(--radius-full, 9999px)',
                  padding: '3px 10px',
                  fontFamily: 'var(--font-interface)',
                  fontSize: 12,
                  cursor: 'pointer',
                  transition: 'background 100ms',
                }}
              >
                {CATEGORY_LABELS[cat]}
              </button>
            )
          })}
        </div>
      </div>

      {/* Theme grid */}
      <div
        ref={gridRef}
        onKeyDown={onGridKeyDown}
        tabIndex={-1}
        style={{
          overflowY: 'auto',
          padding: 12,
          display: 'grid',
          gridTemplateColumns: 'repeat(2, 1fr)',
          gap: 8,
          alignContent: 'start',
        }}
      >
        {filtered.length === 0 ? (
          <div
            style={{
              gridColumn: '1 / -1',
              padding: '24px 0',
              textAlign: 'center',
              fontFamily: 'var(--font-interface)',
              fontSize: 13,
              color: 'var(--text-faint)',
            }}
          >
            No themes match your search.
          </div>
        ) : (
          filtered.map((card, idx) => (
            <ThemeCard
              key={card.id}
              index={idx}
              card={card}
              isActive={card.id === activeThemeId}
              isFocused={idx === focusedIndex}
              swatches={swatchCache[card.id]}
              loadingSwatches={loadingSwatches}
              onFocus={() => setFocusedIndex(idx)}
              onApply={() => applyTheme(card.id)}
            />
          ))
        )}
      </div>

      {/* Footer */}
      {activeThemeId && (
        <div
          style={{
            borderTop: '1px solid var(--background-modifier-border)',
            padding: '8px 16px',
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            flexShrink: 0,
          }}
        >
          <span
            style={{
              width: 6, height: 6, borderRadius: '50%',
              background: 'var(--interactive-accent)',
              display: 'inline-block', flexShrink: 0,
            }}
          />
          <span style={{ fontFamily: 'var(--font-interface)', fontSize: 12, color: 'var(--text-muted)' }}>
            Active:{' '}
            <strong style={{ color: 'var(--text-normal)', fontWeight: 500 }}>
              {allCards.find((c) => c.id === activeThemeId)?.name ?? activeThemeId}
            </strong>
          </span>
        </div>
      )}
    </>
  )
}

// ── SnippetsPanel ─────────────────────────────────────────────────────────────

function SnippetsPanel() {
  const availableSnippets = useThemeStore((s) => s.availableSnippets)
  // Cascade-ordered IDs (first = applied first = lowest priority).
  const enabledSnippets = useThemeStore((s) => s.enabledSnippets)

  // Build the enabled list in cascade order, then show highest priority at top
  // so the user can intuit "top = wins over rows below".
  const enabledOrdered: AvailableSnippet[] = useMemo(() => {
    const byId = new Map(availableSnippets.map((s) => [s.id, s]))
    return [...enabledSnippets]
      .map((id) => byId.get(id))
      .filter((s): s is AvailableSnippet => s !== undefined)
      .reverse() // reverse: display highest-priority (last-applied) at top
  }, [availableSnippets, enabledSnippets])

  const disabledSnippets = useMemo(
    () => availableSnippets.filter((s) => !s.enabled),
    [availableSnippets],
  )

  const toggle = (id: string) =>
    void useThemeStore.getState().toggleSnippet(getPickerApi(), id)

  // Move a snippet within the enabled set. `displayIndex` is the index in
  // `enabledOrdered` (reversed cascade). Moving "up" in display = higher
  // priority = moving toward end of cascade array.
  const move = (displayIndex: number, direction: 'up' | 'down') => {
    // Convert display order back to cascade order for the kernel call.
    const cascadeIds = [...enabledOrdered].reverse().map((s) => s.id)
    // In cascade order, the display's "up" = moving right (toward end).
    const cascadeIndex = cascadeIds.length - 1 - displayIndex
    const targetCascadeIndex =
      direction === 'up' ? cascadeIndex + 1 : cascadeIndex - 1

    if (targetCascadeIndex < 0 || targetCascadeIndex >= cascadeIds.length) return
    const next = [...cascadeIds]
    ;[next[cascadeIndex], next[targetCascadeIndex]] = [
      next[targetCascadeIndex],
      next[cascadeIndex],
    ]
    void useThemeStore.getState().setSnippetOrder(getPickerApi(), next)
  }

  if (availableSnippets.length === 0) {
    return (
      <div
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 8,
          padding: 32,
        }}
      >
        <span style={{ fontSize: 28 }}>🎨</span>
        <span
          style={{
            fontFamily: 'var(--font-interface)',
            fontSize: 14,
            fontWeight: 600,
            color: 'var(--text-normal)',
          }}
        >
          No snippets installed
        </span>
        <span
          style={{
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
            color: 'var(--text-muted)',
            textAlign: 'center',
            maxWidth: 340,
            lineHeight: 1.6,
          }}
        >
          Drop <code style={{ fontFamily: 'var(--font-monospace)' }}>.css</code> files
          into your forge's{' '}
          <code style={{ fontFamily: 'var(--font-monospace)' }}>.forge/snippets/</code>{' '}
          directory, or the global{' '}
          <code style={{ fontFamily: 'var(--font-monospace)' }}>~/.nexus/snippets/</code>.
          The engine hot-reloads them automatically.
        </span>
      </div>
    )
  }

  return (
    <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
      {/* Active snippets */}
      {enabledOrdered.length > 0 && (
        <section>
          <SectionHeader
            label="Active"
            count={enabledOrdered.length}
            hint="Later entries in the cascade override earlier ones."
          />
          {enabledOrdered.map((snippet, idx) => (
            <SnippetRow
              key={snippet.id}
              snippet={snippet}
              enabled
              canMoveUp={idx < enabledOrdered.length - 1}
              canMoveDown={idx > 0}
              onToggle={() => toggle(snippet.id)}
              onMoveUp={() => move(idx, 'up')}
              onMoveDown={() => move(idx, 'down')}
            />
          ))}
        </section>
      )}

      {/* Disabled snippets */}
      {disabledSnippets.length > 0 && (
        <section>
          <SectionHeader label="Available" count={disabledSnippets.length} />
          {disabledSnippets.map((snippet) => (
            <SnippetRow
              key={snippet.id}
              snippet={snippet}
              enabled={false}
              canMoveUp={false}
              canMoveDown={false}
              onToggle={() => toggle(snippet.id)}
              onMoveUp={() => {}}
              onMoveDown={() => {}}
            />
          ))}
        </section>
      )}
    </div>
  )
}

// ── SectionHeader ─────────────────────────────────────────────────────────────

function SectionHeader({
  label,
  count,
  hint,
}: {
  label: string
  count: number
  hint?: string
}) {
  return (
    <div
      style={{
        padding: '8px 16px 4px',
        display: 'flex',
        alignItems: 'baseline',
        gap: 8,
        position: 'sticky',
        top: 0,
        background: 'var(--background-secondary)',
        zIndex: 1,
      }}
    >
      <span
        style={{
          fontFamily: 'var(--font-interface)',
          fontSize: 11,
          fontWeight: 600,
          color: 'var(--text-muted)',
          textTransform: 'uppercase',
          letterSpacing: '0.06em',
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: 'var(--font-interface)',
          fontSize: 11,
          color: 'var(--text-faint)',
          background: 'var(--background-modifier-border)',
          borderRadius: 'var(--radius-full, 9999px)',
          padding: '1px 6px',
        }}
      >
        {count}
      </span>
      {hint && (
        <span
          style={{
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            color: 'var(--text-faint)',
            marginLeft: 'auto',
            fontStyle: 'italic',
          }}
        >
          {hint}
        </span>
      )}
    </div>
  )
}

// ── SnippetRow ────────────────────────────────────────────────────────────────

interface SnippetRowProps {
  snippet: AvailableSnippet
  enabled: boolean
  canMoveUp: boolean
  canMoveDown: boolean
  onToggle(): void
  onMoveUp(): void
  onMoveDown(): void
}

function SnippetRow({
  snippet,
  enabled,
  canMoveUp,
  canMoveDown,
  onToggle,
  onMoveUp,
  onMoveDown,
}: SnippetRowProps) {
  const kernelMode = useThemeStore((s) => s.kernelMode)
  const [hovered, setHovered] = useState(false)

  // Extract mode and scope from the opaque extra-field bag.
  const snippetMode = typeof snippet.mode === 'string' ? snippet.mode : 'all'
  const snippetScope = typeof snippet.scope === 'string' ? snippet.scope : null
  const isScoped = snippetScope !== null && snippetScope !== 'global'

  // Mismatch: snippet targets one mode but a different concrete mode is active.
  const isMismatch =
    enabled &&
    kernelMode !== 'system' &&
    snippetMode !== 'all' &&
    snippetMode !== kernelMode

  const hasBadges = snippetMode === 'light' || snippetMode === 'dark' || isScoped

  const ICON_BTN: React.CSSProperties = {
    background: 'transparent',
    border: '1px solid var(--background-modifier-border)',
    borderRadius: 'var(--radius-s)',
    color: 'var(--text-muted)',
    cursor: 'pointer',
    fontSize: 11,
    lineHeight: 1,
    padding: '2px 5px',
    display: 'flex',
    alignItems: 'center',
  }

  const PILL: React.CSSProperties = {
    fontFamily: 'var(--font-interface)',
    fontSize: 10,
    borderRadius: 'var(--radius-full, 9999px)',
    padding: '1px 6px',
    flexShrink: 0,
    whiteSpace: 'nowrap',
  }

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 10,
        padding: '8px 16px',
        background: hovered ? 'var(--background-modifier-hover)' : 'transparent',
        transition: 'background 80ms',
      }}
    >
      {/* Toggle — dimmed when snippet won't apply in the current mode */}
      <button
        onClick={onToggle}
        aria-pressed={enabled}
        aria-label={enabled ? `Disable ${snippet.name}` : `Enable ${snippet.name}`}
        title={isMismatch ? `Only applies in ${snippetMode} mode — no effect in ${kernelMode} mode` : undefined}
        style={{
          flexShrink: 0,
          width: 32,
          height: 18,
          borderRadius: 9,
          border: 0,
          background: enabled ? 'var(--interactive-accent)' : 'var(--background-modifier-border)',
          cursor: 'pointer',
          position: 'relative',
          transition: 'background 150ms, opacity 150ms',
          opacity: isMismatch ? 0.45 : 1,
        }}
      >
        <span
          style={{
            position: 'absolute',
            top: 2,
            left: enabled ? 16 : 2,
            width: 14,
            height: 14,
            borderRadius: '50%',
            background: 'white',
            transition: 'left 150ms',
            boxShadow: '0 1px 2px rgba(0,0,0,0.3)',
          }}
        />
      </button>

      {/* Name + description + badges */}
      <div style={{ flex: 1, minWidth: 0 }}>
        {/* Name row */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 0 }}>
          <span
            style={{
              fontFamily: 'var(--font-interface)',
              fontSize: 13,
              fontWeight: enabled ? 600 : 400,
              color: enabled ? 'var(--text-normal)' : 'var(--text-muted)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              flex: 1,
              minWidth: 0,
            }}
          >
            {snippet.name}
          </span>
          {isMismatch && (
            <span
              title={`Only applies in ${snippetMode} mode — no effect while ${kernelMode} mode is active`}
              aria-label="Mode mismatch warning"
              style={{ fontSize: 12, flexShrink: 0, cursor: 'default' }}
            >
              ⚠
            </span>
          )}
        </div>

        {/* Description */}
        {snippet.description && (
          <div
            style={{
              fontFamily: 'var(--font-interface)',
              fontSize: 11,
              color: 'var(--text-faint)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {snippet.description}
          </div>
        )}

        {/* Mode / scope badges */}
        {hasBadges && (
          <div style={{ display: 'flex', gap: 4, marginTop: 3, flexWrap: 'wrap' }}>
            {snippetMode === 'light' && (
              <span
                style={{
                  ...PILL,
                  background: 'rgba(243, 156, 18, 0.12)',
                  color: 'var(--color-yellow, #C8915A)',
                  border: '1px solid rgba(243, 156, 18, 0.25)',
                }}
              >
                Light only
              </span>
            )}
            {snippetMode === 'dark' && (
              <span
                style={{
                  ...PILL,
                  background: 'rgba(74, 144, 226, 0.12)',
                  color: 'var(--interactive-accent, #6BA3FF)',
                  border: '1px solid rgba(74, 144, 226, 0.25)',
                }}
              >
                Dark only
              </span>
            )}
            {isScoped && (
              <span
                title={`Scoped to: ${snippetScope}`}
                style={{
                  ...PILL,
                  background: 'var(--background-modifier-border)',
                  color: 'var(--text-muted)',
                  cursor: 'default',
                }}
              >
                Scoped
              </span>
            )}
          </div>
        )}
      </div>

      {/* Reorder arrows — only visible when enabled and hovered */}
      {enabled && (
        <div
          style={{
            display: 'flex',
            gap: 4,
            opacity: hovered ? 1 : 0,
            transition: 'opacity 100ms',
            flexShrink: 0,
          }}
        >
          <button
            onClick={onMoveUp}
            disabled={!canMoveUp}
            aria-label="Increase priority"
            title="Increase priority (apply later)"
            style={{
              ...ICON_BTN,
              opacity: canMoveUp ? 1 : 0.3,
              cursor: canMoveUp ? 'pointer' : 'default',
            }}
          >
            ↑
          </button>
          <button
            onClick={onMoveDown}
            disabled={!canMoveDown}
            aria-label="Decrease priority"
            title="Decrease priority (apply earlier)"
            style={{
              ...ICON_BTN,
              opacity: canMoveDown ? 1 : 0.3,
              cursor: canMoveDown ? 'pointer' : 'default',
            }}
          >
            ↓
          </button>
        </div>
      )}
    </div>
  )
}

// ── ThemeCard ─────────────────────────────────────────────────────────────────

interface ThemeCardProps {
  index: number
  card: ThemeCard
  isActive: boolean
  isFocused: boolean
  swatches: Record<string, string> | undefined
  loadingSwatches: boolean
  onFocus(): void
  onApply(): void
}

function ThemeCard({
  index,
  card,
  isActive,
  isFocused,
  swatches,
  loadingSwatches,
  onFocus,
  onApply,
}: ThemeCardProps) {
  return (
    <div
      data-card-idx={index}
      tabIndex={0}
      role="option"
      aria-selected={isActive}
      aria-label={`${card.name}${isActive ? ', currently active' : ''}`}
      onClick={onApply}
      onMouseEnter={onFocus}
      onFocus={onFocus}
      style={{
        background: isActive
          ? 'var(--background-modifier-active-hover, var(--interactive-accent-soft))'
          : isFocused
            ? 'var(--background-modifier-hover)'
            : 'var(--background-primary)',
        border: isActive
          ? '1.5px solid var(--interactive-accent)'
          : isFocused
            ? '1.5px solid var(--background-modifier-border-hover, var(--background-modifier-border))'
            : '1.5px solid var(--background-modifier-border)',
        borderRadius: 'var(--radius-m)',
        cursor: 'pointer',
        overflow: 'hidden',
        outline: 0,
        display: 'flex',
        flexDirection: 'column',
        transition: 'border-color 100ms',
      }}
    >
      <SwatchStrip swatches={swatches} loading={loadingSwatches} />
      <div style={{ padding: '8px 10px', display: 'flex', flexDirection: 'column', gap: 2 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span
            style={{
              fontFamily: 'var(--font-interface)',
              fontSize: 13,
              fontWeight: 600,
              color: 'var(--text-normal)',
              flex: 1,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {card.name}
          </span>
          {isActive && (
            <span
              aria-hidden="true"
              style={{ fontSize: 11, color: 'var(--interactive-accent)', fontWeight: 700, flexShrink: 0 }}
            >
              ✓
            </span>
          )}
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          {card.author && (
            <span
              style={{
                fontFamily: 'var(--font-interface)',
                fontSize: 11,
                color: 'var(--text-faint)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                flex: 1,
              }}
            >
              {card.author}
            </span>
          )}
          {card.builtin && (
            <span
              style={{
                fontFamily: 'var(--font-interface)',
                fontSize: 10,
                color: 'var(--text-muted)',
                background: 'var(--background-modifier-border)',
                borderRadius: 'var(--radius-s)',
                padding: '1px 5px',
                flexShrink: 0,
              }}
            >
              Built-in
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

// ── SwatchStrip ───────────────────────────────────────────────────────────────

function SwatchStrip({
  swatches,
  loading,
}: {
  swatches: Record<string, string> | undefined
  loading: boolean
}) {
  if (loading && !swatches) {
    return (
      <div
        style={{
          height: 30,
          background: 'var(--background-modifier-border)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <span style={{ fontFamily: 'var(--font-interface)', fontSize: 10, color: 'var(--text-faint)' }}>
          …
        </span>
      </div>
    )
  }

  if (!swatches || Object.keys(swatches).length === 0) {
    return <div style={{ height: 30, background: 'var(--background-modifier-border)' }} />
  }

  return (
    <div style={{ display: 'flex', height: 30 }}>
      {SWATCH_KEYS.map((key) => (
        <div
          key={key}
          title={key}
          style={{ flex: 1, background: swatches[key] ?? 'var(--background-modifier-border)' }}
        />
      ))}
    </div>
  )
}
