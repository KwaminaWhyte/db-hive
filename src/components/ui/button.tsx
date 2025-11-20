import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium transition-all disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 [&_svg]:shrink-0 outline-none focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
  {
    variants: {
      variant: {
        default: "border border-amber-300/70 bg-gradient-to-r from-amber-400 via-amber-300 to-amber-400 text-slate-950 font-medium shadow-[0_4px_12px_rgba(251,191,36,0.3)] hover:-translate-y-[1px] active:translate-y-[1px] dark:text-slate-950",
        destructive:
          "bg-red-500/80 hover:bg-red-500 text-white font-medium border-0 focus-visible:ring-red-500/20 dark:bg-red-500/80 dark:hover:bg-red-500",
        outline:
          "border border-slate-700/60 bg-slate-900/60 text-slate-200 hover:bg-slate-900/90 hover:border-amber-300/40 shadow-xs dark:bg-slate-900/60 dark:border-slate-700/60 dark:text-slate-200 dark:hover:bg-slate-900/90 dark:hover:border-amber-300/40",
        secondary:
          "bg-slate-800/60 text-slate-200 hover:bg-slate-800/90 border border-slate-700/60 dark:bg-slate-800/60 dark:text-slate-200 dark:hover:bg-slate-800/90",
        ghost:
          "hover:bg-slate-800/80 hover:text-amber-300 dark:hover:bg-slate-800/80 dark:hover:text-amber-300",
        link: "text-amber-300 underline-offset-4 hover:underline dark:text-amber-300",
      },
      size: {
        default: "h-9 px-4 py-2 has-[>svg]:px-3",
        sm: "h-8 rounded-lg gap-1.5 px-3 has-[>svg]:px-2.5",
        lg: "h-10 rounded-lg px-6 has-[>svg]:px-4",
        icon: "size-9 rounded-lg",
        "icon-sm": "size-8 rounded-lg",
        "icon-lg": "size-10 rounded-lg",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot : "button"

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button, buttonVariants }
