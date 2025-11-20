import { FC, useState, useRef, useEffect, KeyboardEvent } from 'react';
import { Input } from './ui/input';
import { cn } from '@/lib/utils';

export interface CellChange {
  rowIndex: number;
  columnName: string;
  columnIndex: number;
  oldValue: any;
  newValue: any;
}

interface EditableCellProps {
  /** Current cell value */
  value: any;

  /** Row index (0-based within current page) */
  rowIndex: number;

  /** Column name */
  columnName: string;

  /** Column index */
  columnIndex: number;

  /** Whether this cell is currently being edited */
  isEditing: boolean;

  /** Whether this cell has been modified */
  isModified: boolean;

  /** Column data type */
  dataType: string;

  /** Whether column accepts NULL */
  nullable: boolean;

  /** Callback when cell edit starts */
  onStartEdit: (rowIndex: number, columnIndex: number) => void;

  /** Callback when cell value changes */
  onChange: (change: CellChange) => void;

  /** Callback when cell edit is cancelled */
  onCancelEdit: () => void;

  /** Callback when cell is clicked (for copy functionality) */
  onClick?: () => void;
}

export const EditableCell: FC<EditableCellProps> = ({
  value,
  rowIndex,
  columnName,
  columnIndex,
  isEditing,
  isModified,
  dataType,
  nullable,
  onStartEdit,
  onChange,
  onCancelEdit,
  onClick,
}) => {
  const [editValue, setEditValue] = useState<string>('');
  const inputRef = useRef<HTMLInputElement>(null);

  // Initialize edit value when entering edit mode
  useEffect(() => {
    if (isEditing) {
      const initialValue = value === null || value === undefined ? '' : String(value);
      setEditValue(initialValue);
      // Focus input after a brief delay to ensure it's rendered
      setTimeout(() => inputRef.current?.focus(), 10);
    }
  }, [isEditing, value]);

  const handleDoubleClick = (e: React.MouseEvent) => {
    e.stopPropagation(); // Prevent event from bubbling to parent elements
    if (!isEditing) {
      onStartEdit(rowIndex, columnIndex);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleCommit();
    } else if (e.key === 'Escape') {
      onCancelEdit();
    }
  };

  const handleBlur = () => {
    if (isEditing) {
      handleCommit();
    }
  };

  const handleCommit = () => {
    // Convert empty string to NULL if column is nullable
    let newValue: any = editValue;

    if (editValue === '' && nullable) {
      newValue = null;
    } else if (editValue !== '') {
      // Type conversion based on data type
      const lowerDataType = dataType.toLowerCase();

      if (lowerDataType.includes('int') || lowerDataType.includes('serial')) {
        newValue = parseInt(editValue, 10);
        if (isNaN(newValue)) {
          // Invalid number, keep as string and let backend handle error
          newValue = editValue;
        }
      } else if (lowerDataType.includes('float') || lowerDataType.includes('double') ||
                 lowerDataType.includes('decimal') || lowerDataType.includes('numeric')) {
        newValue = parseFloat(editValue);
        if (isNaN(newValue)) {
          newValue = editValue;
        }
      } else if (lowerDataType.includes('bool')) {
        newValue = editValue.toLowerCase() === 'true' || editValue === '1';
      }
      // For other types (text, varchar, date, etc.), keep as string
    }

    // Only trigger change if value actually changed
    if (newValue !== value) {
      onChange({
        rowIndex,
        columnName,
        columnIndex,
        oldValue: value,
        newValue,
      });
    }

    onCancelEdit();
  };

  // Render edit mode
  if (isEditing) {
    return (
      <div className="p-0 m-0">
        <Input
          ref={inputRef}
          type="text"
          value={editValue}
          onChange={(e) => setEditValue(e.target.value)}
          onKeyDown={handleKeyDown}
          onBlur={handleBlur}
          className="h-8 border-2 border-primary font-mono text-sm px-2"
          placeholder={nullable ? "(NULL)" : ""}
        />
      </div>
    );
  }

  // Render display mode
  const cellString = value === null || value === undefined ? 'NULL' :
                     typeof value === "object" ? JSON.stringify(value) :
                     String(value);
  const isTruncated = cellString.length > 100;
  const displayValue = isTruncated ? cellString.substring(0, 100) + '...' : cellString;

  return (
    <div
      className={cn(
        "truncate cursor-pointer p-2",
        isModified && "bg-yellow-500/20 border-l-2 border-yellow-500"
      )}
      onClick={onClick}
      onDoubleClick={handleDoubleClick}
      title={isTruncated ? `${cellString}\n\nDouble-click to edit • Click to copy` : "Double-click to edit • Click to copy"}
    >
      {value === null || value === undefined ? (
        <span className="italic text-muted-foreground opacity-50">
          NULL
        </span>
      ) : typeof value === "object" ? (
        <code className="text-xs bg-blue-500/10 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
          {displayValue}
        </code>
      ) : typeof value === "boolean" ? (
        <span className={value ? "text-green-600 dark:text-green-400" : "text-red-600 dark:text-red-400"}>
          {String(value)}
        </span>
      ) : typeof value === "number" ? (
        <span className="text-blue-600 dark:text-blue-400">
          {value.toLocaleString()}
        </span>
      ) : value === "" ? (
        <span className="italic text-muted-foreground opacity-50">
          (empty)
        </span>
      ) : (
        <span className="text-foreground">{displayValue}</span>
      )}
    </div>
  );
};
