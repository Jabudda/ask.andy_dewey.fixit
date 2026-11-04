import type { Job, JobDraft } from "./types";

/** Whole days from `today` until `target` (YYYY-MM-DD), negative once passed. Local-calendar based. */
export function daysUntil(target: string, today: Date = new Date()): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(target);
  if (!m) return null;
  const t = Date.UTC(+m[1], +m[2] - 1, +m[3]);
  const n = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  return Math.round((t - n) / 86_400_000);
}

export function money(v: number | null | undefined): string {
  if (v == null || Number.isNaN(v)) return "";
  return v < 1000
    ? `$${v.toFixed(v % 1 === 0 ? 0 : 2)}`
    : `$${Math.round(v).toLocaleString("en-US")}`;
}

const PERIOD: Record<string, string> = { hour: "/hr", day: "/day", week: "/wk", month: "/mo", year: "/yr" };

export function salaryText(j: Pick<JobDraft, "salaryMin" | "salaryMax" | "salaryPeriod">): string {
  const { salaryMin: lo, salaryMax: hi, salaryPeriod: p } = j;
  if (lo == null && hi == null) return "Not listed";
  const suffix = PERIOD[p ?? "year"] ?? "";
  if (lo != null && hi != null && lo !== hi) return `${money(lo)}–${money(hi)}${suffix}`;
  return `${money(lo ?? hi)}${suffix}`;
}

export function annualize(amount: number, period: string | null, hoursPerYear = 2080): number {
  switch (period) {
    case "hour": return amount * hoursPerYear;
    case "day": return amount * (hoursPerYear / 8);
    case "week": return amount * 52;
    case "month": return amount * 12;
    default: return amount;
  }
}

export function pct(v: number | null | undefined): string {
  return v == null ? "—" : `${Math.round(v * 100)}%`;
}

export function shortDate(s: string | null | undefined): string {
  if (!s) return "";
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(s);
  if (!m) return s;
  const d = new Date(+m[1], +m[2] - 1, +m[3]);
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
}

export type SortKey = "employer" | "title" | "status" | "score" | "remote" | "eligibility" | "salary" | "posted" | "discovered";
export type SortDir = "asc" | "desc";

export interface JobFilter {
  text: string;
  status: string; // "" = any; "active" = not closed-out
  hideExcluded: boolean;
  remoteOnly: boolean;
}

const CLOSED = new Set(["Rejected", "Withdrawn", "Closed"]);

export function filterJobs(jobs: Job[], f: JobFilter): Job[] {
  const q = f.text.trim().toLowerCase();
  return jobs.filter((j) => {
    if (f.status === "active" && CLOSED.has(j.status)) return false;
    if (f.status && f.status !== "active" && j.status !== f.status) return false;
    if (f.hideExcluded && j.excludedReasons.length > 0) return false;
    if (f.remoteOnly && j.remoteType !== "remote") return false;
    if (!q) return true;
    return [j.title, j.employer, j.locationText, j.description, j.notes, j.employmentType]
      .some((s) => s?.toLowerCase().includes(q));
  });
}

function sortValue(j: Job, k: SortKey): string | number {
  switch (k) {
    case "employer": return j.employer.toLowerCase();
    case "title": return j.title.toLowerCase();
    case "status": return STATUS_ORDER[j.status] ?? 99;
    case "score": return j.score ?? -1;
    case "remote": return j.remoteType;
    case "eligibility": return j.wisconsinEligibility;
    case "salary": return j.salaryMax != null || j.salaryMin != null
      ? annualize((j.salaryMax ?? j.salaryMin)!, j.salaryPeriod) : -1;
    case "posted": return j.postedDate ?? "";
    case "discovered": return j.discoveredAt;
  }
}

const STATUS_ORDER: Record<string, number> = Object.fromEntries(
  ["Discovered", "Under Review", "Documents Ready", "Awaiting Approval", "Approved", "In Progress", "Submitted",
    "Interviewing", "Offer", "Rejected", "Withdrawn", "Closed"].map((s, i) => [s, i]),
);

export function sortJobs(jobs: Job[], key: SortKey, dir: SortDir): Job[] {
  const m = dir === "asc" ? 1 : -1;
  return [...jobs].sort((a, b) => {
    const va = sortValue(a, key);
    const vb = sortValue(b, key);
    if (va < vb) return -1 * m;
    if (va > vb) return 1 * m;
    return b.id - a.id;
  });
}

/** Split a textarea/comma list into trimmed, unique, non-empty items. */
export function parseList(s: string): string[] {
  const seen = new Set<string>();
  return s
    .split(/[\n,]/)
    .map((x) => x.trim())
    .filter((x) => {
      const k = x.toLowerCase();
      if (!x || seen.has(k)) return false;
      seen.add(k);
      return true;
    });
}

export function emptyDraft(): JobDraft {
  return {
    title: "", employer: "", locationText: "", remoteType: "unknown", wisconsinEligibility: "unknown",
    salaryMin: null, salaryMax: null, salaryPeriod: null, employmentType: "", postedDate: null, validThrough: null,
    applyUrl: null, postingUrl: null, description: "", source: "manual", extractionMethod: "manual", detectionNotes: [],
  };
}

export function draftOf(j: Job): JobDraft {
  const { title, employer, locationText, remoteType, wisconsinEligibility, salaryMin, salaryMax, salaryPeriod,
    employmentType, postedDate, validThrough, applyUrl, postingUrl, description, source, extractionMethod, detectionNotes } = j;
  return { title, employer, locationText, remoteType, wisconsinEligibility, salaryMin, salaryMax, salaryPeriod,
    employmentType, postedDate, validThrough, applyUrl, postingUrl, description, source, extractionMethod, detectionNotes };
}
