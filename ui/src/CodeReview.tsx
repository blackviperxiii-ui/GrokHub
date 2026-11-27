import {
  CODE_REVIEW_FALLBACK,
  filterFindings,
  type ReviewFinding,
  type ReviewOutput,
  type ReviewSeverity,
} from "./codeReview";

const SEVERITIES: ReviewSeverity[] = ["high", "medium", "low", "info"];

function verdictLabel(verdict: "approve" | "needs-attention"): string {
  return verdict === "approve" ? "Approve" : "Needs attention";
}

function severityLabel(severity: ReviewSeverity): string {
  switch (severity) {
    case "high":
      return "High";
    case "medium":
      return "Medium";
    case "low":
      return "Low";
    case "info":
      return "Info";
    default: {
      const _exhaustive: never = severity;
      return _exhaustive;
    }
  }
}

export function CodeReviewPanel({
  output,
  filter,
  onFilter,
  onJump,
}: {
  output: ReviewOutput;
  filter: ReviewSeverity | "all";
  onFilter: (next: ReviewSeverity | "all") => void;
  onJump: (file: string, line: number) => void;
}) {
  if (output.kind === "markdown") {
    return (
      <section className="code-review" aria-label="Code review">
        <p className="note" role="status">
          {CODE_REVIEW_FALLBACK}
        </p>
        <pre className="code-diff" tabIndex={0} role="region" aria-label="Review as markdown">
          {output.text}
        </pre>
      </section>
    );
  }

  const { review } = output;
  const shown = filterFindings(review, filter);
  const present = new Set(review.findings.map((item) => item.severity));

  return (
    <section className="code-review" aria-label="Code review">
      <header className="code-review-head">
        <span
          className={`badge${review.verdict === "needs-attention" ? " warn" : ""}`}
          data-verdict={review.verdict}
        >
          {verdictLabel(review.verdict)}
        </span>
        <p className="code-review-summary">{review.summary}</p>
      </header>
      <div className="code-review-filters" role="group" aria-label="Filter findings by severity">
        <FilterChip label="All" pressed={filter === "all"} onClick={() => onFilter("all")} />
        {SEVERITIES.filter((sev) => present.has(sev)).map((sev) => (
          <FilterChip
            key={sev}
            label={severityLabel(sev)}
            pressed={filter === sev}
            onClick={() => onFilter(sev)}
          />
        ))}
      </div>
      {shown.length === 0 ? (
        <p className="empty">No findings at this severity.</p>
      ) : (
        <ul className="code-review-list">
          {shown.map((item, index) => (
            <FindingCard key={`${item.file}:${item.lineStart}:${index}`} item={item} onJump={onJump} />
          ))}
        </ul>
      )}
    </section>
  );
}

function FilterChip({
  label,
  pressed,
  onClick,
}: {
  label: string;
  pressed: boolean;
  onClick: () => void;
}) {
  return (
    <button type="button" className="chip" aria-pressed={pressed} onClick={onClick}>
      {label}
    </button>
  );
}

function FindingCard({
  item,
  onJump,
}: {
  item: ReviewFinding;
  onJump: (file: string, line: number) => void;
}) {
  const place = `${item.file}:${item.lineStart}`;
  return (
    <li className="card code-review-finding">
      <div className="code-review-finding-top">
        <span className="badge">{severityLabel(item.severity)}</span>
        <strong>{item.title}</strong>
      </div>
      {item.file ? (
        <button
          type="button"
          className="quiet code-review-place"
          onClick={() => onJump(item.file, item.lineStart)}
        >
          {place}
        </button>
      ) : (
        <span className="note">{place}</span>
      )}
      <p>{item.body}</p>
      {item.recommendation ? <p className="note">{item.recommendation}</p> : null}
      {item.nextSteps.length > 0 ? (
        <ul>
          {item.nextSteps.map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ul>
      ) : null}
    </li>
  );
}
