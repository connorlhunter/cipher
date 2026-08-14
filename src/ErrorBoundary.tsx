import { Component, type ErrorInfo, type JSX, type ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  failed: boolean;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  public state: ErrorBoundaryState = { failed: false };

  public static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  public componentDidCatch(_error: Error, _info: ErrorInfo): void {
    // Native diagnostics will receive a redacted failure event later.
  }

  public render(): JSX.Element | ReactNode {
    if (this.state.failed) {
      return (
        <main>
          <p className="eyebrow">Cipher</p>
          <h1>Unable to open Cipher</h1>
          <p className="status">Restart the app and try again.</p>
        </main>
      );
    }

    return this.props.children;
  }
}
