import { Shell } from "@/components/chrome";
import { AdminGuard } from "@/components/admin-guard";
import { AdminRoot } from "./ui";

export default function AdminPage() {
  return (
    <Shell>
      <section className="mx-auto max-w-6xl px-6 py-12 md:py-16">
        <AdminGuard>
          <AdminRoot />
        </AdminGuard>
      </section>
    </Shell>
  );
}
