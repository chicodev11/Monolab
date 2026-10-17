import { tr } from "../../lib/i18n";
import { Component, type ErrorInfo, type ReactNode } from "react";

export class ErrorBoundary extends Component<
  { children: ReactNode; onReset?: () => void },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(tr("Couldn't display this screen."), error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="crash-screen">
        <div>
          <h2>{tr("An error occurred while displaying this screen.")}</h2>
          <p>
            {tr("Server files haven't changed. Go back below, or close and reopen this window.")}</p>
          <pre>{error.message}</pre>
          <div className="empty-actions">
            <button
              className="button primary"
              onClick={() => {
                this.setState({ error: null });
                this.props.onReset?.();
              }}
            >
              {tr("Back to list")}</button>
            <button
              className="button secondary"
              onClick={() => window.location.reload()}
            >
              {tr("Refresh")}</button>
          </div>
        </div>
      </div>
    );
  }
}
