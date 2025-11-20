import { FC, useRef, useEffect } from 'react';
import Editor, { Monaco } from '@monaco-editor/react';
import type * as monaco from 'monaco-editor';
import { useTheme } from './theme-provider';
import { Card, CardContent, CardHeader } from './ui/card';
import { Button } from './ui/button';
import { Play, Trash2, CircleDot, RefreshCw } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useAutocompleteMetadata } from '@/hooks/useAutocompleteMetadata';
import { registerSqlAutocomplete } from '@/lib/sqlAutocomplete';

interface SQLEditorProps {
  /** Active connection ID */
  connectionId: string | null;

  /** Active database name */
  database: string | null;

  /** Callback when query is executed */
  onExecuteQuery: (sql: string) => void;

  /** Current SQL text */
  value: string;

  /** Text change handler */
  onChange: (value: string | undefined) => void;

  /** Whether query is currently executing */
  loading?: boolean;
}

export const SQLEditor: FC<SQLEditorProps> = ({
  connectionId,
  database,
  onExecuteQuery,
  value,
  onChange,
  loading = false,
}) => {
  const { theme } = useTheme();
  const editorRef = useRef<any>(null);
  const autocompleteDisposableRef = useRef<monaco.IDisposable | null>(null);

  // Fetch autocomplete metadata
  const { metadata, loading: metadataLoading, refresh } = useAutocompleteMetadata({
    connectionId,
    database,
    enabled: !!connectionId && !!database,
  });

  // Determine Monaco theme based on current theme
  const getMonacoTheme = () => {
    if (theme === 'dark') return 'vs-dark';
    if (theme === 'light') return 'vs';
    // For system theme, check if dark mode is active
    const isDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    return isDark ? 'vs-dark' : 'vs';
  };

  // Handle editor mount
  const handleEditorDidMount = (editor: any, monaco: Monaco) => {
    editorRef.current = editor;

    // Add Ctrl+Enter keyboard shortcut
    editor.addAction({
      id: 'execute-query',
      label: 'Execute Query',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
      run: () => {
        const sql = editor.getValue();
        if (sql.trim() && connectionId && !loading) {
          onExecuteQuery(sql);
        }
      },
    });
  };

  // Handle execute button click
  const handleExecute = () => {
    if (value.trim() && connectionId && !loading) {
      onExecuteQuery(value);
    }
  };

  // Handle clear button click
  const handleClear = () => {
    onChange('');
    editorRef.current?.focus();
  };

  // Update theme when it changes
  useEffect(() => {
    if (editorRef.current) {
      const monaco = (window as any).monaco;
      if (monaco) {
        monaco.editor.setTheme(getMonacoTheme());
      }
    }
  }, [theme]);

  // Register autocomplete provider when metadata changes
  useEffect(() => {
    const monacoInstance = (window as any).monaco;
    if (!monacoInstance) return;

    // Dispose previous provider
    if (autocompleteDisposableRef.current) {
      autocompleteDisposableRef.current.dispose();
    }

    // Register new provider with current metadata
    autocompleteDisposableRef.current = registerSqlAutocomplete(monacoInstance, metadata);

    // Cleanup on unmount
    return () => {
      if (autocompleteDisposableRef.current) {
        autocompleteDisposableRef.current.dispose();
      }
    };
  }, [metadata]);

  return (
    <Card className="h-full flex flex-col">
      <CardHeader className="pb-3 border-b">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Button
              variant="default"
              size="sm"
              onClick={handleExecute}
              disabled={!connectionId || !value.trim() || loading}
              className="gap-2"
            >
              <Play className="h-4 w-4" />
              Execute
              <kbd className="ml-1 pointer-events-none inline-flex h-5 select-none items-center gap-1 rounded border bg-muted px-1.5 font-mono text-[10px] font-medium text-muted-foreground opacity-100">
                <span className="text-xs">⌘</span>↵
              </kbd>
            </Button>

            <Button
              variant="outline"
              size="sm"
              onClick={handleClear}
              disabled={!value.trim()}
              className="gap-2"
            >
              <Trash2 className="h-4 w-4" />
              Clear
            </Button>

            <Button
              variant="outline"
              size="sm"
              onClick={() => refresh()}
              disabled={!connectionId || !database || metadataLoading}
              className="gap-2"
              title="Refresh autocomplete metadata"
            >
              <RefreshCw className={cn('h-4 w-4', metadataLoading && 'animate-spin')} />
            </Button>
          </div>

          <div className="flex items-center gap-2">
            <div className="flex items-center gap-2 text-sm">
              <CircleDot
                className={cn(
                  'h-3 w-3',
                  connectionId
                    ? 'text-green-500 fill-green-500'
                    : 'text-muted-foreground'
                )}
              />
              <span className="text-muted-foreground">
                {connectionId ? 'Connected' : 'No connection'}
              </span>
            </div>
          </div>
        </div>
      </CardHeader>

      <CardContent className="flex-1 p-0 overflow-hidden">
        <Editor
          height="100%"
          defaultLanguage="sql"
          language="sql"
          theme={getMonacoTheme()}
          value={value}
          onChange={onChange}
          onMount={handleEditorDidMount}
          options={{
            minimap: { enabled: false },
            fontSize: 14,
            lineNumbers: 'on',
            automaticLayout: true,
            scrollBeyondLastLine: false,
            wordWrap: 'on',
            wrappingStrategy: 'advanced',
            padding: { top: 16, bottom: 16 },
            suggestOnTriggerCharacters: true,
            quickSuggestions: true,
            tabSize: 2,
            insertSpaces: true,
            formatOnPaste: true,
            formatOnType: true,
            readOnly: loading,
          }}
          loading={
            <div className="flex items-center justify-center h-full">
              <div className="text-muted-foreground">Loading editor...</div>
            </div>
          }
        />
      </CardContent>
    </Card>
  );
};
