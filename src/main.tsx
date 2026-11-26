import { Component, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { api, initArt } from "./api";
import "./styles.css";

/** A rendering bug must never leave a blank window: show what happened and a way out. */
class Boundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error) {
    api.log("error", `render: ${error}\n${error.stack ?? ""}`);
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="empty" style={{ height: "100%" }}>
        <h2>Something went wrong in Nova's interface</h2>
        <div className="selectable mono" id="crash" style={{ maxWidth: 640, fontSize: 12.5 }}>{String(this.state.error)}{"\n"}{this.state.error?.stack}</div>
        <button className="btn primary" onClick={() => location.reload()}>Reload</button>
        <div className="faint" style={{ fontSize: 12 }}>Your library is safe; nothing was lost.</div>
      </div>
    );
  }
}

// Anything that goes wrong in the interface ends up in nova.log.
window.addEventListener("error", (e) => api.log("error", `${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => {
  const r = e.reason;
  // Command failures are shown to the user already; log real exceptions only.
  if (r instanceof Error) api.log("error", `unhandled: ${r}\n${r.stack ?? ""}`);
});

initArt()
  .catch(() => {})
  .finally(() =>
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        <Boundary>
          <App />
        </Boundary>
      </StrictMode>,
    ),
  );
