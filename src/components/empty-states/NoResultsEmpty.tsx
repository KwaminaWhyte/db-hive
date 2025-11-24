import { FC } from "react";
import { FileQuestion, Play } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface NoResultsEmptyProps {
  onRunQuery?: () => void;
  queryText?: string;
  className?: string;
}

export const NoResultsEmpty: FC<NoResultsEmptyProps> = ({
  onRunQuery,
  queryText,
  className,
}) => {
  const actions: EmptyStateAction[] = onRunQuery
    ? [
        {
          label: "Run Another Query",
          onClick: onRunQuery,
          icon: Play,
          variant: "outline",
        },
      ]
    : [];

  const message = queryText
    ? `The query returned no results. The data you're looking for might not exist or the query conditions might be too restrictive.`
    : "The query returned no results. Try adjusting your query or checking the data.";

  return (
    <EmptyState
      icon={FileQuestion}
      title="No Results"
      message={message}
      actions={actions}
      className={className}
      iconClassName="bg-slate-50 dark:bg-slate-950/30 text-slate-600 dark:text-slate-400"
      size="sm"
    />
  );
};
