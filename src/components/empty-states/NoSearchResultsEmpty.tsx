import { FC } from "react";
import { SearchX, X } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface NoSearchResultsEmptyProps {
  onClearSearch: () => void;
  searchQuery?: string;
  className?: string;
}

export const NoSearchResultsEmpty: FC<NoSearchResultsEmptyProps> = ({
  onClearSearch,
  searchQuery,
  className,
}) => {
  const actions: EmptyStateAction[] = [
    {
      label: "Clear Search",
      onClick: onClearSearch,
      icon: X,
      variant: "outline",
    },
  ];

  const message = searchQuery
    ? `No results found for "${searchQuery}". Try adjusting your search terms or filters.`
    : "No results found. Try adjusting your search terms or filters.";

  return (
    <EmptyState
      icon={SearchX}
      title="No Results Found"
      message={message}
      actions={actions}
      className={className}
      iconClassName="bg-amber-50 dark:bg-amber-950/30 text-amber-600 dark:text-amber-400"
      size="sm"
    />
  );
};
