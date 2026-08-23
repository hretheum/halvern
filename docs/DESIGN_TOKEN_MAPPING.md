# Token mapping tables

The lookup tables that turn the design-system work from judgement into
execution. Everything before this document was deciding what the names mean;
everything after it is applying them.

Companion to [DESIGN_SYSTEM_PLAN.md](DESIGN_SYSTEM_PLAN.md). Values come from
[../design/tokens/halvern.tokens.json](../design/tokens/halvern.tokens.json).

## 1. Captured raw colour → Figma token

What the HTML captures produced, and what each value becomes. The light and dark
columns are the same interface captured under the two themes, which is why one
token has two source hexes.

| Light hex | Dark hex | Uses | Token | Note |
|---|---|---|---|---|
| `#007176` | `#50afb4` | 129 | `accent/default` | The inherited Meetily teal — 49 fills, 49 frame strokes, 31 icon strokes |
| `#ffffff` (fill) | `#ffffff` | 50 | `bg/overlay` | Rows and cards. White in light, a lift off the ground in dark |
| `#ffffff` (stroke) | `#ffffff` | 49 | `accent/on-accent` | Glyphs sitting on the accent |
| `#dfdeda` | `#363531` | 72 | `border/default` | 54 strokes and 18 one-pixel frames used as rules |
| `#767676` | `#767676` | 49 | `border/strong` | Unchecked checkbox outline |
| `#fafaf9` | `#171614` | 5 | `bg/app` | Page ground |
| `#f1f0ed` | — | 2 | `bg/hover` | |
| `#d9d9d9` | `#d9d9d9` | 2 | `border/default` | |
| `#000000` | `#000000` | 3 | `border/default` | Window outline |
| `#65635c` | `#8b8981` | 2 | `text/secondary` | Icon strokes |
| `#1b1b17` | `#e5e4e2` | 2 | `text/primary` | Icon strokes |
| — | `#1f1e1b` | 1 | `bg/surface` | |
| `#ff736a` `#febc2e` `#19c332` | same | 3 | **stay raw** | macOS traffic lights — OS chrome, not brand, must not follow the theme |

The two rules that are easy to get wrong: the same hex means different things on
a fill and on a stroke, so the table is keyed by both; and white on a stroke is
`accent/on-accent`, not `bg/overlay`.

## 2. Figma token → shadcn variable

The app's stylesheet already speaks shadcn. Renaming those variables would churn
every vendored component for nothing, so shadcn stays as the app's component
tier and aliases the semantic tier. These are the values to write into
`globals.css`; they are HSL triples because that is the format
`hsl(var(--name))` expects.

| Figma token | shadcn variable | light | dark |
|---|---|---|---|
| `bg/app` | `--background` | `43 25.9% 94.7%` | `30 11.1% 7.1%` |
| `bg/overlay` | `--card`, `--popover` | `0 0% 100%` | `30 10.2% 19.2%` |
| `bg/hover` | `--secondary`, `--muted` | `42 14.7% 86.7%` | `30 10.2% 19.2%` |
| `text/primary` | `--foreground`, `--card-foreground`, `--popover-foreground`, `--secondary-foreground` | `40 11.1% 10.6%` | `47 19.1% 90.8%` |
| `text/secondary` | `--muted-foreground` | `35 7% 33.7%` | `40 6.5% 63.5%` |
| `text/tertiary` | `--color-tertiary` | `33 5.4% 39.8%` | `33 4.3% 50%` |
| `interactive/primary` | `--primary` | `29 53% 35.9%` | `29 53.6% 49%` |
| `interactive/primary-text` | `--primary-foreground` | `0 0% 100%` | `30 11.1% 7.1%` |
| `accent/subtle` | `--accent` | `42 14.7% 86.7%` | `30 10.2% 19.2%` |
| `accent/default` | `--accent-foreground` | `29 53% 35.9%` | `29 53.6% 49%` |
| `border/default` | `--border`, `--input` | `40 12.2% 80.8%` | `33 9.9% 21.8%` |
| `border/focus` | `--ring` | `29 53.6% 49%` | `31 60.7% 57.1%` |
| `status/danger/solid` | `--destructive` | `12 41.4% 38.8%` | `12 41.4% 38.8%` |

`--primary` and `--ring` are the branding change in code: both are teal today
(`182 100% 23.2%` light, `183 40% 51%` dark) and both become bronze.

### Three things this mapping cannot do cleanly

**The surface model does not line up.** shadcn has two levels, `background` and
`card`. The semantic tier has four — `bg/app`, `bg/surface`, `bg/elevated`,
`bg/overlay`. Mapping app→background and overlay→card covers the ends and leaves
the middle two with no home. Either add `--surface` and `--elevated` alongside
the shadcn names, or accept that the app renders a flatter hierarchy than the
design does. Adding them is cheap and keeps Figma and code saying the same
thing; that is the recommendation, but it is a decision, not a lookup.

**`--color-tertiary: #64748b` is a live violation** — a cool slate hardcoded
among variable references, in a system that is warm everywhere else. It is the
smallest possible first change and a good canary for the export pipeline.

**`--recording` should probably not become `status/danger/solid`.** It is
`357 68% 47.6%` today, a vivid red; `danger/500` is `#8c4a3a`, a muted
terracotta chosen to sit inside a warm palette. The record indicator is the one
element that should alarm slightly, and muting it to match the brand would make
it calmer exactly where calm is wrong. Leave it vivid, or introduce a
`status/recording` token that is allowed to be louder than the rest.

## 3. Where the handoff line falls

Everything above required deciding what a name means, what a colour is for, and
which of two defensible options to take. That is the part worth a careful model
and a human review.

What remains is applying these two tables to the `workshop` and `recording`
views, then to the prototype's remaining states — loading, empty, error, the 900
and 1600 widths, the settings panes — and generating the CSS from
`halvern.tokens.json` rather than retyping it. That work is voluminous,
repetitive and fully specified by this document, which is exactly the whole-job
handoff shape.

One caution belongs with the handoff, learned the hard way on the library view:
**do not collapse wrapper frames.** A test for "inert" that checks paints,
padding and sizing still misses `layoutMode`, and a wrapper that flips the axis
is doing the most important work on the row. `GRID` parents make it worse, since
children occupy cells in order and removing one shifts every sibling after it.
Structural edits need a dry run, a render diff, and an intact copy kept until the
result is verified.
