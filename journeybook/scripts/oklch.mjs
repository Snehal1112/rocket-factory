// oklch -> linear sRGB -> WCAG relative luminance -> contrast ratio.
export function oklch2lin(L, C, H) {
  const h = (H * Math.PI) / 180
  const a = C * Math.cos(h), b = C * Math.sin(h)
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b
  const s_ = L - 0.0894841775 * a - 1.2914855480 * b
  const l = l_ ** 3, m = m_ ** 3, s = s_ ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
  ]
}
const clamp = (x) => Math.min(1, Math.max(0, x))
export const lum = ([r, g, b]) =>
  0.2126 * clamp(r) + 0.7152 * clamp(g) + 0.0722 * clamp(b)
export const contrast = (f, b) => {
  const [a, z] = [lum(f), lum(b)].sort((x, y) => y - x)
  return (a + 0.05) / (z + 0.05)
}
const enc = (u) => { u = clamp(u); return u <= 0.0031308 ? 12.92*u : 1.055*Math.pow(u,1/2.4)-0.055 }
export const hex = (lin) =>
  "#" + lin.map((u) => Math.round(enc(u)*255).toString(16).padStart(2,"0")).join("")
/** Composite a foreground at alpha over an opaque backdrop, in linear light. */
export const over = (fg, bg, alpha) => fg.map((c, i) => c*alpha + bg[i]*(1-alpha))
export const p = (s) => { const m = s.match(/oklch\(([\d.]+) ([\d.]+) ([\d.]+)\)/); return oklch2lin(+m[1], +m[2], +m[3]) }
