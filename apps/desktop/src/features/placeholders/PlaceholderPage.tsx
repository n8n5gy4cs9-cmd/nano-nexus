import { EmptyState } from "@/components/ui";
import type { NavSection } from "@/routes";

/** Stand-in until each section's milestone lands. */
export function PlaceholderPage({ section }: { section: NavSection }) {
  const Icon = section.icon;
  return (
    <div className="grid h-full place-items-center">
      <EmptyState icon={<Icon />} title={section.label} description={section.tagline} />
    </div>
  );
}
