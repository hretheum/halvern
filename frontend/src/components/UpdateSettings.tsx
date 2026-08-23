"use client"

/**
 * Update checking: the setting, the manual check, and the once-per-launch one.
 *
 * The check itself lives in Rust (`updater.rs`) and is reached through the
 * app's own commands, so this file decides *when* to ask and what to show —
 * never how to talk to the network.
 *
 * `UpdateOnLaunch` runs at most once per process, and only when the setting is
 * on. It says nothing when there is no update and nothing when the check
 * fails: nobody asked, so a failure is not news. `UpdateSettings` is the
 * opposite — the user pressed a button, so every outcome gets an answer,
 * including "you are already on the newest one".
 */

import { useCallback, useEffect, useRef, useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { load } from "@tauri-apps/plugin-store"
import { relaunch } from "@tauri-apps/plugin-process"
import { Switch } from "./ui/switch"
import { Loader2, Download, RefreshCw } from "lucide-react"

const STORE_FILE = "updates.json"
const CHECK_ON_LAUNCH_KEY = "checkOnLaunch"

interface UpdateInfo {
  available: boolean
  current_version: string
  new_version: string | null
  notes: string | null
}

async function readCheckOnLaunch(): Promise<boolean> {
  try {
    const store = await load(STORE_FILE, { autoSave: false, defaults: { [CHECK_ON_LAUNCH_KEY]: true } })
    const value = await store.get<boolean>(CHECK_ON_LAUNCH_KEY)
    // Default on. The point of an updater is reaching people who never look,
    // and PRIVACY_POLICY.md says plainly what the check discloses.
    return value ?? true
  } catch {
    return true
  }
}

async function writeCheckOnLaunch(value: boolean): Promise<void> {
  const store = await load(STORE_FILE, { autoSave: false, defaults: { [CHECK_ON_LAUNCH_KEY]: true } })
  await store.set(CHECK_ON_LAUNCH_KEY, value)
  await store.save()
}

/**
 * One check per process, however many times this mounts.
 *
 * Module scope rather than a ref: React strict mode mounts twice in
 * development, and a ref would let the second mount check again.
 */
let launchCheckStarted = false

export function UpdateOnLaunch() {
  const [update, setUpdate] = useState<UpdateInfo | null>(null)
  const [installing, setInstalling] = useState(false)
  const [dismissed, setDismissed] = useState(false)

  useEffect(() => {
    if (launchCheckStarted) return
    launchCheckStarted = true

    let cancelled = false
    ;(async () => {
      if (!(await readCheckOnLaunch())) return
      try {
        const result = await invoke<UpdateInfo>("check_for_update")
        if (!cancelled && result.available) setUpdate(result)
      } catch (error) {
        // Nobody asked, so nobody is told. The manual check reports failures.
        console.warn("Update check failed:", error)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [])

  if (!update || dismissed) return null

  const install = async () => {
    setInstalling(true)
    try {
      await invoke("install_update")
      await relaunch()
    } catch (error) {
      console.error("Update installation failed:", error)
      setInstalling(false)
    }
  }

  return (
    <div className="fixed bottom-4 right-4 z-50 max-w-sm rounded-lg border border-border bg-card p-4 shadow-lg">
      <p className="text-sm font-medium text-foreground">
        Halvern {update.new_version} is available
      </p>
      <p className="mt-1 text-xs text-muted-foreground">
        You are running {update.current_version}.
      </p>
      <div className="mt-3 flex gap-2">
        <button
          onClick={install}
          disabled={installing}
          className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
        >
          {installing ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin" /> Installing…
            </>
          ) : (
            <>
              <Download className="h-3.5 w-3.5" /> Install and restart
            </>
          )}
        </button>
        <button
          onClick={() => setDismissed(true)}
          disabled={installing}
          className="rounded-md px-3 py-1.5 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent disabled:pointer-events-none disabled:opacity-50"
        >
          Later
        </button>
      </div>
    </div>
  )
}

export function UpdateSettings() {
  const [checkOnLaunch, setCheckOnLaunch] = useState<boolean | null>(null)
  const [checking, setChecking] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [result, setResult] = useState<string | null>(null)
  const [update, setUpdate] = useState<UpdateInfo | null>(null)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    readCheckOnLaunch().then((value) => {
      if (mounted.current) setCheckOnLaunch(value)
    })
    return () => {
      mounted.current = false
    }
  }, [])

  const toggle = useCallback(async (value: boolean) => {
    setCheckOnLaunch(value)
    try {
      await writeCheckOnLaunch(value)
    } catch (error) {
      console.error("Could not save the update preference:", error)
      if (mounted.current) setCheckOnLaunch(!value)
    }
  }, [])

  const checkNow = async () => {
    setChecking(true)
    setResult(null)
    setUpdate(null)
    try {
      const info = await invoke<UpdateInfo>("check_for_update")
      if (!mounted.current) return
      if (info.available) {
        setUpdate(info)
        setResult(null)
      } else {
        setResult(`You are on the newest version (${info.current_version}).`)
      }
    } catch (error) {
      if (mounted.current) setResult(`Could not check: ${error}`)
    } finally {
      if (mounted.current) setChecking(false)
    }
  }

  const install = async () => {
    setInstalling(true)
    try {
      await invoke("install_update")
      await relaunch()
    } catch (error) {
      if (mounted.current) {
        setResult(`Installation failed: ${error}`)
        setInstalling(false)
      }
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="pr-4">
          <p className="text-sm font-medium text-foreground">Check for updates on launch</p>
          <p className="text-xs text-muted-foreground">
            Fetches one file from GitHub, once per launch, to see whether a newer
            version exists. No identifier, nothing about your meetings, and never
            on a timer.
          </p>
        </div>
        <Switch
          checked={checkOnLaunch ?? true}
          disabled={checkOnLaunch === null}
          onCheckedChange={toggle}
        />
      </div>

      <div className="flex items-center gap-3">
        <button
          onClick={checkNow}
          disabled={checking || installing}
          className="inline-flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-accent disabled:pointer-events-none disabled:opacity-50"
        >
          {checking ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin" /> Checking…
            </>
          ) : (
            <>
              <RefreshCw className="h-3.5 w-3.5" /> Check now
            </>
          )}
        </button>
        {result && <span className="text-xs text-muted-foreground">{result}</span>}
      </div>

      {update?.available && (
        <div className="rounded-md border border-border p-3">
          <p className="text-sm font-medium text-foreground">
            Halvern {update.new_version} is available
          </p>
          <p className="mt-1 text-xs text-muted-foreground">
            You are running {update.current_version}.
          </p>
          <button
            onClick={install}
            disabled={installing}
            className="mt-2 inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:pointer-events-none disabled:opacity-50"
          >
            {installing ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" /> Installing…
              </>
            ) : (
              <>
                <Download className="h-3.5 w-3.5" /> Install and restart
              </>
            )}
          </button>
        </div>
      )}
    </div>
  )
}
