import type { Components } from "react-markdown";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { CodeBlock } from "./code-highlight";
import { cn } from "./utils";

const components: Components = {
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      rel="noopener noreferrer"
      className="font-medium text-[var(--color-info)] underline-offset-2 hover:underline"
    >
      {children}
    </a>
  ),
  code: ({ className, children, ...props }) => {
    const text = String(children ?? "").replace(/\n$/, "");
    const langMatch = /language-([\w-]+)/.exec(className || "");
    const isBlock =
      Boolean(langMatch) || text.includes("\n") || (className || "").includes("language-");
    if (isBlock) {
      return <CodeBlock code={text} language={langMatch?.[1] || "code"} />;
    }
    return (
      <code
        className="rounded-[4px] border border-[var(--color-border)] bg-[var(--color-elevated)] px-1.5 py-0.5 font-mono text-[12px] text-[color-mix(in_oklab,var(--color-info)_75%,var(--color-fg))]"
        {...props}
      >
        {children}
      </code>
    );
  },
  pre: ({ children }) => <>{children}</>,
  ul: ({ children }) => (
    <ul className="my-2 list-disc space-y-1.5 pl-5 marker:text-[var(--color-info)]">{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className="my-2 list-decimal space-y-1.5 pl-5 marker:font-semibold marker:text-[var(--color-info)]">
      {children}
    </ol>
  ),
  li: ({ children }) => (
    <li className="leading-relaxed pl-0.5 [&>p]:my-1">{children}</li>
  ),
  p: ({ children }) => (
    <p className="my-2 leading-relaxed text-[var(--color-fg)] first:mt-0 last:mb-0">{children}</p>
  ),
  strong: ({ children }) => (
    <strong className="font-semibold text-[var(--color-fg)]">{children}</strong>
  ),
  em: ({ children }) => <em className="italic text-[var(--color-muted)]">{children}</em>,
  h1: ({ children }) => (
    <h1 className="mb-2 mt-4 border-b border-[var(--color-border)] pb-1.5 text-base font-semibold tracking-tight first:mt-0">
      {children}
    </h1>
  ),
  h2: ({ children }) => (
    <h2 className="mb-1.5 mt-3.5 text-[0.95rem] font-semibold tracking-tight text-[var(--color-fg)] first:mt-0">
      {children}
    </h2>
  ),
  h3: ({ children }) => (
    <h3 className="mb-1 mt-3 text-sm font-semibold text-[color-mix(in_oklab,var(--color-info)_55%,var(--color-fg))] first:mt-0">
      {children}
    </h3>
  ),
  h4: ({ children }) => (
    <h4 className="mb-1 mt-2 text-sm font-medium text-[var(--color-muted)]">{children}</h4>
  ),
  blockquote: ({ children }) => (
    <blockquote className="my-2.5 rounded-r-[var(--radius-sm)] border-l-[3px] border-[var(--color-info)] bg-[color-mix(in_oklab,var(--color-info)_8%,var(--color-elevated))] px-3 py-2 text-[var(--color-fg)] [&>p]:my-1">
      {children}
    </blockquote>
  ),
  table: ({ children }) => (
    <div className="my-2.5 overflow-x-auto rounded-[var(--radius-sm)] border border-[var(--color-border)]">
      <table className="w-full border-collapse text-xs">{children}</table>
    </div>
  ),
  th: ({ children }) => (
    <th className="border-b border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 py-1.5 text-left font-semibold">
      {children}
    </th>
  ),
  td: ({ children }) => (
    <td className="border-b border-[var(--color-border)] px-2.5 py-1.5 align-top last:border-0">
      {children}
    </td>
  ),
  hr: () => (
    <hr className="my-3 border-0 border-t border-[var(--color-border-strong)] opacity-70" />
  ),
  img: ({ src, alt }) => (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src={src}
      alt={alt || ""}
      className="my-2 max-h-72 max-w-full rounded-[var(--radius-md)] border border-[var(--color-border)] object-contain"
    />
  ),
};

/** Render assistant/system markdown safely (no raw HTML except controlled code spans). */
export function MarkdownBody({
  content,
  className,
  streaming,
}: {
  content: string;
  className?: string;
  /** While streaming, still render markdown so hierarchy stays visible */
  streaming?: boolean;
}) {
  if (!content) return null;
  return (
    <div
      className={cn(
        "markdown-body min-w-0 break-words",
        streaming && "markdown-streaming",
        className,
      )}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {content}
      </ReactMarkdown>
      {streaming ? (
        <span className="ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-[var(--color-fg)] align-middle opacity-70" />
      ) : null}
    </div>
  );
}
