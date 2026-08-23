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

  private fallback: HTMLElement | null = null;

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

  /** Moves keyboard focus to the replacement content after an unrecoverable render failure. */
  public componentDidUpdate(): void {
    if (this.state.failed) {
      this.fallback?.focus({ preventScroll: true });
    }
  }

  /**
   * @returns The protected children or the application fallback.
   */
  public render(): JSX.Element | ReactNode {
    if (this.state.failed) {
      return (
        <main className="grid min-h-dvh place-items-center bg-canvas p-6 text-text" tabIndex={-1}>
          <section
            aria-labelledby="app-error-title"
            className="w-full max-w-lg rounded-lg border border-border bg-surface p-6 shadow-sm"
            ref={(element) => {
              this.fallback = element;
            }}
            tabIndex={-1}
          >
            <p className="type-label text-accent">Cipher</p>
            <h1 className="type-page-title mt-3" id="app-error-title">
              Unable to open Cipher
            </h1>
            <p className="type-body-muted mt-paragraph">Restart the app and try again.</p>
          </section>
        </main>
      );
    }

    return this.props.children;
  }
}
