import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import type { Contact, Interview, JobDetail as Detail, JobDraft, Reminder } from "../lib/types";
import { STATUSES } from "../lib/types";
import { docKindLabel, draftOf, salaryText, shortDate } from "../lib/format";
import { JobForm } from "../components/JobForm";
import { ScoreCard } from "../components/ScoreCard";
import { DocumentsPanel } from "../components/DocumentsPanel";
import { Card, EligibilityBadge, Empty, Field, RemoteBadge, SalaryBadge, StatusBadge, confirmAction, useToast } from "../components/ui";
import type { Nav } from "../App";

export function JobDetail({ id, go }: { id: number; go: Nav }) {
  const [d, setD] = useState<Detail | null>(null);
  const [editing, setEditing] = useState<JobDraft | null>(null);
  const [notes, setNotes] = useState("");
  const { fail, notify } = useToast();

  const load = () =>
    api.getJobDetail(id).then((x) => { setD(x); setNotes(x.job.notes); }).catch(fail);
  useEffect(() => { load(); }, [id]);

  if (!d) return <div className="page"><p className="muted">Loading…</p></div>;
  const j = d.job;

  const open = (url: string | null) => url && openUrl(url).catch(fail);

  const saveEdit = async () => {
    if (!editing) return;
    try {
      await api.updateJob(j.id, editing);
      setEditing(null);
      notify("ok", "Job updated and re-checked against your requirements");
      load();
    } catch (e) { fail(e); }
  };

  const remove = async () => {
    if (!(await confirmAction(`Delete “${j.title}” at ${j.employer}, with its history, contacts and reminders? This cannot be undone.`, "Delete"))) return;
    try { await api.deleteJob(j.id); notify("ok", "Job deleted"); go({ page: "jobs" }); } catch (e) { fail(e); }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <button className="link" onClick={() => go({ page: "jobs" })}>‹ Opportunities</button>
          <h1>{j.title}</h1>
          <p className="muted">
            {j.employer} · {j.locationText || "Location not stated"} · added {shortDate(j.discoveredAt)}
            {j.isSample && <span className="tag">sample data</span>}
          </p>
        </div>
        <div className="head-actions">
          <StatusBadge status={j.status} />
          {j.postingUrl && <button className="btn" onClick={() => open(j.postingUrl)}>View posting ↗</button>}
          {j.applyUrl && <button className="btn primary" onClick={() => open(j.applyUrl)}>Open employer application ↗</button>}
        </div>
      </div>

      <div className="two-col wide-left">
        <div className="stack">
          <Card title="Requirement checks">
            <div className="checks">
              <div><span className="muted small">Work arrangement</span><RemoteBadge value={j.remoteType} /></div>
              <div><span className="muted small">Hires in Wisconsin</span><EligibilityBadge value={j.wisconsinEligibility} /></div>
              <div><span className="muted small">Pay · {salaryText(j)}</span><SalaryBadge value={j.salaryFlag} /></div>
              <div><span className="muted small">Type</span><span>{j.employmentType || "Not stated"}</span></div>
              <div><span className="muted small">Posted / valid through</span><span>{shortDate(j.postedDate) || "—"} / {shortDate(j.validThrough) || "—"}{j.expired && " (expired)"}</span></div>
              <div><span className="muted small">Compatibility score</span><span className="strong">{j.score ?? "—"}{j.score != null && "/100"}</span></div>
            </div>
            {j.excludedReasons.length > 0 && (
              <div className="alert alert-bad"><strong>⛔ Fails a mandatory requirement</strong><ul>{j.excludedReasons.map((r, i) => <li key={i}>{r}</li>)}</ul></div>
            )}
            {j.reviewFlags.length > 0 && (
              <div className="alert alert-warn"><strong>⚠️ Needs your review</strong><ul>{j.reviewFlags.map((r, i) => <li key={i}>{r}</li>)}</ul></div>
            )}
            {j.detectionNotes.length > 0 && (
              <details className="small"><summary>Where this information came from</summary>
                <ul>{j.detectionNotes.map((r, i) => <li key={i}>{r}</li>)}</ul>
                <p className="muted">Source: {j.source} · extraction: {j.extractionMethod}</p>
              </details>
            )}
          </Card>

          <ScoreCard b={j.scoreBreakdown} />

          <Card title="Job description" actions={
            editing
              ? <><button className="btn small" onClick={() => setEditing(null)}>Cancel</button><button className="btn small primary" onClick={saveEdit}>Save changes</button></>
              : <button className="btn small" onClick={() => setEditing(draftOf(j))}>Edit details</button>
          }>
            {editing ? <JobForm draft={editing} onChange={setEditing} /> : <pre className="description">{j.description || "No description saved."}</pre>}
          </Card>
        </div>

        <div className="stack">
          <StatusPanel detail={d} onChanged={load} />
          <Card title="Notes" actions={<button className="btn small" disabled={notes === j.notes} onClick={() => api.updateNotes(j.id, notes).then(() => { notify("ok", "Notes saved"); load(); }).catch(fail)}>Save</button>}>
            <textarea rows={5} value={notes} onChange={(e) => setNotes(e.target.value)} placeholder="Research, impressions, salary conversations…" />
          </Card>
          <DocumentsPanel jobId={j.id} documents={d.documents} onChanged={load} />
          <Card title={`Other documents (${d.documents.filter((x) => !x.kind.startsWith("generated_")).length})`}>
            {d.documents.filter((x) => !x.kind.startsWith("generated_")).length === 0 ? <Empty>No other documents linked to this job.</Empty> : d.documents.filter((x) => !x.kind.startsWith("generated_")).map((doc) => (
              <div key={doc.id} className="list-row">
                <div><strong>{docKindLabel(doc.kind)}</strong><div className="muted small">{doc.fileName}</div></div>
                <button className="link" onClick={() => api.openDocument(doc.id).catch(fail)}>Open</button>
              </div>
            ))}
          </Card>
          <Interviews jobId={j.id} items={d.interviews} onChanged={load} />
          <Reminders jobId={j.id} items={d.reminders} onChanged={load} />
          <Contacts jobId={j.id} items={d.contacts} onChanged={load} />
          <Card title="Status history">
            <ol className="timeline">
              {d.history.map((h) => (
                <li key={h.id}>
                  <span className="muted small">{h.changedAt}</span>
                  <div>{h.fromStatus ? <>{h.fromStatus} → <strong>{h.toStatus}</strong></> : <strong>{h.toStatus}</strong>}</div>
                  {h.note && <div className="muted small">{h.note}</div>}
                </li>
              ))}
            </ol>
          </Card>
          <button className="btn danger" onClick={remove}>Delete this job</button>
        </div>
      </div>
    </div>
  );
}

