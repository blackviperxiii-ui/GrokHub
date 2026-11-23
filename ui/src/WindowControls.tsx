// The window buttons at the right end of the top bar. On Linux and Windows the
// main window has no system title bar (capabilities say windowControls), so
// the top bar drags the window and these minimize, maximize and close it.
// Close asks the window to close, so "keep in the tray" still applies.
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

const PATHS = {
  minimize: "M6 12h12",
  maximize: "M6.5 6.5h11v11h-11z",
  restore: "M9 6.5h8.5V15M6.5 9h8.5v8.5H6.5z",
  close: "M7 7l10 10M17 7L7 17",
} as const;

function Glyph({ name }: { name: keyof typeof PATHS }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={16}
      height={16}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}

export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let live = true;
    const read = () =>
      Promise.resolve()
        .then(() => getCurrentWindow().isMaximized())
        .then(
          (now) => {
            if (live) setMaximized(Boolean(now));
          },
          () => {},
        );
    void read();
    const off = Promise.resolve()
      .then(() => getCurrentWindow().onResized(() => void read()))
      .catch(() => undefined);
    return () => {
      live = false;
      void off.then((unlisten) => unlisten?.());
    };
  }, []);

  const act = (call: () => Promise<void>) => () => {
    Promise.resolve()
      .then(call)
      .catch(() => {});
  };
  return (
    <div className="window-controls">
      <button type="button" aria-label="Minimize" title="Minimize" onClick={act(() => getCurrentWindow().minimize())}>
        <Glyph name="minimize" />
      </button>
      <button
        type="button"
        aria-label={maximized ? "Restore" : "Maximize"}
        title={maximized ? "Restore" : "Maximize"}
        onClick={act(() => getCurrentWindow().toggleMaximize())}
      >
        <Glyph name={maximized ? "restore" : "maximize"} />
      </button>
      <button type="button" className="window-close" aria-label="Close" title="Close" onClick={act(() => getCurrentWindow().close())}>
        <Glyph name="close" />
      </button>
    </div>
  );
}
