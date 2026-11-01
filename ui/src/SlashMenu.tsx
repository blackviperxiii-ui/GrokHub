import { useEffect, useRef, useState } from "react";
import { slashHits, type SlashHit } from "./api";

/** A draft the composer runs instead of sending: `/word` and maybe text.
 * `/usr/bin is slow` and `/ hi` are messages. */
export function isSlashCommand(draft: string): boolean {
  return /^\s*\/[A-Za-z0-9_-]+(\s|$)/.test(draft);
}

/** The command list for a draft that is still a bare `/word`. Stale answers
 * from an earlier keystroke are dropped. */
export function useSlashHits(draft: string): SlashHit[] {
  const [hits, setHits] = useState<SlashHit[]>([]);
  const asked = useRef(0);
  const bare = /^\s*\/\S*$/.test(draft);
  useEffect(() => {
    const n = ++asked.current;
    // Not a bare `/word`: the hook returns no rows, and the counter bump
    // drops any answer still on its way.
    if (!bare) return;
    slashHits(draft).then(
      (rows) => {
        if (n === asked.current) setHits(rows);
      },
      () => {
        if (n === asked.current) setHits([]);
      },
    );
  }, [draft, bare]);
  return bare ? hits : [];
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
