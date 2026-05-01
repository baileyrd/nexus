/* src/primitives/NavHeader.tsx
   Reusable React primitives that mirror Obsidian's per-view toolbar
   pattern: <NavHeader> wraps a sidebar-view's top strip; inside it
   <NavButtonsContainer> lays out a row of <NavActionButton>s.

   Source: Obsidian app.css
     - .nav-header                     → padding: var(--size-4-2)
     - .nav-buttons-container          → flex-wrap + gap: var(--size-2-1)
     - .nav-buttons-container.has-separator → border-bottom + padding-bottom
     - .nav-action-button              → 24×24, radius-s, muted icon color
     - .nav-action-button:hover        → bg-modifier-hover, text-normal
     - .nav-action-button.is-active    → same as hover (Obsidian uses a
       dedicated --icon-color-focused token; we reuse --text-normal since
       Nexus does not define the former).

   All tokens referenced here are the Obsidian names that Task A
   wired into shell/index.html. Do NOT reference --bg-* / --fg-*
   legacy aliases from this file. */

import { forwardRef, useState } from 'react'
import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from 'react'

export function NavHeader({
  children,
  style,
  ...rest
}: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      {...rest}
      style={{
        flexShrink: 0,
        display: 'flex',
        flexDirection: 'column',
        padding: 'var(--size-4-2)',
        ...style,
      }}
    >
      {children}
    </div>
  )
}

export function NavButtonsContainer({
  hasSeparator,
  children,
  style,
  ...rest
}: { hasSeparator?: boolean } & HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      {...rest}
      style={{
        display: 'flex',
        flexDirection: 'row',
        flexWrap: 'wrap',
        gap: 'var(--size-2-1)',
        ...(hasSeparator
          ? {
              borderBottom: '1px solid var(--divider-color, var(--divider-color))',
              paddingBottom: 'var(--size-2-3)',
              marginBottom: 'var(--size-4-2)',
            }
          : null),
        ...style,
      }}
    >
      {children}
    </div>
  )
}

type NavActionButtonProps = {
  active?: boolean
  label: string
  icon: ReactNode
  onClick: () => void
} & Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children' | 'onClick'>

export const NavActionButton = forwardRef<HTMLButtonElement, NavActionButtonProps>(
  function NavActionButton(
    { active, label, icon, onClick, style, ...rest },
    ref,
  ) {
    const [hover, setHover] = useState(false)
    const emphasized = active || hover
    return (
      <button
        {...rest}
        ref={ref}
        type={rest.type ?? 'button'}
        aria-label={label}
        aria-pressed={active}
        title={label}
        onClick={onClick}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        style={{
          width: 24,
          height: 24,
          padding: 0,
          border: 0,
          background: emphasized
            ? 'var(--background-modifier-hover)'
            : 'transparent',
          color: emphasized ? 'var(--text-normal)' : 'var(--text-muted)',
          cursor: 'pointer',
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          borderRadius: 'var(--radius-s)',
          flexShrink: 0,
          transition: 'background 0.08s, color 0.08s',
          ...style,
        }}
      >
        {icon}
      </button>
    )
  },
)
