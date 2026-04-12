import { CouncilChamberPanel } from "@/app/components/console/CouncilChamberPanel";

export const metadata = { title: "Council chamber" };

export default function CouncilPage() {
  return (
    <div className="flex h-full flex-col">
      <CouncilChamberPanel />
    </div>
  );
}
