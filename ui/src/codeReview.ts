/** Structured Code tab review output. Mirrors crates/grokhub-code/src/review.rs. */

export const CODE_REVIEW_JSON_SCHEMA =
  '{"verdict":"approve"|"needs-attention","summary":string,"findings":[{"severity":"high"|"medium"|"low"|"info","title":string,"body":string,"file":string,"line_start":number,"line_end":number,"confidence":number,"recommendation":string,"next_steps":string[]}]}';

export const CODE_REVIEW_PROMPT =
  "Review the uncommitted changes in this workspace: what changed, anything risky, and what to test before committing. Reply with JSON only that matches this schema: " +
  CODE_REVIEW_JSON_SCHEMA;

export const CODE_REVIEW_FALLBACK =
  "The model did not return structured JSON. Showing the reply as markdown.";

export type ReviewVerdict = "approve" | "needs-attention";
export type ReviewSeverity = "high" | "medium" | "low" | "info";

export interface ReviewFinding {
  severity: ReviewSeverity;
  title: string;
  body: string;
  file: string;
  lineStart: number;
  lineEnd: number;
  confidence: number;
  recommendation: string;
  nextSteps: string[];
}

export interface StructuredReview {
  verdict: ReviewVerdict;
  summary: string;
  findings: ReviewFinding[];
}

export type ReviewOutput =
  | { kind: "structured"; review: StructuredReview }
  | { kind: "markdown"; text: string };

export function parseCodeReview(text: string): ReviewOutput {
  const review = parseStructured(text);
  if (review) return { kind: "structured", review };
  return { kind: "markdown", text };
}

export function filterFindings(
  review: StructuredReview,
  want: ReviewSeverity | "all",
): ReviewFinding[] {
  if (want === "all") return review.findings;
  return review.findings.filter((item) => item.severity === want);
}

function parseStructured(text: string): StructuredReview | undefined {
  const value = extractObject(text);
  if (!value) return undefined;
  const verdict = parseVerdict(value.verdict);
  if (!verdict || typeof value.summary !== "string") return undefined;
  if (value.findings === undefined) {
    return { verdict, summary: value.summary, findings: [] };
  }
  if (!Array.isArray(value.findings)) return undefined;
  const findings: ReviewFinding[] = [];
  for (const item of value.findings) {
    const finding = parseFinding(item);
    if (!finding) return undefined;
    findings.push(finding);
  }
  return { verdict, summary: value.summary, findings };
}

function parseVerdict(raw: unknown): ReviewVerdict | undefined {
  if (typeof raw !== "string") return undefined;
  const key = raw.trim().toLowerCase().replace(/_/g, "-");
  if (key === "approve") return "approve";
  if (key === "needs-attention" || key === "needsattention") return "needs-attention";
  return undefined;
}

function parseSeverity(raw: unknown): ReviewSeverity | undefined {
  if (typeof raw !== "string") return undefined;
  switch (raw.trim().toLowerCase()) {
    case "critical":
    case "high":
      return "high";
    case "medium":
      return "medium";
    case "low":
      return "low";
    case "info":
      return "info";
    default:
      return undefined;
  }
}

function parseFinding(value: unknown): ReviewFinding | undefined {
  if (!isRecord(value)) return undefined;
  const severity = parseSeverity(value.severity);
  if (
    !severity ||
    typeof value.title !== "string" ||
    typeof value.body !== "string" ||
    typeof value.file !== "string" ||
    typeof value.recommendation !== "string"
  ) {
    return undefined;
  }
  const rawLine = intField(value.line_start);
  if (rawLine === undefined) return undefined;
  const lineStart = Math.max(1, rawLine);
  const lineEnd = Math.max(lineStart, intField(value.line_end) ?? lineStart);
  const confidence = numberField(value.confidence);
  if (confidence === undefined) return undefined;
  let nextSteps: string[] = [];
  if (value.next_steps !== undefined) {
    if (!Array.isArray(value.next_steps) || value.next_steps.some((step) => typeof step !== "string")) {
      return undefined;
    }
    nextSteps = value.next_steps;
  }
  return {
    severity,
    title: value.title,
    body: value.body,
    file: value.file,
    lineStart,
    lineEnd,
    confidence: Math.min(1, Math.max(0, confidence)),
    recommendation: value.recommendation,
    nextSteps,
  };
}

function extractObject(text: string): Record<string, unknown> | undefined {
  const trimmed = text.trim();
  const fenced = trimmed.startsWith("```json")
    ? trimmed.slice(7)
    : trimmed.startsWith("```")
      ? trimmed.slice(3)
      : null;
  const body = (fenced?.endsWith("```") ? fenced.slice(0, -3) : fenced)?.trim() ?? trimmed;
  const direct = readObject(body);
  if (direct) return direct;
  const start = body.indexOf("{");
  const end = body.lastIndexOf("}");
  if (start < 0 || end < start) return undefined;
  return readObject(body.slice(start, end + 1));
}

function readObject(text: string): Record<string, unknown> | undefined {
  try {
    const value: unknown = JSON.parse(text);
    return isRecord(value) ? value : undefined;
  } catch {
    return undefined;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function intField(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value) && value >= 0) return Math.trunc(value);
  return undefined;
}

function numberField(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  return undefined;
}
