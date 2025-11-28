import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ArrowLeft, Package, Store } from "lucide-react";
import { PluginMarketplace } from "@/components/PluginMarketplace";
import { InstalledPlugins } from "@/components/InstalledPlugins";

export const Route = createFileRoute("/plugins")({
  component: PluginsRoute,
});

function PluginsRoute() {
  const navigate = useNavigate({ from: "/plugins" });
  const [activeTab, setActiveTab] = useState<"marketplace" | "installed">("marketplace");

  const handleBack = () => {
    navigate({ to: "/" });
  };

  return (
    <div className="flex-1 h-full flex flex-col">
      {/* Header with Back Button */}
      <div className="border-b px-6 py-4 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <Button
            variant="outline"
            size="sm"
            onClick={handleBack}
          >
            <ArrowLeft className="size-4 mr-2" />
            Back
          </Button>
          <div>
            <h1 className="text-2xl font-semibold">Plugin Manager</h1>
            <p className="text-sm text-muted-foreground">
              Extend DB-Hive with plugins
            </p>
          </div>
        </div>
      </div>

      {/* Plugin Tabs */}
      <Tabs
        value={activeTab}
        onValueChange={(value) => setActiveTab(value as "marketplace" | "installed")}
        className="flex-1 flex flex-col"
      >
        <TabsList className="px-6 py-2 justify-start">
          <TabsTrigger value="marketplace" className="gap-2">
            <Store className="size-4" />
            Marketplace
          </TabsTrigger>
          <TabsTrigger value="installed" className="gap-2">
            <Package className="size-4" />
            Installed
          </TabsTrigger>
        </TabsList>

        <TabsContent value="marketplace" className="flex-1 m-0">
          <PluginMarketplace />
        </TabsContent>

        <TabsContent value="installed" className="flex-1 m-0">
          <InstalledPlugins />
        </TabsContent>
      </Tabs>
    </div>
  );
}