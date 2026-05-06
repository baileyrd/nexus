import { useEffect, useMemo, useRef, useState } from 'react'
import { useThemeStore, type ThemeMode } from '../../../stores/themeStore'
import { THEME_PLUGIN_ID } from '../../../stores/themeStore'
import {
  useThemePickerStore,
  SWATCH_KEYS,
  type CategoryFilter,
  type ThemeCard,
} from './themePickerStore'
import { getPickerApi } from './pickerRuntime'

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

/**
 * Mounts nothing when closed. The slot: 'overlay' host already sets
 * pointer-events: none on the container; we flip it back on our backdrop.
 */
export function ThemePicker() {
  const visible = useThemePickerStore((s) => s.visible)
  if (!visible) return null
  return <ThemePickerModal />
}

// ── ThemePickerModal ──────────────────────────────────────────────────────────

function ThemePickerModal() {
  const close = useThemePickerStore((s) => s.close)
  const query = useThemePickerStore((s) => s.query)
  const setQuery = useThemePickerStore((s) => s.setQuery)
  const categoryFilter = useThemePickerStore((s) => s.categoryFilter)
  const setCategoryFilter = useThemePickerStore((s) => s.setCategoryFilter)
  const swatchCache = useThemePickerStore((s) => s.swatchCache)
  const setSwatchCache = useThemePickerStore((s) => s.setSwatchCache)
  const loadingSwatches = useThemePickerStore((s) => s.loadingSwatches)
  const setLoadingSwatches = useThemePickerStore((s) => s.setLoadingSwatches)

  const availableThemes = useThemeStore((s) => s.availableThemes)
  const activeThemeId = useThemeStore((s) => s.activeThemeId)
  const kernelMode = useThemeStore((s) => s.kernelMode)

  const [focusedIndex, setFocusedIndex] = useState(0)

  const searchRef = useRef<HTMLInputElement | null>(null)
  const gridRef = useRef<HTMLDivElement | null>(null)

  // Coerce the opaque ThemeManifestEntry list into ThemeCard for typed access.
  const allCards: ThemeCard[] = useMemo(
    () =>
      availableThemes.map((t) => ({
        id: t.id,
        name: typeof t.name === 'string' ? t.name : t.id,
        author: typeof t.author === 'string' ? t.author : '',
        description: typeof t.description === 'string' ? t.description : '',
        category:
          typeof t.category === 'string'
            ? (t.category as CategoryFilter)
            : 'custom',
        builtin: typeof t.builtin === 'boolean' ? t.builtin : false,
        keywords: Array.isArray(t.keywords) ? (t.keywords as string[]) : [],
      })),
    [availableThemes],
  )

  // Derive the set of categories that have at least one theme.
  const usedCategories = useMemo<CategoryFilter[]>(() => {
    const cats = new Set(allCards.map((c) => c.category))
    return (['light', 'dark', 'sepia', 'high-contrast', 'custom'] as const).filter(
      (c) => cats.has(c),
    )
  }, [allCards])

  // Filter by query and category.
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

  // Clamp focusedIndex when the filtered list shrinks.
  useEffect(() => {
    setFocusedIndex((prev) => Math.max(0, Math.min(prev, filtered.length - 1)))
  }, [filtered.length])

  // Focus the search input on open.
  useEffect(() => {
    const id = requestAnimationFrame(() => searchRef.current?.focus())
    return () => cancelAnimationFrame(id)
  }, [])

  // Batch-fetch swatches for all themes once per session (cache persists as
  // long as the picker store is alive, cleared on next app boot).
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
              // Only keep the swatch keys — reduces memory.
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
        if (!cancelled) {
          setSwatchCache(Object.fromEntries(entries))
        }
      } finally {
        if (!cancelled) setLoadingSwatches(false)
      }
    })()
    return () => { cancelled = true }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allCards.length])

  // Scroll focused card into view.
  useEffect(() => {
    const el = gridRef.current?.querySelector<HTMLDivElement>(
      `[data-card-idx="${focusedIndex}"]`,
    )
    el?.scrollIntoView({ block: 'nearest' })
  }, [focusedIndex])

  const applyTheme = (themeId: string) => {
    void useThemeStore.getState().setActiveTheme(getPickerApi(), themeId)
  }

  const applyMode = (mode: ThemeMode) => {
    void useThemeStore.getState().setMode(getPickerApi(), mode)
  }

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

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) close()
  }

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
      {/* Modal card */}
      <div
        style={{
          width: 660,
          maxWidth: '94vw',
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
        aria-label="Theme Picker"
        aria-modal="true"
      >
        {/* ── Header ── */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            padding: '16px 20px 12px',
            borderBottom: '1px solid var(--background-modifier-border)',
            gap: 12,
            flexShrink: 0,
          }}
        >
          <span
            style={{
              flex: 1,
              fontFamily: 'var(--font-interface)',
              fontSize: 15,
              fontWeight: 600,
              color: 'var(--text-normal)',
            }}
          >
            Themes
          </span>

          {/* Mode toggle */}
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
              const active = kernelMode === value
              return (
                <button
                  key={value}
                  onClick={() => applyMode(value)}
                  style={{
                    background: active ? 'var(--interactive-accent)' : 'transparent',
                    color: active ? 'var(--text-on-accent)' : 'var(--text-muted)',
                    border: 0,
                    padding: '4px 12px',
                    fontFamily: 'var(--font-interface)',
                    fontSize: 12,
                    fontWeight: active ? 600 : 400,
                    cursor: 'pointer',
                    transition: 'background 100ms',
                  }}
                >
                  {label}
                </button>
              )
            })}
          </div>

          {/* Close button */}
          <button
            onClick={close}
            aria-label="Close theme picker"
            style={{
              background: 'transparent',
              border: 0,
              color: 'var(--text-muted)',
              cursor: 'pointer',
              fontSize: 16,
              lineHeight: 1,
              padding: 4,
              borderRadius: 'var(--radius-s)',
              display: 'flex',
              alignItems: 'center',
            }}
          >
            ✕
          </button>
        </div>

        {/* ── Search + filters ── */}
        <div
          style={{
            padding: '10px 16px 8px',
            borderBottom: '1px solid var(--background-modifier-border)',
            flexShrink: 0,
          }}
        >
          {/* Search input */}
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

          {/* Category filter pills */}
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            {(['all', ...usedCategories] as CategoryFilter[]).map((cat) => {
              const active = categoryFilter === cat
              return (
                <button
                  key={cat}
                  onClick={() => setCategoryFilter(cat)}
                  style={{
                    background: active
                      ? 'var(--interactive-accent)'
                      : 'var(--background-primary)',
                    color: active
                      ? 'var(--text-on-accent)'
                      : 'var(--text-muted)',
                    border: active
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

        {/* ── Theme grid ── */}
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

        {/* ── Footer: active theme name ── */}
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
                width: 6,
                height: 6,
                borderRadius: '50%',
                background: 'var(--interactive-accent)',
                display: 'inline-block',
                flexShrink: 0,
              }}
            />
            <span
              style={{
                fontFamily: 'var(--font-interface)',
                fontSize: 12,
                color: 'var(--text-muted)',
              }}
            >
              Active:{' '}
              <strong style={{ color: 'var(--text-normal)', fontWeight: 500 }}>
                {allCards.find((c) => c.id === activeThemeId)?.name ?? activeThemeId}
              </strong>
            </span>
          </div>
        )}
      </div>
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
      {/* Color swatch strip */}
      <SwatchStrip swatches={swatches} loading={loadingSwatches} />

      {/* Card body */}
      <div
        style={{
          padding: '8px 10px',
          display: 'flex',
          flexDirection: 'column',
          gap: 2,
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
          }}
        >
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
              style={{
                fontSize: 11,
                color: 'var(--interactive-accent)',
                fontWeight: 700,
                flexShrink: 0,
              }}
            >
              ✓
            </span>
          )}
        </div>

        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
          }}
        >
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
        <span
          style={{
            fontFamily: 'var(--font-interface)',
            fontSize: 10,
            color: 'var(--text-faint)',
          }}
        >
          …
        </span>
      </div>
    )
  }

  if (!swatches || Object.keys(swatches).length === 0) {
    return (
      <div
        style={{
          height: 30,
          background: 'var(--background-modifier-border)',
        }}
      />
    )
  }

  return (
    <div style={{ display: 'flex', height: 30 }}>
      {SWATCH_KEYS.map((key) => (
        <div
          key={key}
          title={key}
          style={{
            flex: 1,
            background: swatches[key] ?? 'var(--background-modifier-border)',
          }}
        />
      ))}
    </div>
  )
}
