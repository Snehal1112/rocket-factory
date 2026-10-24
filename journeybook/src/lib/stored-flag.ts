import * as React from "react"

/**
 * A boolean that remembers itself in localStorage.
 *
 * The three existing persisted values -- theme, shortcuts and the run itself --
 * each hand-roll this, because each is lifted into App for a reason: the theme
 * lands on <html>, the shortcuts flag gates App's own keydown handler, and the
 * run is read by every row on the page. The rail's view preferences have no
 * such reason. They are read by the rail and nothing else, so they stay in the
 * rail, and this is the shared piece they would otherwise each copy.
 *
 * Encoding matches `journeybook-shortcuts`: a bare "on"/"off" sentinel rather
 * than JSON, with the default folded into the comparison so an absent key and
 * a blocked localStorage both fall back the same way.
 *
 * Every access is wrapped: on file:// in a private window the accessor itself
 * can throw, and a rail that cannot remember whether a panel was open is far
 * better than a rail that does not render.
 */
export function useStoredFlag(
  key: string,
  fallback: boolean
): [boolean, () => void] {
  const [on, setOn] = React.useState(() => {
    try {
      const raw = localStorage.getItem(key)
      return raw === null ? fallback : raw === "on"
    } catch {
      return fallback
    }
  })

  const toggle = React.useCallback(() => {
    setOn((prev) => {
      try {
        localStorage.setItem(key, prev ? "off" : "on")
      } catch {
        // Storage blocked; the choice still holds for this session.
      }
      return !prev
    })
  }, [key])

  return [on, toggle]
}
