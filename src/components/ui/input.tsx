import * as React from "react"

import { cn } from "@/lib/utils"

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "file:text-slate-100 placeholder:text-slate-500 selection:bg-amber-400 selection:text-slate-950 bg-slate-950/60 border-slate-700/60 text-slate-100 h-9 w-full min-w-0 rounded-lg border px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        "focus-visible:border-amber-300/60 focus-visible:ring-amber-300/30 focus-visible:ring-[3px]",
        "aria-invalid:ring-red-400/20 aria-invalid:border-red-400",
        "dark:bg-slate-950/60 dark:border-slate-700/60 dark:text-slate-100 dark:placeholder:text-slate-500",
        className
      )}
      {...props}
    />
  )
}

export { Input }
