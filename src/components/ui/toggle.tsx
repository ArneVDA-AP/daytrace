import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"
import { Toggle as TogglePrimitive } from "radix-ui"

// Pressed is an ink fill (not a background tint only): it reads at a glance, meets 3:1 against
// the unpressed state, and in Windows Contrast themes it becomes Highlight / HighlightText.
const toggleVariants = cva(
  "inline-flex items-center justify-center gap-2 rounded-md font-medium whitespace-nowrap transition-[color,background-color,border-color,box-shadow] duration-(--dur-fast) hover:bg-surface-2 hover:text-foreground focus-visible:border-ring disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive data-[state=on]:border-primary data-[state=on]:bg-primary data-[state=on]:text-primary-foreground data-[state=on]:hover:bg-primary/90 data-[state=on]:hover:text-primary-foreground forced-colors:data-[state=on]:[forced-color-adjust:none] forced-colors:data-[state=on]:border-[Highlight] forced-colors:data-[state=on]:bg-[Highlight] forced-colors:data-[state=on]:text-[HighlightText] [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "bg-transparent",
        outline: "border border-input bg-surface-1 shadow-xs",
      },
      size: {
        default: "h-8 min-w-8 px-2.5 text-body",
        sm: "h-7 min-w-7 px-2 text-meta",
        lg: "h-9 min-w-9 px-3 text-body",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

function Toggle({
  className,
  variant,
  size,
  ...props
}: React.ComponentProps<typeof TogglePrimitive.Root> &
  VariantProps<typeof toggleVariants>) {
  return (
    <TogglePrimitive.Root
      data-slot="toggle"
      className={cn(toggleVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Toggle, toggleVariants }
