import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

/**
 * The notes and premise fields are authored with two inline markers and
 * nothing else: `code` and **bold**. Rendering exactly that subset keeps the
 * data file readable as plain text and avoids pulling in a markdown parser
 * for two constructs. Anything else is shown literally, on purpose -- so a
 * stray asterisk in a real error message is not silently eaten.
 */
export function Prose({
  children,
  className,
}: {
  children: string
  className?: string
}) {
  return <span className={className}>{render(children)}</span>
}

function render(text: string): ReactNode[] {
  const out: ReactNode[] = []
  const pattern = /`([^`]+)`|\*\*([^*]+)\*\*/g
  let last = 0
  let i = 0

  for (const m of text.matchAll(pattern)) {
    if (m.index > last) out.push(text.slice(last, m.index))
    if (m[1] !== undefined) {
      out.push(
        <code
          key={i++}
          className={cn(
            "rounded-[3px] bg-muted px-1 py-px font-mono text-foreground",
            "text-[0.9em]"
          )}
        >
          {m[1]}
        </code>
      )
    } else if (m[2] !== undefined) {
      out.push(
        <strong key={i++} className="font-semibold text-foreground">
          {m[2]}
        </strong>
      )
    }
    last = m.index + m[0].length
  }
  if (last < text.length) out.push(text.slice(last))
  return out
}
