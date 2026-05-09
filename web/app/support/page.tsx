import { Shell } from "@/components/chrome";
import { SupportRoot } from "./ui";

export default function SupportPage() {
  return (
    <Shell>
      <section className="mx-auto max-w-6xl px-6 py-12 md:py-16">
        <SupportRoot />
      </section>
    </Shell>
  );
}
