import * as React from "react"

export type Verdict = "pass" | "fail" | "todo"

/**
 * Bumped only when the shape of a stored run changes incompatibly. Case ids
 * are stable, so adding or renaming cases does not need a bump -- an unknown
 * id simply reads back as "todo" and a dropped one is ignored.
 */
const KEY = "journeybook-run-v1"

type RunMap = Record<string, Verdict>

function read(): RunMap {
  try {
    const raw = localStorage.getItem(KEY)
    return raw ? (JSON.parse(raw) as RunMap) : {}
  } catch {
    // A private window, cleared site data, or a browser blocking storage.
    // The page has to work either way, so an empty run is the right answer.
    return {}
  }
}

function write(map: RunMap) {
  try {
    localStorage.setItem(KEY, JSON.stringify(map))
  } catch {
    // Nothing to do. The run stays in memory for this session.
  }
}

export function useRunState() {
  const [run, setRun] = React.useState<RunMap>(read)

  // Keep two tabs of the same playbook in step.
  React.useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === KEY) setRun(read())
    }
    window.addEventListener("storage", onStorage)
    return () => window.removeEventListener("storage", onStorage)
  }, [])

  const setVerdict = React.useCallback((id: string, v: Verdict) => {
    setRun((prev) => {
      const next = { ...prev }
      if (v === "todo") delete next[id]
      else next[id] = v
      write(next)
      return next
    })
  }, [])

  const reset = React.useCallback(() => {
    setRun({})
    write({})
  }, [])

  const verdictOf = React.useCallback(
    (id: string): Verdict => run[id] ?? "todo",
    [run]
  )

  return { run, setVerdict, reset, verdictOf }
}

export type RunState = ReturnType<typeof useRunState>
