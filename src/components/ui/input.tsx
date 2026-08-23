import { type InputHTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

/** A reviewed input primitive with semantic surface, border, and focus treatment. */
export function Input({
  className,
  type = "text",
  ...props
}: InputHTMLAttributes<HTMLInputElement>): JSX.Element {
  return (
    <input
      className={cn(
        "type-body min-h-control w-full rounded-md border border-border bg-surface px-3 text-text placeholder:text-muted disabled:cursor-not-allowed disabled:opacity-55",
        className,
      )}
      type={type}
      {...props}
    />
  );
}
