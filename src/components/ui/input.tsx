import * as React from "react"

import { cn } from "@/lib/utils"

function Input({ className, type, onMouseDown, ...props }: React.ComponentProps<"input">) {
  // Stop propagation to prevent parent drag handlers from interfering with input focus
  const handleMouseDown = (e: React.MouseEvent<HTMLInputElement>) => {
    e.stopPropagation();
    onMouseDown?.(e);
  };

  return (
    <input
      type={type}
      data-slot="input"
      onMouseDown={handleMouseDown}
      className={cn(
        "file:text-foreground placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground bg-background border-input text-foreground h-9 w-full min-w-0 rounded-lg border px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        "focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px]",
        "aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
        className
      )}
      {...props}
    />
  )
}

export { Input }
