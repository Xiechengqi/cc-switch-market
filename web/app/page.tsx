import { Shell } from "@/components/chrome";
import { HomeHero, MoneyFlow, FeatureGrid, PricingTable, HowToUse, FinalCTA } from "./home-ui";

export default function Home() {
  return (
    <Shell>
      <HomeHero />
      <MoneyFlow />
      <FeatureGrid />
      <PricingTable />
      <HowToUse />
      <FinalCTA />
    </Shell>
  );
}
