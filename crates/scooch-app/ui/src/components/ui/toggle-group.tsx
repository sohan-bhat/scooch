import * as React from "react"
import { ToggleGroup as ToggleGroupPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"

function ToggleGroup({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Root>) {
  return <ToggleGroupPrimitive.Root data-slot="toggle-group" className={cn("flex items-center gap-3", className)} {...props} />
}

function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  return (
    <ToggleGroupPrimitive.Item
      data-slot="toggle-group-item"
      className={cn(
        "relative shrink-0 cursor-pointer overflow-hidden rounded-field bg-room-3 shadow-[inset_0_0_0_1px_rgb(243_236_220/0.1)] outline-none transition-[transform,box-shadow,filter] duration-(--duration-hover) ease-(--ease-out) hover-fine:-translate-y-0.5 active:translate-y-px active:scale-[0.97] focus-visible:outline-2 focus-visible:outline-offset-3 focus-visible:outline-pad data-[state=on]:shadow-[0_0_0_2px_var(--color-room),0_0_0_4px_var(--color-paper)]",
        className
      )}
      {...props}
    />
  )
}

export { ToggleGroup, ToggleGroupItem }
