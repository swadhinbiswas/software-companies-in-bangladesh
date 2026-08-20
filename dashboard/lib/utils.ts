import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatSalary(min: number | null, max: number | null, currency: string | null) {
  if (min == null && max == null) return null
  const fmt = (n: number) => n >= 1000 ? `${(n/1000).toFixed(n % 1000 === 0 ? 0 : 1)}k` : `${n}`
  if (min != null && max != null && min === max) return `${fmt(min)} ${currency || "BDT"}`
  if (min != null && max != null) return `${fmt(min)}–${fmt(max)} ${currency || "BDT"}`
  if (min != null) return `${fmt(min)} ${currency || "BDT"}`
  return `${fmt(max!)} ${currency || "BDT"}`
}
