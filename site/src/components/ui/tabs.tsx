import * as React from "react"
import { Tabs as TabsPrimitive } from "radix-ui"
import { cn } from "@/lib/utils"

function Tabs({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Root>) {
  return <TabsPrimitive.Root data-slot="tabs" className={cn("flex flex-col gap-4", className)} {...props} />
}

function TabsList({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn("inline-flex w-fit items-center gap-1 rounded-pill bg-room-2 p-1 shadow-[inset_0_0_0_1px_rgb(243_236_220/0.08)]", className)}
      {...props}
    />
  )
}

function TabsTrigger({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      data-slot="tabs-trigger"
      className={cn(
        "inline-flex h-10 cursor-pointer items-center justify-center rounded-pill px-5 text-[0.9375rem] font-semibold whitespace-nowrap text-plastic transition-[color,background-color,box-shadow,transform] duration-(--duration-hover) ease-(--ease-out) outline-none hover-fine:text-paper active:scale-[0.96] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-pad data-[state=active]:bg-paper data-[state=active]:text-ink data-[state=active]:shadow-[0_1px_0_rgb(255_255_255/0.4)_inset,0_6px_14px_-8px_rgb(0_0_0/0.7)]",
        className
      )}
      {...props}
    />
  )
}

function TabsContent({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Content>) {
  return <TabsPrimitive.Content data-slot="tabs-content" className={cn("flex-1 outline-none", className)} {...props} />
}

export { Tabs, TabsList, TabsTrigger, TabsContent }
