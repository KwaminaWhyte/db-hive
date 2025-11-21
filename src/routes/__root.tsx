import { Outlet, createRootRouteWithContext } from "@tanstack/react-router";
import { TanStackRouterDevtools } from "@tanstack/router-devtools";
import { ThemeProvider, useTheme } from "@/components/theme-provider";
import { ConnectionProvider } from "@/contexts/ConnectionContext";
import { Toaster } from "sonner";
import { ConnectionProfile } from "@/types/database";

interface RouterContext {
  connectionId: string | null;
  connectionProfile: ConnectionProfile | undefined;
}

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

export const Route = createRootRouteWithContext<RouterContext>()({
  component: () => (
    <ThemeProvider defaultTheme="dark" storageKey="db-hive-theme">
      <ConnectionProvider>
        <RootComponent />
      </ConnectionProvider>
    </ThemeProvider>
  ),
});
