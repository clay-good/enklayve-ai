# Enklayve — Design System

The design of a local, private "ask your data" desktop app. The job of the UI is
to get out of the way: a calm, dark, two-pane workspace where the content (your
conversations, your documents, the answer) is the only thing that feels bright.

Mode: **Operate** — the user is completing a task (asking, reading, managing
documents). Scanability, consistency, and native desktop expectations outrank
expression. Personality lives in precise details, not decoration.

## Principles

1. **Content is the only bright thing.** Chrome recedes (muted greys, hairline
   borders); the accent and full-contrast text are reserved for what matters —
   the active conversation, a user's message, a live toggle, the answer.
2. **One accent, used sparingly.** `--color-accent-primary` (#4a9eff) marks the
   primary action and the "on" state. Everywhere else is greyscale depth.
3. **Depth by elevation, not lines.** Surfaces step up (`bg-primary` →
   `bg-secondary` → `bg-tertiary` → `bg-elevated`); shadows carry a real offset
   and blur. No border-left accents, no zero-blur block shadows.
4. **Drawn icons, never emoji.** Controls use inline stroke SVGs at one weight
   (2px, round caps), currentColor so they inherit state. Emoji are not an icon
   system.
5. **Theme every surface, including the browser's.** Selection, caret, focus
   rings, and scrollbars are themed from the palette, not left to defaults.

## Tokens

The single source of truth is [`src/styles/design-tokens.css`](src/styles/design-tokens.css).
Always use the variables; never hardcode a color, space, radius, or duration.

- **Surfaces:** `--color-bg-primary/secondary/tertiary/elevated`
- **Text:** `--color-text-primary/secondary/tertiary/disabled`
- **Accent:** `--color-accent-primary/hover/active`
- **Semantic:** `--color-success/warning/danger/info`
- **Borders:** `--color-border-primary/secondary/focus`
- **Spacing:** `--space-1..20` (4px base scale) — tight within a group, generous
  between groups.
- **Radius:** `--radius-sm/md/lg/xl/full`
- **Type:** `--font-size-xs..3xl`, `--font-weight-normal..bold`,
  `--line-height-*`; system UI stack for the interface, mono only for code/data.
- **Motion:** `--transition-fast/base/slow` with `--easing-ease-out` /
  `-in-out`; animate from an already-visible default.

Light mode is `:root.light-mode` overriding the same variables. Any new color
must be added to both themes.

## Layout

- **Shell:** a fixed-width sidebar (`--sidebar-width`, 320px) + a flexible main
  chat column, full height, no page scroll.
- **Sidebar** splits into conversations (top) and documents (bottom), each
  independently scrollable.
- **Settings toolbar** (top of the sidebar): text actions (Export, Import) on the
  left, then icon toggles pushed right — security, **web search**, theme. "On"
  toggles tint with the accent; off is muted. (Web search moved here from the
  Documents panel so all settings live together.)
- **Main column:** scrolling message list above a docked composer. The composer
  textarea auto-grows with the prompt up to a cap, then scrolls.

## Components & states

Every interactive element ships hover, active, disabled, and (where it applies)
on/off and loading states. Icon toggles show state through color + a soft accent
tint, not a separate label. Empty states say what to do next in the product's own
voice. Answers render Markdown; citations are inline links — document citations
open a passage viewer, web citations open the URL.

## Responsiveness

The target is a resizable desktop window. The layout holds from a narrow window
up; the sidebar keeps a sensible minimum and the main column reflows. Long
content (titles, file names, code blocks) truncates or scrolls rather than
breaking the layout.

## Non-negotiables (craft floor)

Themed selection/caret/focus/scrollbars; drawn SVG icons at one weight; shadows
with offset + blur; accent reserved for primary/active; real copy at every width;
no emoji-as-icon, no gradient text, no colored border-left accents.
