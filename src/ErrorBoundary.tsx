import { Component, type ErrorInfo, type JSX, type ReactNode } from "react";

/**
 * @property children - Application content protected by the error boundary.
 */
interface ErrorBoundaryProps {
  children: ReactNode;
}

/**
 * @property failed - Whether a descendant raised an uncaught render error.
 */
interface ErrorBoundaryState {
  failed: boolean;
}

/**
 * Replaces the application shell with a safe fallback after an uncaught render error.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  public state: ErrorBoundaryState = { failed: false };

  /**
   * @returns State that activates the fallback interface.
   */
  public static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  /**
   * Intentionally discards render failures instead of serializing props or a
   * component stack that could contain display text.
   *
   * @param _error - Error raised by a descendant.
   * @param _info - React component stack for the failure.
   */
  public componentDidCatch(_error: Error, _info: ErrorInfo): void {
    // A later diagnostics exporter may use only a fixed, content-free code.
  }

  /**
   * @returns The protected children or the application fallback.
   */
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
