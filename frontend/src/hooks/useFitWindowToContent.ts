import { useEffect, useRef, type RefObject } from 'react';

/**
 * Grow the window until a scroll container has nothing left to scroll, without
 * ever growing past the screen the window is on.
 *
 * Onboarding is the one flow where the window's configured 1100×700 is a
 * guess about content nobody has seen yet: the summary-model step lists as
 * many models as the machine's memory allows, so its height depends on the
 * machine. At 700px it clipped the list and the button below it, and the only
 * hint that anything had been cut was that the last tile looked short.
 *
 * Two rules keep this from becoming its own bug:
 *
 * **It only grows.** Shrinking back on a short step would make the window
 * twitch on every navigation, and a window that resizes while you read it is
 * worse than one that is slightly too tall.
 *
 * **The ceiling is `workArea`, not the screen.** `Monitor.size` is the whole
 * panel, including the strip under the menu bar and behind the Dock; a window
 * sized to it has its controls under the Dock. `workArea` is what is actually
 * usable, and on a small display it is the binding constraint — the window
 * stops there and the content scrolls the rest of the way, which is why the
 * scroll container has to work on its own first.
 *
 * The window is also nudged back up if growing pushed its bottom off the work
 * area, because macOS grows a window downwards from its title bar.
 */

/** Breathing room below the last element, so the fit does not look accidental. */
const BOTTOM_MARGIN = 16;

/** Below this the resize is not worth the flicker. */
const MIN_WORTHWHILE_GROWTH = 8;

export function useFitWindowToContent(
  scrollRef: RefObject<HTMLElement | null>,
  enabled: boolean = true
) {
  // The size we last asked for. If the window is a different size when the
  // hook unmounts, the user resized it themselves and we leave it alone.
  const lastRequested = useRef<{ width: number; height: number } | null>(null);
  const originalHeight = useRef<number | null>(null);

  useEffect(() => {
    if (!enabled) return;
    if (typeof window === 'undefined' || !window.__TAURI_INTERNALS__) return;

    const el = scrollRef.current;
    if (!el) return;

    let cancelled = false;

    const fit = async () => {
      const overflow = el.scrollHeight - el.clientHeight;
      if (overflow < MIN_WORTHWHILE_GROWTH) return;

      const { getCurrentWindow, currentMonitor, LogicalSize, LogicalPosition } = await import(
        '@tauri-apps/api/window'
      );
      if (cancelled) return;

      const monitor = await currentMonitor();
      if (cancelled || !monitor) return;

      const scale = monitor.scaleFactor;
      const work = {
        x: monitor.workArea.position.x / scale,
        y: monitor.workArea.position.y / scale,
        width: monitor.workArea.size.width / scale,
        height: monitor.workArea.size.height / scale,
      };

      const win = getCurrentWindow();
      const outer = (await win.outerSize()).toLogical(scale);
      if (cancelled) return;

      if (originalHeight.current === null) originalHeight.current = outer.height;

      const wanted = Math.min(outer.height + overflow + BOTTOM_MARGIN, work.height);
      if (wanted - outer.height < MIN_WORTHWHILE_GROWTH) return;

      await win.setSize(new LogicalSize(outer.width, wanted));
      if (cancelled) return;
      lastRequested.current = { width: outer.width, height: wanted };

      // Growing pushes the bottom edge down. If that took it past the work
      // area, pull the whole window up rather than leaving the button under
      // the Dock — the thing this hook exists to prevent.
      const position = (await win.outerPosition()).toLogical(scale);
      if (cancelled) return;
      const highestAllowed = Math.max(work.y, work.y + work.height - wanted);
      const y = Math.min(Math.max(position.y, work.y), highestAllowed);
      if (Math.abs(y - position.y) >= 1) {
        await win.setPosition(new LogicalPosition(position.x, y));
      }
    };

    void fit();

    // Steps load their content asynchronously — the model list arrives from a
    // Tauri command after the first paint — so one measurement at mount is
    // measuring an empty box.
    const observer = new ResizeObserver(() => void fit());
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);

    return () => {
      cancelled = true;
      observer.disconnect();
    };
  }, [scrollRef, enabled]);

  // Give the window back when onboarding ends, but only if it is still the
  // size we made it.
  useEffect(() => {
    return () => {
      const requested = lastRequested.current;
      const original = originalHeight.current;
      if (!requested || original === null) return;
      if (typeof window === 'undefined' || !window.__TAURI_INTERNALS__) return;

      void (async () => {
        const { getCurrentWindow, LogicalSize } = await import('@tauri-apps/api/window');
        const win = getCurrentWindow();
        const scale = await win.scaleFactor();
        const outer = (await win.outerSize()).toLogical(scale);
        const untouched =
          Math.abs(outer.height - requested.height) < 2 &&
          Math.abs(outer.width - requested.width) < 2;
        if (untouched) await win.setSize(new LogicalSize(outer.width, original));
      })();
    };
  }, []);
}
