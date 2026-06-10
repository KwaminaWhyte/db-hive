import { FC } from "react";
import { Inbox, Plus } from "lucide-react";
import { EmptyState, EmptyStateAction } from "../EmptyState";

export interface NoDataEmptyProps {
  onAddData?: () => void;
  tableName?: string;
  className?: string;
}

export const NoDataEmpty: FC<NoDataEmptyProps> = ({
  onAddData,
  tableName,
  className,
}) => {
  const actions: EmptyStateAction[] = onAddData
    ? [
        {
          label: "Insert Data",
          onClick: onAddData,
          icon: Plus,
          variant: "default",
        },
      ]
    : [];

  const message = tableName
    ? `The table "${tableName}" is empty. Insert your first row to get started.`
    : "This table is empty. Insert your first row to get started.";

  return (
    <EmptyState
      icon={Inbox}
      title="No Data"
      message={message}
      actions={actions}
      className={className}
      iconClassName="bg-primary/10 text-primary"
    />
  );
};