function StatusPanel({ detail, onChanged }: { detail: Detail; onChanged: () => void }) {
  const j = detail.job;
  const [to, setTo] = useState(j.status);
  const [note, setNote] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [ref, setRef] = useState("");
  const { fail, notify } = useToast();
  useEffect(() => { setTo(j.status); setConfirmed(false); setRef(""); setNote(""); }, [j.status, j.id]);

  const apply = async (target = to) => {
    try {
      await api.changeStatus({ jobId: j.id, toStatus: target, note, confirmationRef: ref || null, userConfirmedSubmission: confirmed });
      notify("ok", `Status set to ${target}`);
      onChanged();
    } catch (e) { fail(e); }
  };

  const awaiting = j.status === "Awaiting Approval";
  return (
    <Card title="Application status">
      {j.submittedAt && (
        <p className="small">Submitted {j.submittedAt}{j.confirmationRef ? ` · ref ${j.confirmationRef}` : ""} · {j.confirmationMethod === "reference_number" ? "confirmation number recorded" : "confirmed by you"}</p>
      )}
      {awaiting && (
        <div className="approval">
          <p className="small">Review the description, checks and your documents, then decide. Nothing is ever submitted automatically.</p>
          <div className="btn-row">
            <button className="btn primary" onClick={() => apply("Approved")}>Approve for application</button>
            <button className="btn" onClick={() => apply("Documents Ready")}>Request changes</button>
            <button className="btn" onClick={() => apply("Under Review")}>Save for later</button>
            <button className="btn danger" onClick={() => apply("Rejected")}>Reject opportunity</button>
          </div>
        </div>
      )}
      <div className="form-grid">
        <Field label="Move to">
          <select value={to} onChange={(e) => setTo(e.target.value)}>
            {STATUSES.map((s) => (
              <option key={s} value={s} disabled={s === "Approved" && !awaiting}>
                {s}{s === "Approved" && !awaiting ? " (via Awaiting Approval)" : ""}
              </option>
            ))}
          </select>
        </Field>
        <Field label="Note (optional)"><input value={note} onChange={(e) => setNote(e.target.value)} /></Field>
        {to === "Submitted" && j.status !== "Submitted" && (
          <div className="field-wide submit-confirm">
            <label className="check">
              <input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} />
              I confirm the employer received this application (I saw a confirmation page or email).
            </label>
            <Field label="Confirmation / reference number (if shown)"><input value={ref} onChange={(e) => setRef(e.target.value)} /></Field>
          </div>
        )}
      </div>
      <button className="btn primary" disabled={to === j.status || (to === "Submitted" && !confirmed && !ref.trim())} onClick={() => apply()}>
        Update status
      </button>
    </Card>
  );
}

