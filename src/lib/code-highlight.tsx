/**
 * Lightweight code block chrome + simple syntax coloring (no heavy deps).
 */
import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { cn } from "./utils";

const KEYWORDS =
  /\b(const|let|var|function|return|if|else|for|while|class|import|export|from|async|await|try|catch|throw|new|typeof|interface|type|extends|implements|public|private|protected|static|void|null|undefined|true|false|package|def|elif|with|as|yield|match|case|break|continue|switch|default|struct|enum|fn|mut|pub|use|mod|self|Self|impl|echo|cd|ls|cat|grep|sudo|export|source)\b/g;
const STRINGS = /("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)/g;
const COMMENTS = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/|#(?!\{)[^\n]*)/g;
const NUMBERS = /\b(\d+\.?\d*)\b/g;

function escapeHtml(s: string) {
  return s
    .replace(/&/g, "&")
    .replace(/</g, "<")
    .replace(/>/g, ">");
}

export function highlightCode(code: string, lang?: string): string {
  let html = escapeHtml(code);
  html = html.replace(COMMENTS, '<span class="tok-cmt">$1</span>');
  html = html.replace(STRINGS, '<span class="tok-str">$1</span>');
  html = html.replace(KEYWORDS, '<span class="tok-kw">$1</span>');
  html = html.replace(NUMBERS, '<span class="tok-num">$1</span>');
  void lang;
  return html;
}

export function CodeBlock({
  code,
  language,
  className,
}: {
  code: string;
  language?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  const lang = (language || "").replace(/^language-/, "") || "code";
  const html = highlightCode(code.replace(/\n$/, ""), lang);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  }

  return (
    <div
      className={cn(
        "code-block group relative my-2.5 overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[color-mix(in_oklab,var(--color-bg)_55%,var(--color-elevated))]",
        className,
      )}
    >
      <div className="flex items-center justify-between gap-2 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-surface)_80%,var(--color-elevated))] px-3 py-1.5">
        <span className="font-mono text-[10px] font-medium uppercase tracking-wider text-[var(--color-info)]">
          {lang}
        </span>
        <button
          type="button"
          onClick={() => void copy()}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
        >
          {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="m-0 overflow-x-auto p-3">
        <code
          className="font-mono text-[12.5px] leading-relaxed text-[var(--color-fg)]"
          dangerouslySetInnerHTML={{ __html: html }}
        />
      </pre>
    </div>
  );
}
