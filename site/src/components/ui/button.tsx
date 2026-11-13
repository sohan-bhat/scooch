import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { Slot } from "radix-ui"
import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "relative inline-flex shrink-0 cursor-pointer items-center justify-center gap-2.5 font-semibold whitespace-nowrap select-none outline-none transition-[transform,background-color,box-shadow,color] duration-(--duration-hover) ease-(--ease-out) active:duration-(--duration-press) disabled:pointer-events-none disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-pad [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        pill: "rounded-pill bg-lamp text-ink shadow-[inset_0_1px_0_rgb(255_235_210/0.35),inset_0_-2px_0_rgb(20_17_14/0.25)] hover-fine:bg-lamp-hot hover-fine:-translate-y-0.5 hover-fine:shadow-[inset_0_1px_0_rgb(255_235_210/0.42),inset_0_-2px_0_rgb(20_17_14/0.25)] active:translate-y-px active:scale-[0.97] active:bg-lamp active:shadow-[inset_0_2px_0_rgb(20_17_14/0.25)]",
        quiet: "rounded-field bg-room-3 text-paper shadow-[inset_0_0_0_1px_rgb(243_236_220/0.12)] hover-fine:bg-steel hover-fine:shadow-[inset_0_0_0_1px_rgb(243_236_220/0.22)] active:scale-[0.95] active:bg-room-2",
      },
      size: {
        sm: "h-10 px-5 text-[0.9375rem] [&_svg]:size-[1.1em]",
        md: "h-12 px-6 text-base [&_svg]:size-[1.15em]",
        lg: "h-14 px-8 text-[1.0625rem] [&_svg]:size-[1.2em]",
        icon: "size-11 [&_svg]:size-5",
      },
    },
    defaultVariants: {
      variant: "pill",
      size: "lg",
    },
  }
)

function Button({
  className,
  variant = "pill",
  size = "lg",
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean
  }) {
  const Comp = asChild ? Slot.Root : "button"

  return (
    <Comp
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
}

export { Button, buttonVariants }
