import * as React from "react"
import { Switch as SwitchPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"

function Switch({ className, ...props }: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        "peer relative inline-flex h-7 w-12 shrink-0 cursor-pointer items-center rounded-pill bg-room-3 p-1 shadow-[inset_0_0_0_1px_rgb(243_236_220/0.16),inset_0_2px_3px_rgb(0_0_0/0.35)] outline-none transition-[background-color,box-shadow] duration-(--duration-hover) ease-(--ease-out) hover-fine:shadow-[inset_0_0_0_1px_rgb(243_236_220/0.3),inset_0_2px_3px_rgb(0_0_0/0.35)] focus-visible:outline-2 focus-visible:outline-offset-3 focus-visible:outline-pad disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-paper data-[state=checked]:shadow-[inset_0_0_0_1px_rgb(255_255_255/0.4),inset_0_-2px_0_rgb(20_17_14/0.12)]",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block size-5 rounded-pill bg-plastic shadow-[0_1px_2px_rgb(0_0_0/0.5),inset_0_1px_0_rgb(255_255_255/0.5)] transition-[transform,background-color] duration-(--duration-hover) ease-(--ease-out) data-[state=checked]:translate-x-5 data-[state=checked]:bg-ink data-[state=checked]:shadow-[0_1px_2px_rgb(0_0_0/0.25),inset_0_1px_0_rgb(255_255_255/0.12)]"
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
