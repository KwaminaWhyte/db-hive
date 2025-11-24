import { FC } from "react";
import { History, Play } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface NoHistoryEmptyProps {
  onRunQuery?: () => void;
  className?: string;
}

export const NoHistoryEmpty: FC<NoHistoryEmptyProps> = ({
  onRunQuery,
  className,
}) => {
  const actions: EmptyStateAction[] = onRunQuery
    ? [
        {
          label: "Run Your First Query",
          onClick: onRunQuery,
          icon: Play,
          variant: "default",
        },
      ]
    : [];

  return (
    <EmptyState
      icon={History}
      title="No Query History"
      message="Your executed queries will appear here. Run a query to get started and build your history."
      actions={actions}
      className={className}
      iconClassName="bg-violet-50 dark:bg-violet-950/30 text-violet-600 dark:text-violet-400"
    />
  );
};
