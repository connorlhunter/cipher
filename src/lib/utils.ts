import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/** Merges conditional Tailwind class names while retaining the final utility. */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
