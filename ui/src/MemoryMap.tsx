import { useEffect, useState, type ReactNode } from "react";
import {
  getMemoryGraph,
  type MemoryEdgeKind,
  type MemoryGraph,
  type MemoryGraphEdge,
  type MemoryGraphNode,
  type MemoryRow,
} from "./api";

/** Node types in the order their columns are drawn. */
export const KIND_ORDER = [
  "preference",
  "fact",
  "decision",
  "person",
  "project",
  "routine",
  "need",
  "mind_prior",
  "trail",
] as const;

const KIND_LABELS: Record<string, string> = {
  preference: "Preferences",
  fact: "Facts",
  decision: "Decisions",
  person: "People",
  project: "Projects",
  routine: "Routines",
  need: "Needs",
  mind_prior: "Habits of mind",
  trail: "Trails",
};

export const EDGE_LABELS: Record<MemoryEdgeKind, string> = {
  references: "Refers to",
  supersedes: "Replaces",
  learned_from_span: "Learned from",
  about_project: "About a project",
  contradicts: "Disagrees with",
};

export const PRIVATE_LABEL = "Private line";
export const MAP_EMPTY = "Nothing to map yet.";
export const MAP_NONE_SHOWN = "No lines match these filters.";
export const MAP_PRIVATE_NOTE = "This line is private. Its words stay closed on the map.";

export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

export function nodeLabel(node: MemoryGraphNode): string {
  return node.sealed || !node.label ? PRIVATE_LABEL : node.label;
}

/** Map geometry, in px. */
const COL = 220;
const ROW = 36;
const PAD = 16;
const HEAD = 24;
const R = 7;
/** A type with more lines wraps into another column beside it. */
export const ROWS_PER_COLUMN = 20;
const LABEL_CHARS = 24;

export interface Placed {
  node: MemoryGraphNode;
  x: number;
  y: number;
}

export interface MapColumn {
  kind: string;
  /** Only a type's first column has a heading. */
  heading: string;
  x: number;
}

export interface MapLayout {
  columns: MapColumn[];
  placed: Placed[];
  width: number;
  height: number;
}

/** Each version's older one: `supersedes` runs from the newer line to the older. */
function olderOf(edges: MemoryGraphEdge[]): Map<string, string> {
  const older = new Map<string, string>();
  for (const edge of edges) if (edge.kind === "supersedes" && !older.has(edge.from)) older.set(edge.from, edge.to);
  return older;
}

/** Nodes clustered by type, one column per type (more when it is long), with
 * each line of versions kept together, newest first. Deterministic. */
export function layoutMap(nodes: MemoryGraphNode[], edges: MemoryGraphEdge[]): MapLayout {
  const byKind = new Map<string, MemoryGraphNode[]>();
  for (const node of nodes) byKind.set(node.kind, [...(byKind.get(node.kind) ?? []), node]);
  const known: string[] = KIND_ORDER.filter((kind) => byKind.has(kind));
  const other = [...byKind.keys()].filter((kind) => !known.includes(kind)).sort((a, b) => a.localeCompare(b));
  const older = olderOf(edges);
  const replaced = new Set(older.values());

  const columns: MapColumn[] = [];
  const placed: Placed[] = [];
  let tallest = 0;
  for (const kind of [...known, ...other]) {
    const group = byKind.get(kind) ?? [];
    const ids = new Map(group.map((node) => [node.id, node]));
    const order: MemoryGraphNode[] = [];
    const seen = new Set<string>();
    const chainFrom = (start: MemoryGraphNode) => {
      let at: MemoryGraphNode | undefined = start;
      while (at && !seen.has(at.id)) {
        seen.add(at.id);
        order.push(at);
        const next = older.get(at.id);
        at = next ? ids.get(next) : undefined;
      }
    };
    // Heads first (lines nothing replaces), then whatever a cycle left.
    for (const node of group) if (!replaced.has(node.id)) chainFrom(node);
    for (const node of group) chainFrom(node);

    order.forEach((node, i) => {
      const sub = Math.floor(i / ROWS_PER_COLUMN);
      if (i % ROWS_PER_COLUMN === 0) {
        columns.push({ kind, heading: sub === 0 ? kindLabel(kind) : "", x: PAD + columns.length * COL });
      }
      const row = i % ROWS_PER_COLUMN;
      placed.push({ node, x: columns[columns.length - 1].x + R, y: PAD + HEAD + row * ROW + R });
      tallest = Math.max(tallest, row + 1);
    });
  }
  return {
    columns,
    placed,
    width: PAD * 2 + Math.max(columns.length, 1) * COL,
    height: PAD * 2 + HEAD + tallest * ROW,
  };
}

