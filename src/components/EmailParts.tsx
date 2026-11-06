import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import type { AlertJob, EmailCategory, EmailMessage, Job } from "../lib/types";
import { useToast } from "./ui";
import type { Nav } from "../App";

export const CATEGORY: Record<EmailCategory, [string, string]> = {
  alert: ["Job alert", "info"],
  confirmation: ["Application received", "good"],
  rejection: ["Not moving forward", "bad"],
  interview: ["Interview", "accent"],
  assessment: ["Assessment", "warn"],
  offer: ["Offer", "good"],
  recruiter: ["Recruiter / employer", "neutral"],
  sent: ["Sent by you", "muted"],
  other: ["Other", "muted"],
};

const OUTCOME: Record<string, [string, string]> = {
  added: ["Added", "good"],
  duplicate: ["Already in your list", "muted"],
  filtered: ["Not a match", "muted"],
  excluded: ["Fails a requirement", "warn"],
  incomplete: ["Company unclear", "warn"],
  error: ["Error", "bad"],
};

export function CategoryBadge({ c }: { c: EmailCategory }) {
  const [label, tone] = CATEGORY[c] ?? CATEGORY.other;
  return <span className={`badge tone-${tone}`}>{label}</span>;
}

export function who(m: EmailMessage): string {
  if (m.direction === "out") return `To ${m.toAddrs || "—"}`;
  return m.fromName ? `${m.fromName} <${m.fromAddr}>` : m.fromAddr;
}

/** Accept / dismiss buttons for a proposed status change. */
export function SuggestionActions({ m, onChanged }: { m: EmailMessage; onChanged: () => void }) {
  const { fail, notify } = useToast();
  if (m.suggestionState === "accepted") return <span className="badge tone-good">✓ Marked {m.suggestion}</span>;
  if (m.suggestionState === "dismissed") return <span className="muted small">Suggestion dismissed</span>;
  if (m.suggestionState === "stale") return <span className="muted small">Suggested {m.suggestion}; the job has moved on since</span>;
  if (m.suggestionState !== "pending" || !m.suggestion) return null;
  return (
    <div className="btn-row">
      <button className="btn small primary" onClick={() => api.emailAccept(m.id).then(() => { notify("ok", `Marked ${m.suggestion}`); onChanged(); }).catch(fail)}>
        Mark as {m.suggestion}
      </button>
      <button className="btn small" onClick={() => api.emailDismiss(m.id).then(onChanged).catch(fail)}>Dismiss</button>
    </div>
  );
}

