'use client'

import { useEffect } from 'react'
import { useTheme } from 'next-themes'
import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * Makes "System" mean the operating system, not the webview's opinion of it.
 *
 * `next-themes` resolves the system theme from `prefers-color-scheme`. Inside a
 * Tauri window that answer comes from the webview's effective appearance, which
 * on macOS was reported as light even with the system set to dark and no theme
 * forced in the configuration. Tauri asks the OS directly and gets it right, so
 * the class is driven from there instead.
 *
 * Driving it once is not enough. `next-themes` re-applies its own (wrong)
 * answer from its effects, so whoever writes last wins — and strict mode's
 * double-invoked effects changed that order, which is how the packaged app
 * started light on a dark system. A MutationObserver on the <html> class keeps
 * the OS answer authoritative no matter who writes when: every stomp is
 * corrected immediately, and once the class matches, correcting stops, so the
 * two writers converge instead of looping.
 *
 * Only the `system` choice is bridged. An explicit Light or Dark is the user
 * overriding the OS, and `next-themes` already handles that correctly — taking
 * it over here would fight the library and lose the stored preference.
 */
export function SystemThemeBridge() {
  const { theme } = useTheme()

  useEffect(() => {
    if (theme !== 'system') return

    let cancelled = false
    let unlisten: (() => void) | undefined
    // null until the OS has answered; the observer stays passive that long.
    let osTheme: string | null = null

    const enforce = () => {
      if (cancelled || !osTheme) return
      const wantDark = osTheme === 'dark'
      if (document.documentElement.classList.contains('dark') !== wantDark) {
        document.documentElement.classList.toggle('dark', wantDark)
      }
    }

    const observer = new MutationObserver(enforce)
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class'],
    })

    // Outside Tauri there is no window to ask — getCurrentWindow() throws
    // synchronously, which in an effect takes the whole tree down. A browser
    // preview then falls back to next-themes' own answer, which is correct
    // there: a real browser's media query is not the one that lies.
    let win: ReturnType<typeof getCurrentWindow>
    try {
      win = getCurrentWindow()
    } catch {
      observer.disconnect()
      return
    }

    win
      .theme()
      .then((value) => {
        // Logged once per run: when the interface and the system disagree, this
        // line says which of the two layers was wrong.
        console.log('[theme] OS reports', value, '| media query reports',
          window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
        osTheme = value
        enforce()
      })
      .catch((e) => console.warn('[theme] could not read the OS theme:', e))

    win
      .onThemeChanged(({ payload }) => {
        osTheme = payload
        enforce()
      })
      .then((fn) => {
        if (cancelled) fn()
        else unlisten = fn
      })
      .catch((e) => console.warn('[theme] could not subscribe to OS theme changes:', e))

    return () => {
      cancelled = true
      observer.disconnect()
      unlisten?.()
    }
  }, [theme])

  return null
}