/** The line of versions through `id`, newest first; just `id` when it has none. */
export function versionsOf(id: string, edges: MemoryGraphEdge[]): string[] {
  const older = olderOf(edges);
  const newer = new Map<string, string>();
  for (const [from, to] of older) if (!newer.has(to)) newer.set(to, from);
  let head = id;
  const up = new Set([id]);
  for (let next = newer.get(head); next && !up.has(next); next = newer.get(head)) {
    up.add(next);
    head = next;
  }
  const line: string[] = [];
  for (let at: string | undefined = head; at && !line.includes(at); at = older.get(at)) line.push(at);
  return line;
}

/** Each pair of lines that disagree, once. */
export function disagreements(edges: MemoryGraphEdge[]): [string, string][] {
  const seen = new Set<string>();
  const pairs: [string, string][] = [];
  for (const edge of edges) {
    if (edge.kind !== "contradicts" || edge.from === edge.to) continue;
    const key = [edge.from, edge.to].sort().join("\n");
    if (seen.has(key)) continue;
    seen.add(key);
    pairs.push([edge.from, edge.to]);
  }
  return pairs;
}

function short(text: string): string {
  return text.length > LABEL_CHARS ? `${text.slice(0, LABEL_CHARS - 1)}…` : text;
}

function edgePath(a: Placed, b: Placed): string {
  if (a.x === b.x) {
    // Same column: bow out to the right so the line clears the dots between.
    const bow = a.x + 48 + Math.min(Math.abs(a.y - b.y) / 4, 64);
    return `M ${a.x} ${a.y} C ${bow} ${a.y}, ${bow} ${b.y}, ${b.x} ${b.y}`;
  }
  const mid = (a.x + b.x) / 2;
  return `M ${a.x} ${a.y} C ${mid} ${a.y}, ${mid} ${b.y}, ${b.x} ${b.y}`;
}

/** Personal → Map: the learned lines as dots clustered by type, joined by
 * their links. Picking a dot shows that line's own card and actions. */
