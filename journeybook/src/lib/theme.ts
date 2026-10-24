import * as React from "react"

export type Theme = "light" | "dark" | "system"

const KEY = "journeybook-theme"

function apply(theme: Theme) {
  const dark =
    theme === "dark" ||
    (theme === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches)
  document.documentElement.classList.toggle("dark", dark)
}

export function useTheme() {
  const [theme, setThemeState] = React.useState<Theme>(() => {
    try {
      return (localStorage.getItem(KEY) as Theme | null) ?? "system"
    } catch {
      return "system"
    }
  })

  React.useEffect(() => {
    apply(theme)
    try {
      localStorage.setItem(KEY, theme)
    } catch {
      // Preference is not persistable here; the page still renders correctly.
    }
    if (theme !== "system") return
    // Only "system" needs to follow the OS after the first paint.
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const onChange = () => apply("system")
    mq.addEventListener("change", onChange)
    return () => mq.removeEventListener("change", onChange)
  }, [theme])

  return { theme, setTheme: setThemeState }
}
