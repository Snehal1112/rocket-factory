import type * as React from "react"

/**
 * The Rocket Factory mark: the same teal tile as the RocketVault family, with
 * three nodes on a line for the pipeline (spec, build, merge).
 *
 * It is inlined rather than imported as a file for the same reason the fonts
 * are: this page ships as one HTML file opened from disk, so a `<img src>`
 * pointing at a sibling asset would resolve to nothing once the file is
 * emailed on. Keep it in step with the data URI in index.html.
 *
 * The colours are the brand's own hex values, not theme tokens. A logo does
 * not restate itself per theme, and the teal-on-deep-teal pair carries on
 * both the paper and the dark ground.
 */
export function BrandMark({
  className,
  ...props
}: React.ComponentProps<"svg">) {
  return (
    <svg
      viewBox="0 0 64 64"
      className={className}
      role="img"
      aria-label="Rocket Factory"
      {...props}
    >
      <rect x="4" y="4" width="56" height="56" rx="14" fill="#0FA89A" />
      <rect x="9" y="9" width="46" height="46" rx="10" fill="#0A6B62" />
      <path
        d="M20 32H44"
        stroke="#FFFFFF"
        strokeWidth="2.4"
        strokeLinecap="round"
      />
      <circle cx="20" cy="32" r="4" fill="#0A6B62" stroke="#FFFFFF" strokeWidth="2.4" />
      <circle cx="32" cy="32" r="4" fill="#0A6B62" stroke="#FFFFFF" strokeWidth="2.4" />
      <circle cx="44" cy="32" r="4.6" fill="#FFFFFF" />
    </svg>
  )
}
