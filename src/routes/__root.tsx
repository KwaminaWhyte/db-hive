import { Outlet, createRootRoute } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/router-devtools";
import { ThemeProvider, useTheme } from "@/components/theme-provider";
import { ConnectionProvider, useConnectionContext } from "@/contexts/ConnectionContext";
import { TabProvider } from "@/contexts/TabContext";
import { Toaster } from "sonner";

function RootComponent() {
  const { theme } = useTheme();

  return (
    <div className="flex h-screen w-full bg-background">
      <Outlet />
      <Toaster
        richColors
        position="bottom-right"
        theme={theme === "system" ? undefined : (theme as "light" | "dark")}
      />
      {import.meta.env.DEV && <TanStackRouterDevtools />}
    </div>
  );
}

function WithTabProvider() {
  const { connectionId, currentDatabase } = useConnectionContext();

  return (
    <TabProvider connectionId={connectionId} currentDatabase={currentDatabase}>
      <RootComponent />
    </TabProvider>
  );
}

export const Route = createRootRoute({
  component: () => (
    <ThemeProvider defaultTheme="dark" storageKey="db-hive-theme">
      <ConnectionProvider>
        <WithTabProvider />
      </ConnectionProvider>
    </ThemeProvider>
  ),
});
