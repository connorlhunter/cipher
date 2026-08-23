import { cva, type VariantProps } from "class-variance-authority";
import { type ButtonHTMLAttributes, type JSX } from "react";

import { cn } from "../../lib/utils";

const buttonVariants = cva(
  "type-label inline-flex min-h-control items-center justify-center gap-2 rounded-md px-3 transition-[background-color,border-color,color,box-shadow] motion-reduce:transition-none disabled:pointer-events-none disabled:opacity-55",
  {
    variants: {
      variant: {
        default: "bg-accent text-on-accent hover:bg-accent-hover active:bg-accent-active",
        secondary: "border border-border bg-surface text-text hover:bg-elevated active:bg-canvas",
        ghost: "text-text hover:bg-elevated active:bg-surface",
        destructive:
          "bg-destructive text-on-destructive hover:bg-destructive-hover active:bg-destructive-active",
      },
      size: {
        default: "min-w-11",
        compact: "min-h-9 min-w-9 px-2",
      },
    },
    defaultVariants: {
      size: "default",
      variant: "default",
    },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {}

/** A reviewed, source-owned button primitive for keyboard-safe desktop controls. */
export function Button({
  className,
  size,
  type = "button",
  variant,
  ...props
}: ButtonProps): JSX.Element {
  return (
    <button className={cn(buttonVariants({ className, size, variant }))} type={type} {...props} />
  );
}

export { buttonVariants };
