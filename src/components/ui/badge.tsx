import { cva, type VariantProps } from "class-variance-authority";
import { type HTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "type-caption inline-flex min-h-6 items-center rounded-full border px-2 font-medium leading-none",
  {
    variants: {
      tone: {
        neutral: "border-border bg-surface text-muted",
        success: "border-success/45 bg-success/12 text-success",
        warning: "border-warning/45 bg-warning/12 text-warning",
        destructive: "border-destructive/45 bg-destructive/12 text-destructive",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export interface BadgeProps
  extends HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {}

/** A compact, text-first status primitive with semantic color roles. */
export function Badge({ className, tone, ...props }: BadgeProps): JSX.Element {
  return <span className={cn(badgeVariants({ className, tone }))} {...props} />;
}