function Interviews({ jobId, items, onChanged }: { jobId: number; items: Interview[]; onChanged: () => void }) {
  const blank: Interview = { jobId, scheduledAt: "", kind: "", notes: "", outcome: "" };
  const [x, setX] = useState<Interview>(blank);
  const { fail } = useToast();
  const save = () => api.saveInterview(x).then(() => { setX(blank); onChanged(); }).catch(fail);
  return (
    <Card title={`Interviews (${items.length})`}>
      {items.map((i) => (
        <div key={i.id} className="list-row">
          <div><strong>{i.scheduledAt.replace("T", " ")}</strong> · {i.kind || "Interview"}{i.outcome && ` — ${i.outcome}`}{i.notes && <div className="muted small">{i.notes}</div>}</div>
          <button className="link danger" onClick={() => api.deleteJobItem("interviews", i.id!).then(onChanged).catch(fail)}>Remove</button>
        </div>
      ))}
      <div className="form-grid">
        <Field label="Date & time"><input type="datetime-local" value={x.scheduledAt} onChange={(e) => setX({ ...x, scheduledAt: e.target.value })} /></Field>
        <Field label="Type"><input placeholder="Phone screen, panel…" value={x.kind} onChange={(e) => setX({ ...x, kind: e.target.value })} /></Field>
        <Field label="Notes" wide><input value={x.notes} onChange={(e) => setX({ ...x, notes: e.target.value })} /></Field>
      </div>
      <button className="btn small" disabled={!x.scheduledAt} onClick={save}>Add interview</button>
    </Card>
  );
}

function Reminders({ jobId, items, onChanged }: { jobId: number; items: Reminder[]; onChanged: () => void }) {
  const blank: Reminder = { jobId, dueDate: "", note: "", done: false };
  const [x, setX] = useState<Reminder>(blank);
  const { fail } = useToast();
  return (
    <Card title={`Follow-up reminders (${items.filter((r) => !r.done).length} open)`}>
      {items.map((r) => (
        <div key={r.id} className={`list-row ${r.done ? "done" : ""}`}>
          <label className="check">
            <input type="checkbox" checked={r.done} onChange={(e) => api.saveReminder({ ...r, done: e.target.checked }).then(onChanged).catch(fail)} />
            {shortDate(r.dueDate)} — {r.note || "Follow up"}
          </label>
          <button className="link danger" onClick={() => api.deleteJobItem("reminders", r.id!).then(onChanged).catch(fail)}>Remove</button>
        </div>
      ))}
      <div className="form-grid">
        <Field label="Due"><input type="date" value={x.dueDate} onChange={(e) => setX({ ...x, dueDate: e.target.value })} /></Field>
        <Field label="What to do"><input value={x.note} onChange={(e) => setX({ ...x, note: e.target.value })} /></Field>
      </div>
      <button className="btn small" disabled={!x.dueDate} onClick={() => api.saveReminder(x).then(() => { setX(blank); onChanged(); }).catch(fail)}>Add reminder</button>
    </Card>
  );
}

function Contacts({ jobId, items, onChanged }: { jobId: number; items: Contact[]; onChanged: () => void }) {
  const blank: Contact = { jobId, name: "", role: "", email: "", phone: "", notes: "" };
  const [x, setX] = useState<Contact>(blank);
  const { fail } = useToast();
  return (
    <Card title={`Recruiter & hiring contacts (${items.length})`}>
      {items.length === 0 && <Empty>No contacts recorded.</Empty>}
      {items.map((c) => (
        <div key={c.id} className="list-row">
          <div><strong>{c.name}</strong>{c.role && ` · ${c.role}`}<div className="muted small">{[c.email, c.phone].filter(Boolean).join(" · ")}</div></div>
          <button className="link danger" onClick={() => api.deleteJobItem("contacts", c.id!).then(onChanged).catch(fail)}>Remove</button>
        </div>
      ))}
      <div className="form-grid">
        <Field label="Name"><input value={x.name} onChange={(e) => setX({ ...x, name: e.target.value })} /></Field>
        <Field label="Role"><input placeholder="Recruiter" value={x.role} onChange={(e) => setX({ ...x, role: e.target.value })} /></Field>
        <Field label="Email"><input value={x.email} onChange={(e) => setX({ ...x, email: e.target.value })} /></Field>
        <Field label="Phone"><input value={x.phone} onChange={(e) => setX({ ...x, phone: e.target.value })} /></Field>
      </div>
      <button className="btn small" disabled={!x.name.trim()} onClick={() => api.saveContact(x).then(() => { setX(blank); onChanged(); }).catch(fail)}>Add contact</button>
    </Card>
  );
}
