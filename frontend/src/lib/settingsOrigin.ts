/**
 * Where Settings was opened from, so leaving it returns there.
 *
 * Settings is reached from three places — the gear in the top bar, the tray
 * item, and ⌘, from the application menu — and from any screen. Leaving it
 * used to go to the Library unconditionally, which meant opening Settings
 * mid-recording and pressing Escape put you on a screen you had not asked for.
 *
 * `router.back()` would express this in one line and is not used, for one
 * reason: it depends on the webview's history being intact. A reload while
 * Settings is open — a dev-server refresh, or anything that restarts the
 * webview on that route — leaves an empty history, and `back()` then does
 * nothing at all. A stored origin degrades to the Library instead, which is
 * the behaviour this replaces rather than a new failure.
 *
 * sessionStorage rather than a context: the value has to survive that same
 * reload, and it must not survive the application being closed and reopened.
 */

const KEY = 'halvern:settings-origin';

/** Settings cannot be its own way out, and an empty path is not a screen. */
function isReturnable(path: string | null | undefined): path is string {
  return typeof path === 'string' && path.length > 0 && path !== '/settings';
}

/** Call at the moment of navigating into Settings, with the screen being left. */
export function rememberSettingsOrigin(path: string | null | undefined): void {
  try {
    if (isReturnable(path)) {
      sessionStorage.setItem(KEY, path);
    } else {
      sessionStorage.removeItem(KEY);
    }
  } catch {
    // sessionStorage throws when site data is blocked. Losing the origin costs
    // a return to the Library, so there is nothing to report and nothing to do.
  }
}

/**
 * The screen to return to, consumed in the process. Falls back to the Library,
 * which is where Settings has always sent people.
 */
export function takeSettingsOrigin(): string {
  try {
    const stored = sessionStorage.getItem(KEY);
    sessionStorage.removeItem(KEY);
    return isReturnable(stored) ? stored : '/';
  } catch {
    return '/';
  }
}
