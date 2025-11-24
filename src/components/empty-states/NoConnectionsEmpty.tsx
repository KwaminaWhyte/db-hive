import { FC } from "react";
import { Database, Plus } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface NoConnectionsEmptyProps {
  onAddConnection: () => void;
  className?: string;
}

export const NoConnectionsEmpty: FC<NoConnectionsEmptyProps> = ({
  onAddConnection,
  className,
}) => {
  const actions: EmptyStateAction[] = [
    {
      label: "Add Connection",
      onClick: onAddConnection,
      icon: Plus,
      variant: "default",
    },
  ];

  return (
    <EmptyState
      icon={Database}
      title="No Connections Yet"
      message="Get started by creating your first database connection. Connect to PostgreSQL, MySQL, SQLite, and more."
      actions={actions}
      className={className}
      iconClassName="bg-blue-50 dark:bg-blue-950/30 text-blue-600 dark:text-blue-400"
    />
  );
};
