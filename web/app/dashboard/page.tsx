import { Shell } from "@/components/chrome";
import { DashboardRoot } from "./ui";

export default function DashboardPage() {
  return (
    <Shell>
      <section className="mx-auto max-w-6xl px-6 py-12 md:py-16">
        <DashboardRoot />
      </section>
    </Shell>
  );
}
