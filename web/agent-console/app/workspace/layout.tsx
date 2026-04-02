import { WorkspaceProvider } from "@/app/context/WorkspaceContext";
import { ConsoleAppShell } from "@/app/components/console/ConsoleAppShell";

export default function WorkspaceLayout({ children }: { children: React.ReactNode }) {
  return (
    <WorkspaceProvider>
      <ConsoleAppShell>{children}</ConsoleAppShell>
    </WorkspaceProvider>
  );
}
