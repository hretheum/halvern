# Halvern design system: token pyramid and branding rollout

A plan for turning the captured app views in Figma into a branded, tokenised
design system that can serve both the desktop app and the future website.

Written 17 August 2026 against Figma file `JGGujPmlHo0mVCBIFQblKo` and the
frontend at `f8b2f02`. Every number below was measured in the file, not
estimated.

## 1. Where things actually stand

Better than expected in one place, worse in another.

**The token foundation exists and is well shaped.** Two local collections:

| Collection | Modes | Count | Content |
|---|---|---|---|
| `Primitives` | `Value` | 23 | `ink`, `bronze`, `stone`, `dark/0–4`, `light/0–3`, `text/*`, `bronze/light\|hover\|dark`, `success`, `destructive` |
| `Semantic` | `Light`, `Dark` | 16 | `bg/*`, `border/*`, `text/*`, `accent/*`, `status/*`, `interactive/*` |

The aliasing is correct two-tier work — `bg/app → light/0 \| dark/0`,
`bg/surface → light/1 \| dark/1`, `border/default → light/3 \| dark/4`. Nothing
here needs redoing; it needs widening.

**The views barely use it.** In `app-library--light` (node `8:2`, 1030 nodes)
only **eight distinct variables** are bound across 266 bindings, and the
distribution is lopsided:

- `text/secondary` — 205 bindings. *(Corrected 17 August: I first read this as a
  category error because the bindings spanned fills and strokes. Splitting them
  showed 203 fills on TEXT and 2 strokes on VECTOR — icons stroked in the text
  colour, which is correct usage. Nothing to unpick.)*
- `text/primary` — 55 bindings.
- `bg/surface`, `bg/elevated`, `border/default`, `accent/default`,
  `interactive/primary`, `interactive/primary-text` — **one binding each**.

So surfaces, dividers and borders are almost entirely raw hex. Nine distinct
untokenised fills remain, and the top of that list is the tell:

| Hex | Uses | What it is | Should become |
|---|---|---|---|
| `#ffffff` | 54 | card / row surfaces | `bg/surface` or `bg/overlay` |
| `#007176` | 49 | **inherited Meetily teal** | `accent/default` (Halvern bronze) |
| `#dfdeda` | 18 | dividers, input borders | `border/default` |
| `#fafaf9` | 5 | page ground | `bg/app` |
| `#f1f0ed`, `#d9d9d9` | 4 | hover / subtle fills | `bg/elevated`, `border/subtle` |
| `#ff736a`, `#febc2e`, `#19c332` | 3 | macOS traffic lights | stay raw — OS chrome, not brand |

Forty-nine uses of the old teal is the branding job in one number.

**Typography is untokenised.** Zero text styles and zero paint styles exist in
the file. The body face is right — Source Sans 3, matching `--font-sans` in
`globals.css` — but five nodes are still on Figma's default Inter: `Halvern`,
`Library`, `49 meetings`, `Import`, `Record`. Those are the labels edited by
hand after capture; Figma silently defaults new text to Inter.

**Layout is structurally sound but over-nested.** 520 auto-layout frames against
159 plain ones, max depth 8, and only one absolutely-positioned node, no
fixed-size text, one zero-size node. The real noise is **197 redundant wrappers**
(an auto-layout frame whose only child is another frame) and **158 plain frames
sitting inside auto-layout parents** — classic HTML-capture residue where every
`<div>` became a frame.

**The two theme frames have already diverged**: `app-library--light` has 1030
nodes, `app-library--dark` has 1029. They were captured separately and are now
being edited separately. That divergence only grows.

## 2. The decision that shapes everything else

Once a view is fully bound to `Semantic`, **one frame renders both themes** by
switching the collection mode. The dark twins then stop being artefacts to
maintain and become a preview toggle.

So the target is three frames, not six: `library`, `workshop`, `recording`, each
mode-switchable. The dark captures are deleted once their light twin is bound
and verified to resolve correctly in `Dark`.

This is worth stating plainly because it inverts the obvious approach. The
temptation is to fix twelve frames; the correct move is to fix three and throw
six away.

## 3. The token pyramid

The current two tiers are the middle of a mature system. A pyramid that can
carry an app *and* a marketing site needs a wider base and a scoped top.

### Tier 0 — Core primitives (one mode, no meaning)

What exists is a hand-picked set of nine greys and three bronzes. That is enough
for one app and too thin for a website, which needs hover states, disabled
states, chart series and marketing surfaces.

- **Colour ramps** rather than picks: `stone/50…950` and `bronze/50…950` on a
  perceptual scale, plus `success`, `warning`, `danger`, `info` as ramps, not
  single values. Keep `white` and `black` as fixed anchors.
- **Type**: families (`sans` = Source Sans 3, `mono` for timestamps and code),
  a modular size scale, weights, line-heights, letter-spacing.
- **Space**: a 4-px based scale. Currently every padding and gap in the captures
  is a raw number.
