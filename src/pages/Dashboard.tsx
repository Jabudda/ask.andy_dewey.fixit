import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { DashboardStats } from "../lib/types";
import { daysUntil, pct, shortDate } from "../lib/format";
import { ActivityChart, HBarChart } from "../components/Charts";
import { Card, Empty, useToast } from "../components/ui";
import type { Nav } from "../App";

const PIPELINE = ["Discovered", "Under Review", "Documents Ready", "Awaiting Approval", "Approved", "In Progress", "Submitted", "Interviewing", "Offer"];

export function Dashboard({ go }: { go: Nav }) {
  const [s, setS] = useState<DashboardStats | null>(null);
  const [editing, setEditing] = useState(false);
  const [date, setDate] = useState("");
  const { fail, notify } = useToast();

  const load = () => api.dashboardStats().then(setS).catch(fail);
  useEffect(() => { load(); }, []);

  if (!s) return <div className="page"><p className="muted">Loading…</p></div>;

  const days = daysUntil(s.milestoneDate);
  const counts = Object.fromEntries(s.statusCounts.map((c) => [c.label, c.count]));
  const closedOut = (counts["Rejected"] ?? 0) + (counts["Withdrawn"] ?? 0) + (counts["Closed"] ?? 0);

  const saveDate = async () => {
    try {
      await api.setMilestone(date);
      setEditing(false);
      notify("ok", "Milestone date updated. Application history is unchanged.");
      load();
    } catch (e) { fail(e); }
  };

  const kpis: [string, number, string, () => void][] = [
    ["Jobs discovered", s.totalJobs, `${s.excluded} excluded · ${s.needsReview} need review`, () => go({ page: "jobs" })],
    ["Strong matches", s.strongMatches, s.unscored ? `${s.unscored} not yet scored (Phase 2)` : "Score ≥ 75", () => go({ page: "jobs" })],
    ["Awaiting approval", s.awaitingApproval, "Your decision needed", () => go({ page: "jobs", status: "Awaiting Approval" })],
    ["Applications submitted", s.submitted, "Confirmed submissions only", () => go({ page: "jobs", status: "Submitted" })],
    ["Interviews scheduled", s.interviewsScheduled, "Upcoming", () => go({ page: "jobs", status: "Interviewing" })],
    ["Offers", s.offers, "Ever reached Offer", () => go({ page: "jobs", status: "Offer" })],
    ["Rejections", s.rejections, "Currently Rejected", () => go({ page: "jobs", status: "Rejected" })],
    ["Need follow-up", s.needsFollowUp, "Due reminders or stale submissions", () => document.querySelector(".followups")?.scrollIntoView({ behavior: "smooth" })],
  ];

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Dashboard</h1>
          <p className="muted">Your search at a glance. All numbers come from records in this app — nothing is projected.</p>
        </div>
        <button className="btn primary" onClick={() => go({ page: "import" })}>+ Add a job</button>
      </div>

      <div className="hero-row">
        <Card className="countdown">
          <div className="countdown-num">{days == null ? "—" : Math.abs(days)}</div>
          <div>
            <div className="countdown-label">{days != null && days < 0 ? "days since milestone" : "days until milestone"}</div>
            {editing ? (
              <div className="inline-form">
                <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
                <button className="btn small primary" onClick={saveDate} disabled={!date}>Save</button>
                <button className="btn small" onClick={() => setEditing(false)}>Cancel</button>
              </div>
            ) : (
              <div className="muted">
                {shortDate(s.milestoneDate)} — current role ends{" "}
                <button className="link" onClick={() => { setDate(s.milestoneDate); setEditing(true); }}>Change</button>
              </div>
            )}
          </div>
        </Card>
        <Card className="rates">
          <div className="rate"><span className="rate-num">{pct(s.interviewRate)}</span><span className="muted">submitted → interview</span></div>
          <div className="rate"><span className="rate-num">{pct(s.offerRate)}</span><span className="muted">submitted → offer</span></div>
          <p className="muted small">Conversion rates are historical ratios of your own applications, not predictions.</p>
        </Card>
      </div>

      <div className="kpis">
        {kpis.map(([label, value, sub, onClick]) => (
          <button key={label} className="kpi" onClick={onClick}>
            <span className="kpi-label">{label}</span>
            <span className="kpi-value">{value}</span>
            <span className="kpi-sub">{sub}</span>
          </button>
        ))}
      </div>

      <Card title="Application pipeline">
        <div className="pipeline">
          {PIPELINE.map((st, i) => (
            <button key={st} className="stage" onClick={() => go({ page: "jobs", status: st })}>
              <span className="stage-count">{counts[st] ?? 0}</span>
              <span className="stage-label">{st}</span>
              {i < PIPELINE.length - 1 && <span className="stage-arrow" aria-hidden>›</span>}
            </button>
          ))}
        </div>
        <p className="muted small">Closed out: {closedOut} (Rejected {counts["Rejected"] ?? 0}, Withdrawn {counts["Withdrawn"] ?? 0}, Closed {counts["Closed"] ?? 0})</p>
      </Card>

      <div className="two-col">
        <Card title="Job-search activity — last 12 weeks">
          <ActivityChart data={s.activity} />
        </Card>
        <Card title="Jobs by compatibility score">
          <HBarChart data={s.scoreBuckets} />
          <p className="muted small">Compatibility scoring is added in Phase 2. Until then every job is shown as “Unscored” rather than given an estimated score.</p>
        </Card>
      </div>

      <Card className="followups" title={`Follow-ups (${s.followUps.length})`}>
        {s.followUps.length === 0 ? (
          <Empty>No follow-ups due. Add reminders on any job's detail page.</Empty>
        ) : (
          <table className="table">
            <thead><tr><th>Position</th><th>Employer</th><th>Why</th><th>Due</th></tr></thead>
            <tbody>
              {s.followUps.map((f, i) => (
                <tr key={i} className="clickable" onClick={() => go({ page: "job", jobId: f.jobId })}>
                  <td>{f.title}</td><td>{f.employer}</td><td>{f.reason}</td><td>{shortDate(f.due)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  );
}
