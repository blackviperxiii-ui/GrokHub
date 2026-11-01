// Renders a reply's Markdown (./markdown) as React elements, the way the cabin
// paints it (crates/grokhub-app/src/markdown.rs). Text only ever lands in the
// DOM as text nodes: no dangerouslySetInnerHTML, so `<script>` or raw HTML in a
// reply shows as the characters it is.
import { memo, useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { openUrl } from "./api";
import {
  codeTokens,
  mdBlocks,
  mdLines,
  mdLinkOk,
  mdPlain,
  mdSpans,
  type MdBlock,
  type MdSpan,
} from "./md";
import { inShell } from "./transport";

export const COPY_LABEL = "Copy";
export const COPIED_LABEL = "Copied";
export const COPY_FAILED_LABEL = "Can't copy";
const COPY_FLASH_MS = 1500;

/** Puts `text` on the clipboard. Falls back to a hidden textarea where the
 * async API is missing (a web app served over plain http). */
async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Try the fallback below.
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  try {
    return document.execCommand?.("copy") ?? false;
  } catch {
    return false;
  } finally {
    area.remove();
  }
}

function Link({ text, url }: { text: string; url: string }) {
  // md_spans only builds http(s) links; check again before anything opens.
  if (!mdLinkOk(url)) return <>{text}</>;
  function onClick(event: MouseEvent<HTMLAnchorElement>) {
    // In the shell the default browser opens it; the web app uses a new tab.
    if (!inShell()) return;
    event.preventDefault();
    void openUrl(url).catch(() => {});
  }
  return (
    <a className="md-link" href={url} title={url} target="_blank" rel="noopener noreferrer" onClick={onClick}>
      {text}
    </a>
  );
}

function Span({ span }: { span: MdSpan }) {
  switch (span.kind) {
    case "text":
      return <>{span.text}</>;
    case "bold":
      return <strong>{span.text}</strong>;
    case "italic":
      return <em>{span.text}</em>;
    case "strike":
      return <s className="md-strike">{span.text}</s>;
    case "code":
      return <code className="md-inline-code">{span.text}</code>;
    case "link":
      return <Link text={span.text} url={span.url} />;
  }
}

function Inline({ line }: { line: string }) {
  return (
    <>
      {mdSpans(line).map((span, i) => (
        <Span key={i} span={span} />
      ))}
    </>
  );
}

function CodeBlock({ lang, body }: { lang: string; body: string }) {
  const [label, setLabel] = useState(COPY_LABEL);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    const ok = await copyText(body);
    setLabel(ok ? COPIED_LABEL : COPY_FAILED_LABEL);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setLabel(COPY_LABEL), COPY_FLASH_MS);
  }
  const tag = lang || "code";
  const lines = mdLines(body);
  return (
    <div className="md-code" role="group" aria-label={`${tag} code`}>
      <div className="md-code-head">
        <span className="md-code-lang">{tag}</span>
        <button type="button" className="quiet md-copy" title="Copy this code" onClick={() => void copy()}>
          {label}
        </button>
      </div>
      <pre>
        <code>
          {lines.map((line, n) => (
            <span key={n}>
              {n > 0 && "\n"}
              {codeTokens(lang, line).map(([tok, text], t) =>
                tok === "plain" ? (
                  text
                ) : (
                  <span key={t} className={`md-tok-${tok}`}>
                    {text}
                  </span>
                ),
              )}
            </span>
          ))}
        </code>
      </pre>
    </div>
  );
}

function Table({ block }: { block: Extract<MdBlock, { kind: "table" }> }) {
  return (
    // Focusable so the keyboard can scroll a wide table sideways.
    <div className="md-table-wrap" role="region" aria-label="Table" tabIndex={0}>
      <table className="md-table">
        <thead>
          <tr>
            {block.header.map((cell, c) => (
              <th key={c} style={{ textAlign: block.align[c] }}>
                {mdPlain(cell)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {block.rows.map((row, r) => (
            <tr key={r}>
              {row.map((cell, c) => (
                <td key={c} style={{ textAlign: block.align[c] }}>
                  <Inline line={cell} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

type ListBlock = Extract<MdBlock, { kind: "bullet" | "numbered" | "task" }>;
type Group = { list: "ul" | "ol"; items: ListBlock[] } | { list: null; block: MdBlock };

/** Runs of list items become one list, so screen readers count them. */
function group(blocks: MdBlock[]): Group[] {
  const out: Group[] = [];
  for (const block of blocks) {
    const list =
      block.kind === "numbered" ? "ol" : block.kind === "bullet" || block.kind === "task" ? "ul" : null;
    const last = out[out.length - 1];
    if (list && last?.list === list) last.items.push(block as ListBlock);
    else if (list) out.push({ list, items: [block as ListBlock] });
    else out.push({ list: null, block });
  }
  return out;
}

function Item({ item }: { item: ListBlock }) {
  const style = { marginInlineStart: `${item.depth * 16}px` };
  switch (item.kind) {
    case "numbered":
      return (
        <li className="md-item" style={style} value={Number(item.num)}>
          <span className="md-marker" aria-hidden="true">
            {item.num}.
          </span>
          <span className="md-item-text">
            <Inline line={item.text} />
          </span>
        </li>
      );
    case "task":
      return (
        <li className="md-item md-task" style={style}>
          <input
            type="checkbox"
            className="md-marker"
            checked={item.done}
            readOnly
            disabled
            aria-label={item.done ? "Done" : "Not done"}
          />
          <span className="md-item-text">
            <Inline line={item.text} />
          </span>
        </li>
      );
    case "bullet":
      return (
        <li className="md-item" style={style}>
          <span className={item.depth === 0 ? "md-marker md-dot" : "md-marker md-dot hollow"} aria-hidden="true" />
          <span className="md-item-text">
            <Inline line={item.text} />
          </span>
        </li>
      );
  }
}

function Block({ block }: { block: MdBlock }): ReactNode {
  switch (block.kind) {
    case "heading": {
      const H = `h${block.level}` as "h1";
      return <H className="md-heading">{mdPlain(block.text)}</H>;
    }
    case "quote":
      return (
        <blockquote className="md-quote">
          {mdLines(block.text).map((line, i) =>
            line.trim() === "" ? (
              <div key={i} className="md-gap" />
            ) : (
              <p key={i}>
                <Inline line={line} />
              </p>
            ),
          )}
        </blockquote>
      );
    case "rule":
      return <hr className="md-rule" />;
    case "table":
      return <Table block={block} />;
    case "code":
      return <CodeBlock lang={block.lang} body={block.body} />;
    case "blank":
      return <div className="md-gap" />;
    case "para":
      return (
        <p className="md-para">
          <Inline line={block.text} />
        </p>
      );
    default:
      return null;
  }
}

/** One reply as Markdown. Memoized on the text, so a streaming reply re-parses
 * only its own bubble as it grows. */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  const groups = useMemo(() => group(mdBlocks(text)), [text]);
  return (
    <div className="md">
      {groups.map((g, i) => {
        if (g.list === null) return <Block key={i} block={g.block} />;
        const List = g.list;
        return (
          <List key={i} className="md-list">
            {g.items.map((item, n) => (
              <Item key={n} item={item} />
            ))}
          </List>
        );
      })}
    </div>
  );
});