- **Radius**, **elevation** (shadow sets), **motion** (durations, easings) —
  none of these exist today, and the website will need all three.

### Tier 1 — Semantic (themed: `Light`, `Dark`)

Keep the existing sixteen names and the aliasing pattern; widen for the states a
real interface has: `bg/sunken`, `text/disabled`, `text/inverse`, `text/link`,
`border/strong`, `border/focus`, `accent/active`, `accent/on-accent`, and status
tokens split into `bg` / `border` / `text` triples rather than one flat colour.

This tier stays surface-agnostic. It must not learn that a website exists.

### Tier 2 — Component (scoped, aliases tier 1)

Absent today, and it is the tier that lets a redesign happen without touching
every frame: `button/{primary,secondary,ghost}/{bg,text,border}` across
rest/hover/active/disabled, `input/{bg,border,placeholder,focus-ring}`,
`card/{bg,border,shadow}`, `list-row/{bg,hover,selected,divider}`,
`titlebar/{bg,text}`.

This is also where app and website legitimately diverge while sharing tiers 0–1
— the "one core, many surfaces" shape.

### Reconciling Figma with the code

The code already has a semantic layer, in shadcn's vocabulary, consumed through
Tailwind 4's `@theme`:

```css
@theme {
  --font-sans: var(--font-source-sans-3);
  --color-background: hsl(var(--background));
  --color-primary:    hsl(var(--primary));
  --color-tertiary:   #64748b;   /* ← a raw hex among the aliases */
}
```

Three vocabularies are in play: Figma `Semantic` (`bg/surface`, `text/primary`),
shadcn (`--card`, `--foreground`), Tailwind (`--color-*`). If they are not
reconciled deliberately the system is decorative.

The pragmatic call is **not** to rename shadcn's variables — the vendored
components expect them, and the churn buys nothing. Instead treat the shadcn
layer as tier 2 for the app, aliasing tier 1, and write the mapping table once.
`--color-tertiary`'s hardcoded `#64748b` is a live violation and should go early
as the canary.

Export path: Figma variables → JSON → Style Dictionary → CSS custom properties
consumed by `@theme`. One source of truth, generated rather than retyped.

## 4. Order of work

1. ~~**Widen tier 0 and tier 1 in Figma.**~~ **Done, 17 August 2026.** See §7 for
   what was built and what the contrast audit changed.
2. **Build `app-library--light` as the reference implementation.** Every raw fill
   mapped per §1's table, `text/secondary` unpicked from strokes, the five Inter
   nodes moved to Source Sans 3, the gatehouse mark placed in the title bar, the
   197 wrappers collapsed. Verify it resolves in both modes, then delete
   `app-library--dark`.
3. **Write the mapping table down** — raw hex → token, and Figma token → shadcn
   variable. This is what makes step 4 mechanical.
4. **Roll out** to `workshop` and `recording`, then to the states the prototype
   still holds (loading, empty, error; 900 and 1600 widths; the settings panes).
5. **Add tier 2 and wire the export** to Style Dictionary once two views have
   proven the vocabulary.

## 5. The logomark

`gatehouse.svg` is a single 20×20 path filled `#C07A3A` — exactly
`bronze/light`. It should be imported as a vector and its fill **bound to a
variable**, not left as a literal, so the mark inverts correctly in dark mode.
The brief's requirement that it survive at 16 px is satisfied by the geometry:
one path, no strokes, no detail below ~2 px.

## 6. Model routing

The work splits cleanly, and the split is not about difficulty but about
reversibility.

**Opus — the decisions.** Naming tiers 0–2, constructing the ramps, the
Figma↔shadcn reconciliation, and building `app-library--light` end to end as the
reference. These are few, high-consequence, and expensive to undo: a wrong token
name propagates into every frame and every stylesheet before anyone notices.
Proving the vocabulary on one real view is what turns opinion into spec.

**Fable — the rollout.** Creating several hundred variables and aliases,
rebinding the remaining views, collapsing wrappers, generating the export. This
is voluminous, repetitive and fully specified once step 3 exists — which is
exactly the whole-job handoff shape, and exactly what Opus should not spend a
session doing by hand.

The dividing line is step 3. Everything before it is judgement; everything after
it is execution.

## 7. Tier 0 and tier 1 as built

Done 17 August 2026. Exported to
[design/tokens/halvern.tokens.json](../design/tokens/halvern.tokens.json), which
is the code-side source of truth; edit in Figma and re-export rather than
hand-editing it.

**103 core tokens, 38 semantic, 22 retired.**

The ramps were anchored on the brand rather than invented alongside it. Twelve
of the fourteen `stone` stops and four of the eleven `bronze` stops are the
exact values the hand-picked primitives already held; only four stops are new,
and they close gaps the picks had left open. `stone` carries fourteen stops
rather than the usual eleven because a dark interface needs finer surface
separation than a light one, and discarding brand values to fit a conventional
scale would have been the wrong trade.

