/**
 * ErrorStatesDemo - Visual showcase of all error and empty state components
 *
 * This page can be used for:
 * - Visual testing during development
 * - Design review and QA
 * - Component documentation
 * - Regression testing
 *
 * Access via: /error-states-demo (add to router if needed)
 */

import { useState } from "react";
import {
  ErrorState,
  ConnectionLostError,
  QueryErrorState,
  EmptyState,
  NoConnectionsEmpty,
  NoResultsEmpty,
} from "@/components";
import {
  AlertCircle,
  RefreshCw,
  Home,
  Database,
  FileSearch,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export const ErrorStatesDemo = () => {
  const [activeTab, setActiveTab] = useState("errors");

  return (
    <div className="container mx-auto p-8 space-y-8">
      {/* Header */}
      <div className="space-y-2">
        <h1 className="text-4xl font-bold tracking-tight">
          Error & Empty States Demo
        </h1>
        <p className="text-muted-foreground text-lg">
          Visual showcase of all error and empty state components (Milestone 3.9)
        </p>
      </div>

      {/* Tabs */}
      <Tabs value={activeTab} onValueChange={setActiveTab}>
        <TabsList className="grid w-full grid-cols-2 max-w-md">
          <TabsTrigger value="errors">Error States</TabsTrigger>
          <TabsTrigger value="empty">Empty States</TabsTrigger>
        </TabsList>

        {/* Error States Tab */}
        <TabsContent value="errors" className="space-y-8 mt-8">
          {/* 1. Basic Error State */}
          <DemoSection
            title="1. Basic Error State"
            description="General purpose error component with custom icon and actions"
          >
            <ErrorState
              title="Operation Failed"
              message="We encountered an error while processing your request. Please try again or contact support if the issue persists."
              icon={AlertCircle}
              actions={[
                {
                  label: "Retry",
                  onClick: () => alert("Retry clicked"),
                  icon: RefreshCw,
                },
                {
                  label: "Go Home",
                  onClick: () => alert("Go Home clicked"),
                  variant: "outline",
                  icon: Home,
                },
              ]}
            />
          </DemoSection>

          {/* 2. Error Variants */}
          <DemoSection
            title="2. Error Variants"
            description="Different visual styles for different severity levels"
          >
            <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
              <div>
                <h4 className="text-sm font-semibold mb-4 text-center">
                  Error (Red)
                </h4>
                <ErrorState
                  title="Critical Error"
                  message="A critical error occurred."
                  icon={AlertCircle}
                  variant="error"
                />
              </div>
              <div>
                <h4 className="text-sm font-semibold mb-4 text-center">
                  Warning (Orange)
                </h4>
                <ErrorState
                  title="Warning"
                  message="This action may have consequences."
                  icon={AlertCircle}
                  variant="warning"
                />
              </div>
              <div>
                <h4 className="text-sm font-semibold mb-4 text-center">
                  Info (Blue)
                </h4>
                <ErrorState
                  title="Information"
                  message="Here's some useful information."
                  icon={AlertCircle}
                  variant="info"
                />
              </div>
            </div>
          </DemoSection>

          {/* 3. Connection Lost Error */}
          <DemoSection
            title="3. Connection Lost Error"
            description="Specialized component for database connection failures"
          >
            <ConnectionLostError
              databaseName="PostgreSQL Production"
              onReconnect={() => alert("Reconnecting...")}
              onGoToDashboard={() => alert("Going to dashboard...")}
            />
          </DemoSection>

          {/* 4. Connection Lost Error - Custom Message */}
          <DemoSection
            title="4. Connection Lost Error - Custom Message"
            description="With custom error message for maintenance scenarios"
          >
            <ConnectionLostError
              message="The database server is currently undergoing scheduled maintenance. Please try again in 15 minutes."
              onReconnect={() => alert("Reconnecting...")}
            />
          </DemoSection>

          {/* 5. Query Error State */}
          <DemoSection
            title="5. Query Error State"
            description="Detailed query execution error with syntax highlighting"
          >
            <QueryErrorState
              message='column "usre_name" does not exist'
              errorCode="42703"
              query="SELECT id, usre_name, email FROM users WHERE status = 'active';"
              onRetry={() => alert("Retrying query...")}
              onViewDocs={() =>
                alert("Opening documentation... (would open in new tab)")
              }
            />
          </DemoSection>

          {/* 6. Query Syntax Error */}
          <DemoSection
            title="6. Query Syntax Error"
            description="SQL syntax error with error code"
          >
            <QueryErrorState
              message='syntax error at or near "SELCT"'
              errorCode="42601"
              query="SELCT * FROM users;"
              onRetry={() => alert("Retrying...")}
            />
          </DemoSection>

          {/* 7. Query Error - Long Query */}
          <DemoSection
            title="7. Query Error - Complex Query"
            description="Error with a longer, multi-line query"
          >
            <QueryErrorState
              message="Permission denied for relation users"
              errorCode="42501"
              query={`SELECT
  u.id,
  u.username,
  u.email,
  COUNT(o.id) AS order_count
FROM
  users u
  LEFT JOIN orders o ON u.id = o.user_id
WHERE
  u.status = 'active'
GROUP BY
  u.id, u.username, u.email
LIMIT 100;`}
              onRetry={() => alert("Retrying...")}
              showDetailsInitially={true}
            />
          </DemoSection>
        </TabsContent>

        {/* Empty States Tab */}
        <TabsContent value="empty" className="space-y-8 mt-8">
          {/* 1. No Connections Empty */}
          <DemoSection
            title="1. No Connections Empty"
            description="Shown when user has no database connections configured"
          >
            <NoConnectionsEmpty
              onAddConnection={() => alert("Opening connection form...")}
            />
          </DemoSection>

          {/* 2. No Connections - Another Example */}
          <DemoSection
            title="2. No Connections - Another Example"
            description="Simple usage with action handler"
          >
            <NoConnectionsEmpty
              onAddConnection={() => alert("Adding connection...")}
            />
          </DemoSection>

          {/* 3. No Results - Basic */}
          <DemoSection
            title="3. No Results - Basic"
            description="Shown when query returns no results"
          >
            <NoResultsEmpty
              onRunQuery={() => alert("Running query...")}
            />
          </DemoSection>

          {/* 4. No Results - With Query Text */}
          <DemoSection
            title="4. No Results - With Query Context"
            description="Shows contextual message based on the executed query"
          >
            <NoResultsEmpty
              queryText="SELECT * FROM users WHERE status = 'active'"
              onRunQuery={() => alert("Running another query...")}
            />
          </DemoSection>

          {/* 5. Generic Empty State */}
          <DemoSection
            title="5. Generic Empty State"
            description="Base empty state component with custom icon"
          >
            <EmptyState
              title="No Data Available"
              message="There's nothing to display here yet. Get started by adding some data."
              icon={Database}
              size="md"
              actions={[
                {
                  label: "Add Data",
                  onClick: () => alert("Adding data..."),
                },
              ]}
            />
          </DemoSection>

          {/* 6. Empty State Sizes */}
          <DemoSection
            title="6. Empty State Sizes"
            description="Different size variants for different contexts"
          >
            <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
              <div>
                <h4 className="text-sm font-semibold mb-4 text-center">
                  Small
                </h4>
                <EmptyState
                  title="No History"
                  message="Your history is empty."
                  icon={FileSearch}
                  size="sm"
                />
              </div>
              <div>
                <h4 className="text-sm font-semibold mb-4 text-center">
                  Medium (Default)
                </h4>
                <EmptyState
                  title="No Data"
                  message="There's no data to display."
                  icon={Database}
                  size="md"
                />
              </div>
              <div>
                <h4 className="text-sm font-semibold mb-4 text-center">
                  Large
                </h4>
                <EmptyState
                  title="Get Started"
                  message="Begin by creating your first item."
                  icon={Database}
                  size="lg"
                />
              </div>
            </div>
          </DemoSection>
        </TabsContent>
      </Tabs>

      {/* Footer */}
      <Card className="bg-muted/50">
        <CardContent className="pt-6">
          <div className="space-y-4">
            <h3 className="font-semibold text-lg">Developer Notes</h3>
            <ul className="space-y-2 text-sm text-muted-foreground">
              <li>
                • All components support dark/light theme and are fully responsive
              </li>
              <li>
                • Animations use CSS transitions for smooth appearance
              </li>
              <li>• Action buttons are optional and can be customized</li>
              <li>• See ErrorStateExamples.tsx for code examples</li>
              <li>
                • See ERROR_AND_EMPTY_STATES.md for complete documentation
              </li>
            </ul>
          </div>
        </CardContent>
      </Card>
    </div>
  );
};

// Helper component for demo sections
const DemoSection = ({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) => {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-xl">{title}</CardTitle>
        <p className="text-sm text-muted-foreground">{description}</p>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
};
