import { FC, useState } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import {
  NoConnectionsEmpty,
  NoHistoryEmpty,
  NoTablesEmpty,
  NoSearchResultsEmpty,
  NoResultsEmpty,
  NoDataEmpty,
} from "./index";

/**
 * EmptyStateShowcase - A demo component to showcase all empty state variants
 * This can be used for testing, documentation, or as a style guide reference
 */
export const EmptyStateShowcase: FC = () => {
  const [activeTab, setActiveTab] = useState("connections");

  const handleAction = (action: string) => {
    console.log(`Action triggered: ${action}`);
    alert(`Action: ${action}`);
  };

  return (
    <div className="container mx-auto p-8 space-y-8">
      <div className="space-y-2">
        <h1 className="text-3xl font-bold tracking-tight">Empty State Components</h1>
        <p className="text-muted-foreground">
          A showcase of all empty state components used throughout DB-Hive
        </p>
      </div>

      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList className="grid w-full grid-cols-3 lg:grid-cols-6">
          <TabsTrigger value="connections">Connections</TabsTrigger>
          <TabsTrigger value="history">History</TabsTrigger>
          <TabsTrigger value="tables">Tables</TabsTrigger>
          <TabsTrigger value="search">Search</TabsTrigger>
          <TabsTrigger value="results">Results</TabsTrigger>
          <TabsTrigger value="data">Data</TabsTrigger>
        </TabsList>

        <TabsContent value="connections" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>No Connections Empty State</CardTitle>
              <CardDescription>
                Displayed when the user has no saved database connections
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoConnectionsEmpty
                onAddConnection={() => handleAction("Add Connection")}
              />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="history" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>No History Empty State</CardTitle>
              <CardDescription>
                Displayed when query history is empty
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoHistoryEmpty
                onRunQuery={() => handleAction("Run First Query")}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>No History (Without Action)</CardTitle>
              <CardDescription>
                Variant without the action button
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoHistoryEmpty />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="tables" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>No Tables Empty State</CardTitle>
              <CardDescription>
                Displayed when a database has no tables
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoTablesEmpty
                databaseName="production_db"
                onCreateTable={() => handleAction("Create Table")}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>No Tables (Generic)</CardTitle>
              <CardDescription>
                Without database name specified
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoTablesEmpty />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="search" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>No Search Results Empty State</CardTitle>
              <CardDescription>
                Displayed when search returns no results (smaller variant)
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoSearchResultsEmpty
                searchQuery="test query"
                onClearSearch={() => handleAction("Clear Search")}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>No Search Results (Generic)</CardTitle>
              <CardDescription>
                Without search query specified
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoSearchResultsEmpty
                onClearSearch={() => handleAction("Clear Search")}
              />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="results" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>No Results Empty State</CardTitle>
              <CardDescription>
                Displayed when a query returns 0 rows (smaller variant)
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoResultsEmpty
                queryText="SELECT * FROM users WHERE status = 'inactive'"
                onRunQuery={() => handleAction("Run Another Query")}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>No Results (Without Action)</CardTitle>
              <CardDescription>
                Variant without the action button
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoResultsEmpty />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="data" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>No Data Empty State</CardTitle>
              <CardDescription>
                Displayed when a table exists but has no rows
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoDataEmpty
                tableName="users"
                onAddData={() => handleAction("Insert Data")}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>No Data (Generic)</CardTitle>
              <CardDescription>
                Without table name specified
              </CardDescription>
            </CardHeader>
            <CardContent className="min-h-[400px] flex items-center justify-center">
              <NoDataEmpty />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      {/* Size Variants Demo */}
      <Card>
        <CardHeader>
          <CardTitle>Size Variants</CardTitle>
          <CardDescription>
            All empty states support three size variants: sm, md (default), and lg
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-8">
          <div className="space-y-2">
            <h3 className="text-sm font-medium text-muted-foreground">Small (sm)</h3>
            <div className="border rounded-lg p-4">
              <NoSearchResultsEmpty
                searchQuery="example"
                onClearSearch={() => handleAction("Clear")}
              />
            </div>
          </div>

          <div className="space-y-2">
            <h3 className="text-sm font-medium text-muted-foreground">
              Medium (md) - Default
            </h3>
            <div className="border rounded-lg p-4">
              <NoConnectionsEmpty
                onAddConnection={() => handleAction("Add")}
              />
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Implementation Notes */}
      <Card>
        <CardHeader>
          <CardTitle>Implementation Notes</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <h4 className="font-medium">Features</h4>
            <ul className="list-disc list-inside space-y-1 text-sm text-muted-foreground">
              <li>Smooth entrance animations (fade-in, slide-up, zoom-in)</li>
              <li>Color-coded icons for visual recognition</li>
              <li>Responsive layout with proper spacing</li>
              <li>Accessible with semantic HTML</li>
              <li>Hover effects on icons and buttons</li>
              <li>Dark mode support</li>
            </ul>
          </div>

          <div className="space-y-2">
            <h4 className="font-medium">Usage</h4>
            <pre className="bg-muted p-4 rounded-lg text-xs overflow-auto">
              {`import { NoConnectionsEmpty } from "@/components/empty-states";

<NoConnectionsEmpty
  onAddConnection={() => setShowDialog(true)}
/>`}
            </pre>
          </div>
        </CardContent>
      </Card>
    </div>
  );
};