export function MemoryMap({
  rows,
  renderRow,
  onKeep,
}: {
  /** The Personal rows; a new list means the memory changed, so the map reloads. */
  rows: MemoryRow[];
  /** The list's own card for a row, with its Edit, Don't use and Forget. */
  renderRow: (row: MemoryRow) => ReactNode;
  /** Settles a disagreement: keep one line, forget the other. */
  onKeep: (keep: string, drop: string) => void;
}) {
  const [graph, setGraph] = useState<MemoryGraph | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState<string[]>([]);
  const [onlyProposed, setOnlyProposed] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);

  useEffect(() => {
    let dropped = false;
    getMemoryGraph().then(
      (next) => {
        if (dropped) return;
        setGraph(next);
        setError(null);
      },
      (err: unknown) => {
        if (!dropped) setError(err instanceof Error ? err.message : String(err));
      },
    );
    return () => {
      dropped = true;
    };
  }, [rows]);

  if (error) return <p className="note warn">The map couldn't load: {error}</p>;
  if (!graph) return <p className="empty">Loading the map…</p>;
  if (graph.nodes.length === 0) return <p className="empty">{MAP_EMPTY}</p>;

  const kinds = [...new Set(graph.nodes.map((node) => node.kind))].sort(
    (a, b) => rank(a) - rank(b) || a.localeCompare(b),
  );
  const nodes = graph.nodes.filter((node) => !hidden.includes(node.kind) && (!onlyProposed || node.proposed));
  const shownIds = new Set(nodes.map((node) => node.id));
  const edges = graph.edges.filter((edge) => shownIds.has(edge.from) && shownIds.has(edge.to));
  const layout = layoutMap(nodes, edges);
  const at = new Map(layout.placed.map((p) => [p.node.id, p]));
  const byId = new Map(graph.nodes.map((node) => [node.id, node]));
  const rowOf = new Map(rows.map((row) => [row.id, row]));
  const nameOf = (id: string) => {
    const node = byId.get(id);
    return node ? nodeLabel(node) : id;
  };
  const choice = picked && shownIds.has(picked) ? byId.get(picked) : undefined;
  const pairs = disagreements(edges);

  return (
    <div className="memory-map">
      <div className="memory-map-filters" role="group" aria-label="Map filters">
        {kinds.map((kind) => (
          <label key={kind} className="memory-map-filter">
            <input
              type="checkbox"
              checked={!hidden.includes(kind)}
              onChange={(e) =>
                setHidden((now) => (e.target.checked ? now.filter((k) => k !== kind) : [...now, kind]))
              }
            />
            {kindLabel(kind)}
          </label>
        ))}
        <label className="memory-map-filter">
          <input type="checkbox" checked={onlyProposed} onChange={(e) => setOnlyProposed(e.target.checked)} />
          Only lines waiting for your OK
        </label>
      </div>
      {graph.truncated && <p className="note">The map shows the newest {graph.nodes.length} lines.</p>}
      {graph.locked > 0 && (
        <p className="note">
          {graph.locked === 1 ? "1 private line is" : `${graph.locked} private lines are`} locked, so not on the map.
        </p>
      )}
      {nodes.length === 0 ? (
        <p className="empty">{MAP_NONE_SHOWN}</p>
      ) : (
        <div className="memory-map-canvas">
          <svg
            width={layout.width}
            height={layout.height}
            viewBox={`0 0 ${layout.width} ${layout.height}`}
            role="group"
            aria-label={`Memory map: ${nodes.length} lines, ${edges.length} links`}
          >
            {layout.columns.map((col, i) =>
              col.heading ? (
                <text key={i} className="memory-map-heading" x={col.x} y={PAD + 12}>
                  {col.heading}
                </text>
              ) : null,
            )}
            {edges.map((edge, i) => {
              const a = at.get(edge.from);
              const b = at.get(edge.to);
              if (!a || !b) return null;
              const lit = picked === edge.from || picked === edge.to;
              return (
                <path
                  key={i}
                  className={`memory-map-edge edge-${edge.kind}${lit ? " lit" : ""}`}
                  d={edgePath(a, b)}
                >
                  <title>{`${nameOf(edge.from)} ${EDGE_LABELS[edge.kind].toLowerCase()} ${nameOf(edge.to)}`}</title>
                </path>
              );
            })}
            {layout.placed.map(({ node, x, y }) => {
              const name = nodeLabel(node);
              const state = [
                node.proposed ? "waiting for your OK" : "",
                node.paused ? "not used" : "",
                node.superseded ? "replaced" : "",
              ].filter(Boolean);
              return (
                <g
                  key={node.id}
                  className={[
                    "memory-map-node",
                    node.proposed && "proposed",
                    node.paused && "paused",
                    node.superseded && "superseded",
                    node.sealed && "sealed",
                    picked === node.id && "picked",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  role="button"
                  tabIndex={0}
                  aria-pressed={picked === node.id}
                  aria-label={`${kindLabel(node.kind)}: ${name}${state.length ? `, ${state.join(", ")}` : ""}`}
                  onClick={() => setPicked(picked === node.id ? null : node.id)}
                  onKeyDown={(e) => {
                    if (e.key !== "Enter" && e.key !== " ") return;
                    e.preventDefault();
                    setPicked(picked === node.id ? null : node.id);
                  }}
                >
                  <circle cx={x} cy={y} r={R} />
                  <text x={x + R + 6} y={y + 4}>
                    {short(name)}
                  </text>
                </g>
              );
            })}
          </svg>
        </div>
      )}
      <ul className="memory-map-legend" aria-label="Link kinds">
        {(Object.keys(EDGE_LABELS) as MemoryEdgeKind[]).map((kind) => (
          <li key={kind}>
            <svg width="24" height="8" aria-hidden="true">
              <path className={`memory-map-edge edge-${kind}`} d="M 0 4 L 24 4" />
            </svg>
            {EDGE_LABELS[kind]}
          </li>
        ))}
      </ul>
      {choice && <Picked node={choice} row={rowOf.get(choice.id)} edges={graph.edges} nameOf={nameOf} renderRow={renderRow} onPick={setPicked} />}
      {pairs.length > 0 && (
        <div className="memory-map-disagree" role="group" aria-label="Lines that disagree">
          <h3>Lines that disagree</h3>
          <ul className="rows">
            {pairs.map(([a, b]) => (
              <li key={`${a}\n${b}`} className="card">
                <p>
                  {nameOf(a)} <span className="warn">disagrees with</span> {nameOf(b)}
                </p>
                <div className="memory-row-actions">
                  <button type="button" aria-label={`Keep ${nameOf(a)}, forget ${nameOf(b)}`} onClick={() => onKeep(a, b)}>
                    Keep the first
                  </button>
                  <button type="button" aria-label={`Keep ${nameOf(b)}, forget ${nameOf(a)}`} onClick={() => onKeep(b, a)}>
                    Keep the second
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}
      <details className="memory-map-list">
        <summary>The map as a list</summary>
        {kinds
          .filter((kind) => !hidden.includes(kind))
          .map((kind) => {
            const list = layout.placed.filter((p) => p.node.kind === kind);
            if (list.length === 0) return null;
            return (
              <div key={kind} className="memory-group">
                <h3>{kindLabel(kind)}</h3>
                <ul aria-label={kindLabel(kind)}>
                  {list.map(({ node }) => (
                    <li key={node.id}>
                      <button type="button" className="link" onClick={() => setPicked(node.id)}>
                        {nodeLabel(node)}
                      </button>
                      {linkLines(node.id, edges, nameOf).map((line) => (
                        <span key={line} className="memory-map-link">
                          {" "}
                          · {line}
                        </span>
                      ))}
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
      </details>
    </div>
  );
}

function rank(kind: string): number {
  const at = (KIND_ORDER as readonly string[]).indexOf(kind);
  return at === -1 ? KIND_ORDER.length : at;
}

/** "Replaces X", "Disagrees with Y" for the links that start at `id`. */
function linkLines(id: string, edges: MemoryGraphEdge[], nameOf: (id: string) => string): string[] {
  return edges.filter((edge) => edge.from === id).map((edge) => `${EDGE_LABELS[edge.kind]} ${nameOf(edge.to)}`);
}

function Picked({
  node,
  row,
  edges,
  nameOf,
  renderRow,
  onPick,
}: {
  node: MemoryGraphNode;
  row: MemoryRow | undefined;
  edges: MemoryGraphEdge[];
  nameOf: (id: string) => string;
  renderRow: (row: MemoryRow) => ReactNode;
  onPick: (id: string) => void;
}) {
  const versions = versionsOf(node.id, edges);
  return (
    <div className="memory-map-picked" role="group" aria-label={`Picked: ${nodeLabel(node)}`}>
      <h3>{kindLabel(node.kind)}</h3>
      {row ? <ul className="rows">{renderRow(row)}</ul> : <p className="note">{node.sealed ? MAP_PRIVATE_NOTE : nodeLabel(node)}</p>}
      {versions.length > 1 && (
        <div className="memory-map-versions">
          <p className="note">Versions, newest first:</p>
          <ol aria-label="Versions">
            {versions.map((id) => (
              <li key={id}>
                {id === node.id ? (
                  <strong>{nameOf(id)}</strong>
                ) : (
                  <button type="button" className="link" onClick={() => onPick(id)}>
                    {nameOf(id)}
                  </button>
                )}
              </li>
            ))}
          </ol>
        </div>
      )}
    </div>
  );
}
