// A web-search reply's Sources list as one chip per site, as the cabin drew
// it (ui_kit/source_chips.rs): the chip names the site, hovering shows the
// link, and a click opens it in the browser.
import { openExternal } from "./external";
import type { ReplySource } from "./replySources";

export const SOURCES_LABEL = "Sources";

export function SourceChips({ sources }: { sources: readonly ReplySource[] }) {
  return (
    <ul className="source-chips" aria-label={SOURCES_LABEL}>
      {sources.map((source) => (
        <li key={source.url}>
          <button
            type="button"
            className="source-chip"
            title={source.url}
            aria-label={`Open ${source.site}`}
            onClick={() => void openExternal(source.url).catch(() => {})}
          >
            {source.site}
          </button>
        </li>
      ))}
    </ul>
  );
}
