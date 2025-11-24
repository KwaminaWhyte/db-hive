import { FC } from "react";
import { Table, Plus } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface NoTablesEmptyProps {
  onCreateTable?: () => void;
  databaseName?: string;
  className?: string;
}

export const NoTablesEmpty: FC<NoTablesEmptyProps> = ({
  onCreateTable,
  databaseName,
  className,
}) => {
  const actions: EmptyStateAction[] = onCreateTable
    ? [
        {
          label: "Create Table",
          onClick: onCreateTable,
          icon: Plus,
          variant: "default",
        },
      ]
    : [];

  const message = databaseName
    ? `The database "${databaseName}" doesn't have any tables yet. Create your first table to get started.`
    : "This database doesn't have any tables yet. Create your first table to get started.";

  return (
    <EmptyState
      icon={Table}
      title="No Tables Found"
      message={message}
      actions={actions}
      className={className}
      iconClassName="bg-emerald-50 dark:bg-emerald-950/30 text-emerald-600 dark:text-emerald-400"
    />
  );
};
