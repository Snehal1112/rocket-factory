import * as React from "react"
import { Check, Copy } from "lucide-react"
import { cn } from "@/lib/utils"

/**
 * A captured shell block. It is drawn entirely from the --term* tokens, which
 * each theme defines for itself: a recessed light well in light mode, and the
 * page's own ground in dark. Nothing here assumes either.
 *
 * Each logical line is its own block element so that an over-long line wraps
 * with a hanging indent rather than scrolling off the right edge. Copying a
 * selection still yields the original line breaks, because adjacent block
 * elements are separated by newlines when the browser serialises a selection
 * -- and the Copy button bypasses the DOM entirely, writing the source string.
 */
export function Terminal({
  children,
  label,
  copy,
  tone = "command",
  className,
}: {
  children: string
  /**
   * Omit inside a labelled layout -- the case panel puts the label in its own
   * gutter column, and a second one here would say the same word twice.
   */
  label?: string
  copy?: boolean
  tone?: "command" | "output"
  className?: string
}) {
  const [copied, setCopied] = React.useState(false)
  const lines = React.useMemo(() => children.split("\n"), [children])

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(children)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch {
      // Clipboard is unavailable over file:// in some browsers. The text is
      // selectable, so the reader can still copy it by hand.
    }
  }

  return (
    <div className={cn("group/term", className)}>
      {label || copy ? (
        <div className="label mb-1.5 flex items-center justify-between text-muted-foreground">
          <span>{label}</span>
          {copy ? (
            <button
              type="button"
              onClick={onCopy}
              className="focus-ring label inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 text-muted-foreground hover:text-foreground"
            >
              {copied ? (
                <Check className="size-3" aria-hidden="true" />
              ) : (
                <Copy className="size-3" aria-hidden="true" />
              )}
              {copied ? "Copied" : "Copy"}
            </button>
          ) : null}
        </div>
      ) : null}
      <pre
        className={cn(
          // pre-wrap plus break-word means nothing should overflow, but an
          // unbreakable token would be clipped irrecoverably by
          // overflow-hidden. Let it scroll instead.
          "transcript overflow-x-auto rounded-md border border-term-border bg-term px-3.5 py-3",
          tone === "command" ? "text-term-foreground" : "text-term-muted"
        )}
      >
        <code>
          {lines.map((line, i) => (
            <span
              // Lines are static content with no identity of their own, and
              // the list never reorders -- the index is the only stable key.
              // biome-ignore lint/suspicious/noArrayIndexKey: see above.
              key={i}
              className="transcript-line"
            >
              {/* A genuinely blank line still needs to occupy a line box. */}
              {line === "" ? " " : line}
            </span>
          ))}
        </code>
      </pre>
    </div>
  )
}
