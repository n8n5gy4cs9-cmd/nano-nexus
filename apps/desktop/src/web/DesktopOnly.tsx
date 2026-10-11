import { Monitor } from "lucide-react";

import { EmptyState } from "@/components/ui";

/** Shown in the web build for features that only work on the computer running NanoNexus. */
export function DesktopOnly({ title }: { title: string }) {
  return (
    <div className="grid h-full place-items-center px-4">
      <EmptyState
        icon={<Monitor />}
        title={`${title} is desktop-only`}
        description="Open NanoNexus on your computer for this."
      />
    </div>
  );
}
