import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import type { Job } from "../lib/types";
import { STATUSES } from "../lib/types";
import { filterJobs, salaryText, shortDate, sortJobs, type SortDir, type SortKey } from "../lib/format";
import { EligibilityBadge, Empty, RemoteBadge, SalaryBadge, StatusBadge, useToast } from "../components/ui";
import type { Nav } from "../App";

const COLS: [SortKey, string][] = [
  ["employer", "Employer"], ["title", "Position"], ["status", "Status"], ["score", "Score"], ["remote", "Remote"],
  ["eligibility", "WI eligible"], ["salary", "Pay"], ["posted", "Posted"], ["discovered", "Added"],
];

export function Opportunities({ go, initialStatus }: { go: Nav; initialStatus?: string }) {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [text, setText] = useState("");
  const [status, setStatus] = useState(initialStatus ?? "");
  const [hideExcluded, setHideExcluded] = useState(false);
  const [remoteOnly, setRemoteOnly] = useState(false);
  const [sort, setSort] = useState<[SortKey, SortDir]>(["discovered", "desc"]);
  const { fail } = useToast();

  useEffect(() => { api.listJobs().then((j) => { setJobs(j); setLoaded(true); }).catch(fail); }, []);
  useEffect(() => { setStatus(initialStatus ?? ""); }, [initialStatus]);

  const rows = useMemo(
    () => sortJobs(filterJobs(jobs, { text, status, hideExcluded, remoteOnly }), sort[0], sort[1]),
    [jobs, text, status, hideExcluded, remoteOnly, sort],
  );

  const toggleSort = (k: SortKey) =>
    setSort(([ck, cd]) => (ck === k ? [k, cd === "asc" ? "desc" : "asc"] : [k, k === "employer" || k === "title" ? "asc" : "desc"]));

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Opportunities</h1>
          <p className="muted">{rows.length} of {jobs.length} jobs shown</p>
        </div>
        <button className="btn primary" onClick={() => go({ page: "import" })}>+ Add a job</button>
      </div>

      <div className="filters">
        <input className="search" placeholder="Search title, employer, description, notes…" value={text} onChange={(e) => setText(e.target.value)} />
        <select value={status} onChange={(e) => setStatus(e.target.value)}>
          <option value="">All statuses</option>
          <option value="active">Active (not closed out)</option>
          {STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
        <label className="check"><input type="checkbox" checked={hideExcluded} onChange={(e) => setHideExcluded(e.target.checked)} /> Hide excluded</label>
        <label className="check"><input type="checkbox" checked={remoteOnly} onChange={(e) => setRemoteOnly(e.target.checked)} /> Confirmed remote only</label>
      </div>

      {loaded && jobs.length === 0 ? (
        <Empty>
          No jobs yet. <button className="link" onClick={() => go({ page: "import" })}>Import a posting</button> or load the
          sample data from <button className="link" onClick={() => go({ page: "data" })}>Data &amp; Logs</button>.
        </Empty>
      ) : (
        <div className="table-scroll">
          <table className="table jobs-table">
            <thead>
              <tr>
                {COLS.map(([k, l]) => (
                  <th key={k} className={k === "score" ? "num sortable" : "sortable"} onClick={() => toggleSort(k)} aria-sort={sort[0] === k ? (sort[1] === "asc" ? "ascending" : "descending") : "none"}>
                    {l}{sort[0] === k ? (sort[1] === "asc" ? " ▲" : " ▼") : ""}
                  </th>
                ))}
                <th>Checks</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((j) => (
                <tr key={j.id} className={`clickable ${j.excludedReasons.length ? "row-excluded" : ""}`} onClick={() => go({ page: "job", jobId: j.id })}>
                  <td>{j.employer}{j.isSample && <span className="tag">sample</span>}</td>
                  <td className="strong">{j.title}</td>
                  <td><StatusBadge status={j.status} /></td>
                  <td className="num">{j.score ?? <span className="muted">—</span>}</td>
                  <td><RemoteBadge value={j.remoteType} /></td>
                  <td><EligibilityBadge value={j.wisconsinEligibility} /></td>
                  <td className="nowrap">{salaryText(j)}</td>
                  <td className="nowrap">{shortDate(j.postedDate)}</td>
                  <td className="nowrap">{shortDate(j.discoveredAt)}</td>
                  <td>
                    {j.excludedReasons.length > 0 ? (
                      <span className="badge tone-bad" title={j.excludedReasons.join("\n")}>✕ Excluded</span>
                    ) : j.reviewFlags.length > 0 ? (
                      <span className="badge tone-warn" title={j.reviewFlags.join("\n")}>! {j.reviewFlags.length} to review</span>
                    ) : (
                      <SalaryBadge value={j.salaryFlag} />
                    )}
                    {j.expired && <span className="badge tone-muted">Expired</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