function AlertJobs({ m, go, onChanged }: { m: EmailMessage; go: Nav; onChanged: (m: EmailMessage) => void }) {
  const { fail, notify } = useToast();
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<AlertJob | null>(null);
  const add = (i: number, edited: AlertJob | null) =>
    api.emailAddAlertJob(m.id, i, edited).then((x) => { setEditing(null); notify("ok", "Job added to Opportunities"); onChanged(x); }).catch(fail);
  if (m.alertJobs.length === 0) return <p className="muted small">No job listings were found in this email.</p>;
  return (
    <table className="table compact">
      <thead><tr><th>Job</th><th>Company</th><th>Location</th><th>Result</th><th></th></tr></thead>
      <tbody>
        {m.alertJobs.map((j, i) => {
          const [label, tone] = OUTCOME[j.outcome] ?? ["—", "muted"];
          if (editing === i && draft) {
            return (
              <tr key={i}>
                <td><input value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} /></td>
                <td><input value={draft.employer} onChange={(e) => setDraft({ ...draft, employer: e.target.value })} placeholder="Company" /></td>
                <td><input value={draft.location} onChange={(e) => setDraft({ ...draft, location: e.target.value })} /></td>
                <td colSpan={2}>
                  <div className="btn-row">
                    <button className="btn small primary" disabled={!draft.title.trim() || !draft.employer.trim()} onClick={() => add(i, draft)}>Add</button>
                    <button className="btn small" onClick={() => setEditing(null)}>Cancel</button>
                  </div>
                </td>
              </tr>
            );
          }
          return (
            <tr key={i} className={j.outcome === "filtered" || j.outcome === "duplicate" ? "row-excluded" : ""}>
              <td>{j.url ? <button className="link" onClick={() => openUrl(j.url).catch(fail)}>{j.title}</button> : j.title}</td>
              <td>{j.employer || <span className="muted">—</span>}</td>
              <td>{j.location || <span className="muted">—</span>}</td>
              <td><span className={`badge tone-${tone}`} title={j.reason}>{label}</span></td>
              <td>
                {j.jobId != null && (j.outcome === "added" || j.outcome === "duplicate") ? (
                  <button className="link" onClick={() => go({ page: "job", jobId: j.jobId! })}>Open job</button>
                ) : (
                  <button className="link" title="Check the details, then add it to Opportunities" onClick={() => { setEditing(i); setDraft({ ...j }); }}>Add anyway…</button>
                )}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** One email, expandable: why it was linked, the link control, the text, and any alert jobs. */
export function EmailItem({ m: initial, jobs, go, onChanged, showJob = true }: { m: EmailMessage; jobs: Job[]; go: Nav; onChanged: () => void; showJob?: boolean }) {
  const [m, setM] = useState(initial);
  const [open, setOpen] = useState(false);
  const [body, setBody] = useState<string | null>(null);
  const { fail, notify } = useToast();
  useEffect(() => setM(initial), [initial]);
  useEffect(() => {
    if (open && body === null) api.emailBody(m.id).then(setBody).catch(() => setBody(""));
  }, [open]);

  const link = (v: string) =>
    api.emailLink(m.id, v === "" ? null : Number(v)).then((x) => { setM(x); notify("ok", v === "" ? "Unlinked" : "Linked to the job"); onChanged(); }).catch(fail);
  const reread = () => api.emailReread(m.id).then((x) => { setM(x); notify("ok", `Read again: ${x.alertJobs.length} job(s) found`); onChanged(); }).catch(fail);

  return (
    <div className={`email-item ${open ? "open" : ""}`}>
      <button className="email-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="email-dir" aria-label={m.direction === "out" ? "Sent" : "Received"}>{m.direction === "out" ? "↗" : "↙"}</span>
        <span className="email-main">
          <span className="email-subject">{m.subject || "(no subject)"}</span>
          <span className="muted small">{who(m)} · {m.sentAt.slice(0, 16)}</span>
        </span>
        <span className="email-tags">
          <CategoryBadge c={m.category} />
          {m.category === "alert" && <span className="badge tone-neutral">{m.alertJobs.filter((j) => j.outcome === "added").length} added / {m.alertJobs.length}</span>}
          {showJob && m.jobLabel && <span className="badge tone-accent" title={m.matchNote}>{m.jobLabel}</span>}
          {m.suggestionState === "pending" && <span className="badge tone-warn">Needs your OK</span>}
        </span>
      </button>
      {open && (
        <div className="email-body">
          {m.category !== "alert" && (
            <div className="email-link-row">
              <label className="small muted">Application</label>
              <select value={m.jobId ?? ""} onChange={(e) => link(e.target.value)}>
                <option value="">Not linked to a job</option>
                {jobs.map((j) => <option key={j.id} value={j.id}>{j.employer} — {j.title} ({j.status})</option>)}
              </select>
              {m.jobId != null && showJob && <button className="link" onClick={() => go({ page: "job", jobId: m.jobId! })}>Open job</button>}
            </div>
          )}
          {m.matchNote && <p className="muted small">{m.matchNote}</p>}
          <SuggestionActions m={m} onChanged={onChanged} />
          {m.category === "alert" && <AlertJobs m={m} go={go} onChanged={(x) => { setM(x); onChanged(); }} />}
          {m.board && <div className="btn-row"><button className="btn small" onClick={reread} title="Read this job-board email again with the current rules">Read again</button></div>}
          <pre className="email-text">{body === null ? "Loading…" : body || m.snippet || "(no text)"}</pre>
        </div>
      )}
    </div>
  );
}
