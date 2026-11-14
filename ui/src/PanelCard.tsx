// A panel the agent drew with show_panel (generative UI), as the cabin
// painted it: a title, then each element as plain text. A button never runs
// anything itself: it hands its action id up, and the chat sends it as a
// turn, so whatever the agent does next asks like any other step.
import type { PanelElement, PanelSpec } from "./api";

/** The text a panel button sends as the next turn. */
export function panelActionText(panelId: string, action: string): string | null {
  // The core allows only these; checked again so nothing else is ever sent.
  if (!/^[a-z][a-z0-9_-]{0,63}$/.test(action)) return null;
  if (!/^[A-Za-z0-9_.:-]{1,128}$/.test(panelId)) return null;
  return `[panel action] ${action} on ${panelId}`;
}

/** The title, else the first heading, else "Panel <id>". */
export function panelTitle(id: string, spec: PanelSpec): string {
  const title = spec.title?.trim();
  if (title) return title;
  const heading = spec.elements.find((e) => e.type === "heading" && e.text.trim());
  return heading?.type === "heading" ? heading.text.trim() : `Panel ${id}`;
}

/** 0.856 → 86, clamped to 0..100 like the cabin. */
export function percent(value: number): number {
  return Math.round(Math.min(1, Math.max(0, Number.isFinite(value) ? value : 0)) * 100);
}

export function PanelCard({
  id,
  spec,
  onAction,
  onDismiss,
}: {
  id: string;
  spec: PanelSpec;
  /** A button's action id, for the chat to send. */
  onAction: (action: string) => void;
  onDismiss: () => void;
}) {
  const title = panelTitle(id, spec);
  return (
    <div className="card turn-extra panel-card" role="group" aria-label={title}>
      <div className="panel-head">
        <span className="card-title">{title}</span>
        <button type="button" className="chip fix-pill quiet" onClick={onDismiss}>
          Dismiss
        </button>
      </div>
      {spec.elements.map((element, i) => (
        <Element key={i} element={element} onAction={onAction} />
      ))}
    </div>
  );
}

function Element({ element, onAction }: { element: PanelElement; onAction: (action: string) => void }) {
  switch (element.type) {
    case "heading":
      return <p className="panel-heading">{element.text}</p>;
    case "text":
      return <p className="panel-text">{element.text}</p>;
    case "list": {
      const Tag = element.ordered ? "ol" : "ul";
      return (
        <Tag className="panel-list">
          {element.items.map((item, i) => (
            <li key={i}>{item}</li>
          ))}
        </Tag>
      );
    }
    case "table":
      if (element.headers.length === 0) return null;
      return (
        <div className="panel-table-wrap">
          <table className="panel-table">
            <thead>
              <tr>
                {element.headers.map((h, i) => (
                  <th key={i} scope="col">
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {element.rows.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, c) => (
                    <td key={c}>{cell}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "key_value":
      return (
        <dl className="panel-pairs">
          {element.pairs.map((pair, i) => (
            <div key={i} className="panel-pair">
              <dt>{pair.key}:</dt>
              <dd>{pair.value}</dd>
            </div>
          ))}
        </dl>
      );
    case "button":
      return (
        <div className="fix-pills">
          <button type="button" className="chip fix-pill" onClick={() => onAction(element.action)}>
            {element.label}
          </button>
        </div>
      );
    case "input":
      return (
        <p className="panel-text">
          {element.label}: {element.value ?? ""}
        </p>
      );
    case "progress": {
      const pct = percent(element.value);
      return (
        <div className="panel-progress">
          <span>
            {element.label} — {pct}%
          </span>
          <progress max={100} value={pct} aria-label={element.label} />
        </div>
      );
    }
    default:
      return <p className="panel-text">Unsupported panel element.</p>;
  }
}
