import { Shell } from "@/components/chrome";
import { ClaimRoot } from "./ui";

export default function ClaimPage() {
  return (
    <Shell>
      <section className="mx-auto max-w-6xl px-6 py-12 md:py-16">
        <ClaimRoot />
      </section>
    </Shell>
  );
}
