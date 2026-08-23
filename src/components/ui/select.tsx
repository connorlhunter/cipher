import { type JSX, type SelectHTMLAttributes } from "react";

import { cn } from "../../lib/utils";

/** A reviewed native select with semantic surface, border, and focus treatment. */
export function Select({
  className,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>): JSX.Element {
  return (
    <select
      className={cn(
        "type-label min-h-9 min-w-0 rounded-md border border-border bg-surface px-2 text-text disabled:cursor-not-allowed disabled:opacity-55",
        className,
      )}
      {...props}
    />
  );
}
