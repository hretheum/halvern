/**
 * The Halvern gatehouse mark.
 *
 * Inline rather than an <img> so it inherits `currentColor`: the mark has to be
 * bronze on both themes, and the two bronzes differ (`bronze/700` on light,
 * `bronze/500` on dark). An exported PNG would freeze one of them and be wrong
 * in the other. Callers set the colour with a text utility — `text-primary`
 * resolves to whichever bronze the active theme calls for.
 *
 * The geometry is one path with no strokes, which is what lets it survive at
 * 16px in the menu bar; keep it that way. Source of truth for the artwork is
 * design/brand/gatehouse.svg.
 */
export function HalvernMark({
  size = 20,
  className,
  title,
}: {
  size?: number;
  className?: string;
  /** Omit for decorative use beside the wordmark; the SVG is then hidden from
   *  assistive tech rather than announced as an unlabelled graphic. */
  title?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
    >
      {title ? <title>{title}</title> : null}
      <path
        d="M17 0C18.6569 0 20 1.34315 20 3V19C20 19.5523 19.5523 20 19 20H14.0557C14.2608 19.4209 14.375 18.7882 14.375 18.125C14.375 15.3636 12.4162 13.125 10 13.125C7.58375 13.125 5.625 15.3636 5.625 18.125C5.625 18.7882 5.73916 19.4209 5.94434 20H1C0.447715 20 8.05326e-09 19.5523 0 19V3C0 1.34315 1.34315 6.44255e-08 3 0H17Z"
        fill="currentColor"
      />
    </svg>
  );
}
