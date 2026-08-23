'use client'

import { useEffect, useState } from 'react'

/**
 * The theme the interface is actually rendered in: 'light' or 'dark'.
 *
 * Read off the <html> class rather than from `useTheme().resolvedTheme`,
 * because with the System choice inside Tauri the class is written by the
 * SystemThemeBridge from the OS answer, while resolvedTheme still reflects the
 * webview's media query — the one that lies. The class is the single place
 * every writer converges on, so components that cannot inherit their colors
 * from CSS (BlockNote takes a theme prop) follow it.
 *
 * Starts as 'light' before mount: the value is only wrong for the first paint
 * of a component that is itself inside a themed tree, and correcting on mount
 * beats guessing at markup the server never themed.
 */
export function useResolvedThemeClass(): 'light' | 'dark' {
  const [theme, setTheme] = useState<'light' | 'dark'>('light')

  useEffect(() => {
    const read = () =>
      setTheme(document.documentElement.classList.contains('dark') ? 'dark' : 'light')

    read()
    const observer = new MutationObserver(read)
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class'],
    })
    return () => observer.disconnect()
  }, [])

  return theme
}
