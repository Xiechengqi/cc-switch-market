"use client";

import * as React from "react";
import { Switch as SwitchPrimitive } from "radix-ui";

import { cn } from "@/lib/cn";

function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        // memphis-theme: 2px slate-800 边胶囊；checked = emerald-400，unchecked = white
        "peer relative inline-flex h-7 w-12 shrink-0 cursor-pointer items-center rounded-full border-2 border-slate-800 transition-colors outline-none focus-visible:ring-2 focus-visible:ring-violet-500 focus-visible:ring-offset-2 focus-visible:ring-offset-white disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-emerald-400 data-[state=unchecked]:bg-white",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className={cn(
          // memphis-theme: thumb 是带 2px 黑边的小白圆，开/关位移 20px
          "pointer-events-none block h-4 w-4 translate-x-1 rounded-full border-2 border-slate-800 bg-white transition-transform data-[state=checked]:translate-x-[22px]"
        )}
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
