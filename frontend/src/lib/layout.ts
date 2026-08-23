/**
 * The two vertical rules every screen aligns to.
 *
 * Before this existed, each component chose its own horizontal padding and the
 * meeting view ended up with eight different left edges — the date sat 22px
 * left of the paragraph it described, because `ml-8` was a guess at the width
 * of the back button rather than a consequence of it.
 *
 * The layout is the classic one for running text with marginalia:
 *
 *   16px                    72px
 *    │                       │
 *    │  ‹                    │  Meeting title
 *    │                       │  17 Aug 2026 · 15:00 · 57 min
 *    │  ⌕                    │  Search transcript…
 *    │  [00:00]              │  Me
 *    │                       │  I said I'd have it ready by…
 *
 * RULE 1 — the margin, at `INSET`. Everything that marks or points rather than
 * reads: the logo mark, the back chevron, the search magnifier, timestamps.
 * They form a column the eye skips when reading and finds when scanning.
 *
 * RULE 2 — the content edge, at `CONTENT`. Everything meant to be read: titles,
 * the metadata line under them, the text inside the search field, speaker
 * labels, transcript paragraphs.
 *
 * CONTENT is not a chosen number: it is INSET + GUTTER + GAP, where GUTTER is
 * the real width of a timestamp. Change the timestamp width and the content
 * edge follows, which is the property the old hardcoded values lacked.
 *
 * Two deliberate exemptions. The wordmark stays welded to the mark as a lockup
 * — stretching it to the content rule would break the logo — so only the mark
 * answers to RULE 1. And the toolbar is window chrome: it obeys the margin,
 * not the content edge.
 *
 * ONE TRAP, and it cost an afternoon. `GUTTER` is a width, so it only applies
 * to an element that can take one — a flex child, or something blockified.
 * Wrap it in anything that renders its own element (`TooltipTrigger` without
 * `asChild` is the case that bit) and the marginalia element becomes inline
 * inside that wrapper, `width` is silently ignored, the column shrinks to its
 * text, and every paragraph beside it slides left of the rule by the
 * difference. It measures correct in isolation and wrong in the tree, which is
 * why it survived a harness. If a gutter element sits inside a wrapper
 * component, put `GUTTER` on whatever ends up being the flex child — or use
 * `asChild` so no extra element appears — and add `block` for good measure.
 */

export const LAYOUT = {
  /** RULE 1. Horizontal padding of any screen-level container. 16px. */
  INSET: 'px-4',
  /** Width of the marginalia column. 48px — fits `1:00:00` at 12px. */
  GUTTER: 'w-12',
  /** Between the margin column and the content column. 8px. */
  GAP: 'gap-2',
  /**
   * RULE 2, for content that has no marginalia element beside it and must be
   * pushed to the content edge by hand. 56px — measured from inside a
   * container that already carries INSET, so 16 + 56 = 72.
   */
  CONTENT_OFFSET: 'ml-14',
  /**
   * Padding of a square icon button. Shared so that trailing icons in
   * different bands — the toolbar's gear, the meeting header's delete — land
   * on one vertical axis. They are only ever aligned by accident otherwise:
   * the axis is INSET + this padding + half the glyph, so a band differing in
   * either term puts its icon somewhere else.
   */
  ICON_BUTTON: 'p-1.5',
} as const;
