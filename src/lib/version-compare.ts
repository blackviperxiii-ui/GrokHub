/** Compare dotted versions; true when `a` is strictly newer than `b`. */
export function versionNewer(a: string, b: string): boolean {
  const parts = (v: string) =>
    String(v || "")
      .replace(/^v/i, "")
      .split(/[.+-]/)
      .map((p) => {
        const n = parseInt(p, 10);
        return Number.isFinite(n) ? n : 0;
      });
  const pa = parts(a);
  const pb = parts(b);
  const n = Math.max(pa.length, pb.length);
  for (let i = 0; i < n; i++) {
    const da = pa[i] || 0;
    const db = pb[i] || 0;
    if (da > db) return true;
    if (da < db) return false;
  }
  return false;
}
