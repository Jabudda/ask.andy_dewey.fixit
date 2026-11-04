import { describe, expect, it } from "vitest";
import { annualize, daysUntil, docKindLabel, draftOf, emptyDraft, filterJobs, money, parseList, pct, salaryText, sortJobs } from "./format";
import type { Job } from "./types";

function job(over: Partial<Job>): Job {
  return {
    ...emptyDraft(),
    id: 1, title: "Data Analyst", employer: "Fabrikam", salaryFlag: "missing", reviewFlags: [], excludedReasons: [],
    expired: false, status: "Discovered", score: null, assessment: null, notes: "", submittedAt: null,
    confirmationRef: null, confirmationMethod: null, isSample: false, discoveredAt: "2026-11-01 10:00:00",
    updatedAt: "2026-11-01 10:00:00",
    ...over,
  };
}

describe("countdown", () => {
  it("counts calendar days to the milestone", () => {
    expect(daysUntil("2027-01-31", new Date(2026, 10, 4, 23, 30))).toBe(88);
    expect(daysUntil("2026-11-04", new Date(2026, 10, 4))).toBe(0);
    expect(daysUntil("2026-11-01", new Date(2026, 10, 4))).toBe(-3);
    expect(daysUntil("soon")).toBeNull();
  });
});

describe("formatting", () => {
  it("formats money and salary ranges", () => {
    expect(money(65000)).toBe("$65,000");
    expect(money(28.5)).toBe("$28.50");
    expect(money(null)).toBe("");
    expect(salaryText({ salaryMin: 60000, salaryMax: 75000, salaryPeriod: "year" })).toBe("$60,000–$75,000/yr");
    expect(salaryText({ salaryMin: 30, salaryMax: 30, salaryPeriod: "hour" })).toBe("$30/hr");
    expect(salaryText({ salaryMin: null, salaryMax: null, salaryPeriod: null })).toBe("Not listed");
    expect(pct(0.256)).toBe("26%");
    expect(pct(null)).toBe("—");
  });
  it("annualizes like the backend", () => {
    expect(annualize(25, "hour")).toBe(52000);
    expect(annualize(5000, "month")).toBe(60000);
    expect(annualize(70000, null)).toBe(70000);
  });
  it("labels document kinds", () => {
    expect(docKindLabel("sent_cover_letter")).toBe("Cover letter sent");
    expect(docKindLabel("something_new")).toBe("something new");
  });
  it("parses lists without duplicates", () => {
    expect(parseList("Excel, excel\nSmartsheet,, \n SharePoint")).toEqual(["Excel", "Smartsheet", "SharePoint"]);
  });
});

describe("filter & sort", () => {
  const jobs = [
    job({ id: 1, employer: "Beta", status: "Submitted", salaryMin: 30, salaryMax: 30, salaryPeriod: "hour", remoteType: "remote" }),
    job({ id: 2, employer: "alpha", status: "Rejected", salaryMin: 70000, salaryMax: 70000, salaryPeriod: "year" }),
    job({ id: 3, employer: "Gamma", status: "Discovered", excludedReasons: ["Hybrid"], description: "Smartsheet admin" }),
  ];
  it("filters by text, status and exclusions", () => {
    expect(filterJobs(jobs, { text: "smartsheet", status: "", hideExcluded: false, remoteOnly: false }).map((j) => j.id)).toEqual([3]);
    expect(filterJobs(jobs, { text: "", status: "active", hideExcluded: true, remoteOnly: false }).map((j) => j.id)).toEqual([1]);
    expect(filterJobs(jobs, { text: "", status: "Rejected", hideExcluded: false, remoteOnly: false }).map((j) => j.id)).toEqual([2]);
    expect(filterJobs(jobs, { text: "", status: "", hideExcluded: false, remoteOnly: true }).map((j) => j.id)).toEqual([1]);
  });
  it("sorts by employer, annualized salary and pipeline order", () => {
    expect(sortJobs(jobs, "employer", "asc").map((j) => j.id)).toEqual([2, 1, 3]);
    expect(sortJobs(jobs, "salary", "desc").map((j) => j.id)).toEqual([2, 1, 3]);
    expect(sortJobs(jobs, "status", "asc").map((j) => j.id)).toEqual([3, 1, 2]);
  });
  it("extracts an editable draft from a job", () => {
    const d = draftOf(jobs[0]);
    expect(d.employer).toBe("Beta");
    expect("id" in d).toBe(false);
  });
});