Every `Semantic` alias was then re-pointed off the hand-picked primitives and
onto the ramp — twenty-nine aliases, **verified byte-identical before and
after**, so the refactor moved names without moving a single colour. The picks
that were left unreferenced were renamed to `_legacy/*` rather than deleted,
which keeps the change reversible; they can go once nothing outside the file
uses them.

Three axes that did not exist at all now do: typography (families verified
against what the machine actually has installed, sizes, weights, leading as
percent because that is what Figma binds, tracking), spacing on Tailwind's
4-px step so a Figma decision reads across to a class name, and radii. Motion
tokens exist but are scoped to nothing — Figma cannot bind time or easing, so
they are export-only.

### What the contrast audit changed

Resolving all 38 semantic tokens in both modes and measuring the pairs the
interface actually renders found six problems that reading the palette would
not have:

| Token | Was | Now | Why |
|---|---|---|---|
| `text/tertiary` (dark) | 2.99:1 | 4.35:1 | Row metadata is real content, not decoration |
| `text/disabled` (light) | 1.71:1 | 3.25:1 | Read as invisible rather than inactive |
| `status/warning/text` (light) | 4.13:1 | 8.9:1 | Yellows are light; warning needs a darker text stop than the other hues |
| `accent/on-accent` (dark) | 3.45:1 | 5.2:1 | White on mid-bronze fails; in dark mode the accent is the bright element, so its label goes to ink |
| `interactive/primary-text` (dark) | 3.45:1 | 5.2:1 | Same defect on the primary button label |
| `text/secondary` vs `text/tertiary` (dark) | identical | distinct | The first fix had collapsed two levels into one |

The text ladder now reads 14.13 → 5.95 → 4.72 → 3.25 in light and
14.13 → 6.85 → 4.35 → 2.99 in dark: four distinct levels in both modes, with
every content level clearing AA and only the WCAG-exempt disabled step below it.
All twenty-two content pairs pass AA; nothing sits in the large-text-only band.

Elevation is still missing and cannot be a variable — Figma models shadows as
effect styles. It belongs with the text styles in the next step.

## 8. The library view as built

Done 17 August 2026. The frame is `app-library` (`64:178`), 1029 nodes, stored
in `Light`, and it renders `Dark` correctly by switching the collection mode.
**One frame now serves both themes** — the goal §2 set. The old dark capture is
renamed `_superseded--app-library--dark` and locked rather than deleted.

Only three nodes still carry raw paint, and they are the macOS traffic lights,
which must not follow the theme.

### What the counts in §1 got wrong

Two numbers in the original audit were measured badly, and both mattered.

**The raw colour count missed strokes.** Counting fills only put the inherited
Meetily teal at 49 uses. Including strokes it is **129** — 49 fills, 49 frame
strokes and 31 icon strokes. The full mapping bound 369 paints across 266 nodes:

| Was | Uses | Became |
|---|---|---|
| `#007176` / `#50afb4` | 129 | `accent/default` |
| `#ffffff` fills | 50 | `bg/overlay` |
| `#ffffff` strokes | 49 | `accent/on-accent` |
| `#dfdeda` / `#363531` | 72 | `border/default` |
| `#767676` | 49 | `border/strong` |
| `#fafaf9` / `#171614` | 5 | `bg/app` |
| traffic lights | 3 | left raw |

**"197 redundant wrappers" was wrong, and acting on it broke the view.** The
real figure for single-child auto-layout frames is 453; of those only 51 were
inert by the test I wrote — no padding, fill, stroke, radius, clip, and sizing
matching the child. Collapsing those 51 destroyed the layout: titles truncated
to three characters and row heights grew by 31 px.

The test was incomplete in two ways. It never checked `layoutMode`, so a wrapper
that flips the axis counted as inert when it is doing the most important work on
the row. And it treated `GRID` as ordinary auto-layout — in a grid, children
occupy cells **in order**, so removing one shifts every sibling after it into
the wrong cell. That is why the damage was not local.

Recovery came from the dark twin, which was untouched and, as it happened,
carried more bindings than the light frame did. Cloning it, binding its
dark-palette raws to the same tokens and fixing its fonts reproduced everything
in a few minutes. The lesson worth keeping is not "be careful with wrappers" but
**dry-run structural edits and diff the render**, which is what caught it — and
keep an intact copy of anything being restructured until the result is verified.

### Still open on this view

- The title-bar instance is still named `brand bar/Dark`. It inherits the
  frame's mode and renders correctly in both, so the name is cosmetic — but it
  will mislead the next person, and the component's Light/Dark variants now
  duplicate what the mode switch does on its own.
- Elevation and text styles are still unbuilt; the type ramp exists as variables
  but no text styles apply it.
- The 402 wrappers that genuinely do layout work are untouched and should stay
  that way unless a specific one is proven inert by a stricter test.
