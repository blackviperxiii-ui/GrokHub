import { useEffect, useRef, useState } from "react";
import { slashHits, type SlashHit } from "./api";

/** Rust's whitespace is JS `\s` without U+FEFF and with U+0085. */
const RUST_SHELL_LINE = /^(?:(?!\uFEFF)\s|\u0085)*\$ (?:\uFEFF|(?!\u0085)\S)/;

/** A draft the composer runs instead of sending: `/word` and maybe text.
 * `/usr/bin is slow` and `/ hi` are messages. */
export function isSlashCommand(draft: string, remote = false): boolean {
  return /^\s*\/[A-Za-z0-9_-]+(\s|$)/.test(draft) || isShellLine(draft, remote);
}

/** `$ <line>` on this computer is `/sh <line>`; `$5` and `$  ls` are
 * messages, and so is any `$ ` line in a browser. The whitespace is Rust's
 * (`char::is_whitespace`), so this agrees with `slash::dollar_line`. */
export function isShellLine(draft: string, remote = false): boolean {
  return !remote && RUST_SHELL_LINE.test(draft);
}

/** The command list for a draft that is still a bare `/word`. `rows` is the
 * last answer, kept on screen while the next one is on its way; `fresh` is
 * true only when `rows` answers this exact draft, so a key press never acts
 * on an earlier keystroke's list. Stale answers are dropped. */
export function useSlashHits(draft: string): { rows: SlashHit[]; fresh: boolean } {
  const [hits, setHits] = useState<{ for: string; rows: SlashHit[] }>({ for: "", rows: [] });
  const asked = useRef(0);
  const bare = /^\s*\/\S*$/.test(draft);
  useEffect(() => {
    const n = ++asked.current;
    // Not a bare `/word`: the hook returns no rows, and the counter bump
    // drops any answer still on its way.
    if (!bare) return;
    slashHits(draft).then(
      (rows) => {
        if (n === asked.current) setHits({ for: draft, rows });
      },
      () => {
        if (n === asked.current) setHits({ for: draft, rows: [] });
      },
    );
  }, [draft, bare]);
  return bare ? { rows: hits.rows, fresh: hits.for === draft } : { rows: [], fresh: false };
}

/** The list above the composer. The composer keeps focus; Up and Down move
 * the pick, Tab fills it in, Enter runs it, Escape closes the list. */
export function SlashMenu({
  hits,
  at,
  onPick,
  onHover,
}: {
  hits: SlashHit[];
  at: number;
  onPick: (hit: SlashHit) => void;
  onHover: (i: number) => void;
}) {
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const row = list.current?.children[at] as HTMLElement | undefined;
    row?.scrollIntoView?.({ block: "nearest" });
  }, [at]);
  return (
    <ul ref={list} className="slash-menu" id="slash-menu" role="listbox" aria-label="Commands">
      {hits.map((hit, i) => (
        <li
          key={hit.cmd}
          id={`slash-row-${i}`}
          role="option"
          aria-selected={i === at}
          aria-disabled={hit.disabled ? true : undefined}
          className={hit.disabled ? "slash-row off" : "slash-row"}
          onMouseEnter={() => onHover(i)}
          onMouseDown={(e) => {
            // Keep the composer focused.
            e.preventDefault();
            onPick(hit);
          }}
        >
          <code>{hit.cmd}</code>
          <span className="slash-hint">{hit.hint}</span>
          {hit.disabled && <span className="badge">{hit.disabled}</span>}
        </li>
      ))}
    </ul>
  );
}
