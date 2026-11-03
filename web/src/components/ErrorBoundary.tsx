import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  /** Names the part of the window, for the message and the console. */
  name: string;
  /** A new key (another view, another note) clears the error and renders again. */
  resetKey?: string;
  /** For a dialog: the fallback sits on the dialog's backdrop with a close button. */
  onClose?: () => void;
  children: ReactNode;
}

interface State {
  error: Error | null;
  key: string | undefined;
}

// One view throwing while it renders must not blank the whole window: this catches it,
// logs it, and shows a small panel in that view's place. The rest of the app keeps working.
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, key: this.props.resetKey };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey !== state.key ? { error: null, key: props.resetKey } : null;
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(`vault-hq: ${this.props.name} hit an error`, error, info.componentStack);
  }

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    const { onClose } = this.props;
    const panel = (
      <div className="view-error" role="alert" onMouseDown={(e) => e.stopPropagation()}>
        <p>
          <b>This {this.props.name} hit an error:</b> {error.message || error.name}
        </p>
        <p className="view-error-actions">
          <button type="button" className="button" onClick={() => this.setState({ error: null })}>
            try again
          </button>{" "}
          <button type="button" className="button" onClick={() => window.location.reload()}>
            reload
          </button>
          {onClose && (
            <>
              {" "}
              <button type="button" className="button" onClick={onClose}>
                close
              </button>
            </>
          )}
        </p>
      </div>
    );
    return onClose ? (
      <div className="palette-backdrop" onMouseDown={onClose}>
        {panel}
      </div>
    ) : (
      panel
    );
  }
}
