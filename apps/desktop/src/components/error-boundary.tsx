import { Component, type ErrorInfo, type ReactNode } from "react";
import i18next from "i18next";

import { RotateCcw } from "@/components/icons";
import { ErrorAlert } from "@/components/page";
import { Button } from "@/components/ui/button";

type Props = {
  children: ReactNode;
  /** Title shown above the error message. */
  title?: string;
};

type State = { error: Error | null };

/**
 * Catches render-time exceptions (including failed `React.lazy` chunk loads) below
 * it and shows an inline error instead of letting React unmount the whole tree.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: unknown): State {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  // A full reload is the only reliable recovery: `React.lazy` caches a rejected
  // import, and a dev-server re-optimization also invalidates loaded modules.
  private reload = () => {
    this.setState({ error: null });
    location.reload();
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="grid gap-3">
        <ErrorAlert title={this.props.title ?? i18next.t("errors.somethingWentWrong")} error={error.stack || error.message} />
        <div>
          <Button variant="outline" size="sm" onClick={this.reload}>
            <RotateCcw />
            {i18next.t("errors.reload")}
          </Button>
        </div>
      </div>
    );
  }
}

export default ErrorBoundary;
