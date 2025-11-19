import { Component, ErrorInfo, ReactNode } from "react";
import { Alert, AlertDescription } from "./ui/alert";
import { Button } from "./ui/button";
import { AlertCircle, RefreshCw } from "lucide-react";

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
}

export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
    errorInfo: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return {
      hasError: true,
      error,
      errorInfo: null,
    };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("ErrorBoundary caught an error:", error, errorInfo);
    this.setState({
      error,
      errorInfo,
    });
  }

  private handleReset = () => {
    this.setState({
      hasError: false,
      error: null,
      errorInfo: null,
    });
  };

  public render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }

      return (
        <div className="flex items-center justify-center h-full p-8">
          <div className="max-w-2xl w-full space-y-4">
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>
                <div className="space-y-2">
                  <div className="font-semibold text-lg">
                    Something went wrong
                  </div>
                  <div className="text-sm">
                    An unexpected error occurred in the application. This has
                    been logged for debugging.
                  </div>
                  {this.state.error && (
                    <details className="mt-4">
                      <summary className="cursor-pointer text-sm font-medium hover:underline">
                        Technical details
                      </summary>
                      <div className="mt-2 p-3 bg-muted rounded-md">
                        <div className="font-mono text-xs break-all">
                          <div className="font-semibold mb-2">
                            {this.state.error.name}:{" "}
                            {this.state.error.message}
                          </div>
                          {this.state.errorInfo && (
                            <pre className="text-xs overflow-auto max-h-48">
                              {this.state.errorInfo.componentStack}
                            </pre>
                          )}
                        </div>
                      </div>
                    </details>
                  )}
                  <div className="flex gap-2 mt-4">
                    <Button onClick={this.handleReset} variant="outline">
                      <RefreshCw className="h-4 w-4 mr-2" />
                      Try Again
                    </Button>
                    <Button
                      onClick={() => window.location.reload()}
                      variant="secondary"
                    >
                      Reload Application
                    </Button>
                  </div>
                </div>
              </AlertDescription>
            </Alert>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
