'use client'

import { useEffect, useState } from 'react'
import { Toaster } from 'sonner'

/**
 * Toasts that follow the app's theme.
 *
 * Sonner paints its own surface and keeps its own idea of light and dark. Left
 * alone it defaults to light, which is why the recording-notification toast
 * stayed white on a dark app — the content inside it uses tokens and was
 * correct; the container around it was not.
 *
 * The theme is read from the `dark` class on `<html>`, not from
 * `useTheme().resolvedTheme` and not from sonner's own `theme="system"`. Both
 * of those resolve "system" through `prefers-color-scheme`, which the Tauri
 * webview answers wrongly on macOS — the reason `SystemThemeBridge` exists at
 * all. The class is what actually decides how every other surface in the app
 * is painted, so it is the honest source here too, whoever wrote it last.
 *
 * A MutationObserver keeps it current: the bridge corrects that class after
 * next-themes writes its own answer, so a value sampled once at mount can be
 * the wrong one.
 */
export function ThemedToaster() {
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

  return <Toaster position="bottom-center" richColors closeButton theme={theme} />
}
