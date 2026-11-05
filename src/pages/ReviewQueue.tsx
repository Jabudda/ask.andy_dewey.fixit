import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { BankAnswer, Decision, DocumentRec, JobAnswer, Packet, QueueItem } from "../lib/types";
import { Card, Empty, StatusBadge, useToast } from "../components/ui";
import type { Nav } from "../App";

const GROUPS: [string, string, string][] = [
  ["Awaiting Approval", "Waiting for your decision", "Read the packet, then approve, ask for changes, save for later or pass."],
  ["Approved", "Approved: ready to apply", "Open the official application page and apply there yourself."],
  ["In Progress", "Applying now", "When the employer confirms they received it, record it here."],
  ["Documents Ready", "Ready to send to review", "Documents exist. Send the job to review when you're ready to decide."],
];

const STATUS_LABEL: Record<string, string> = { proposed: "Proposed, please check", needs_input: "⚠️ Needs your answer", confirmed: "✓ Confirmed" };

/** Phase 4: one place to review each application packet. Nothing is ever submitted by the app. */
export function ReviewQueue({ go, initialJobId }: { go: Nav; initialJobId?: number }) {
  const [items, setItems] = useState<QueueItem[] | null>(null);
  const [sel, setSel] = useState<number | null>(initialJobId ?? null);
  const { fail } = useToast();

  const load = () => api.reviewQueue().then((q) => {
    setItems(q);
    setSel((s) => (s && q.some((i) => i.job.id === s) ? s : q[0]?.job.id ?? null));
  }).catch(fail);
  useEffect(() => { load(); }, []);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Review queue</h1>
          <p className="muted">Nothing is submitted without your approval. The app opens the employer's official application page, and you apply there.</p>
        </div>
      </div>

      {items && items.length === 0 ? (
        <Empty>
          The queue is empty. A job appears here once it has documents: open a job, generate and approve a tailored resume, and it moves to <strong>Documents Ready</strong>.{" "}
          <button className="link" onClick={() => go({ page: "jobs" })}>Go to Opportunities</button>
        </Empty>
      ) : (
        <div className="queue">
          <nav className="queue-list" aria-label="Jobs in the queue">
            {GROUPS.map(([status, title, hint]) => {
              const group = (items ?? []).filter((i) => i.job.status === status);
              if (!group.length) return null;
              return (
                <section key={status}>
                  <h2 className="queue-group">{title} <span className="muted">({group.length})</span></h2>
                  <p className="muted small">{hint}</p>
                  {group.map((i) => (
                    <button key={i.job.id} className={`queue-item ${sel === i.job.id ? "active" : ""}`} onClick={() => setSel(i.job.id)}>
                      <span className="queue-title">{i.job.title}</span>
                      <span className="muted small">{i.job.employer}{i.job.score != null ? ` · score ${i.job.score}` : ""}</span>
                      <span className="chips">
                        <span className={`badge tone-${i.resumeApproved ? "good" : "warn"}`}>{i.resumeApproved ? "✓ Resume" : "⚠️ Resume"}</span>
                        {i.answersNeedingInput > 0 && <span className="badge tone-warn">⚠️ {i.answersNeedingInput} to answer</span>}
                        {i.blockers.length === 0 && status === "Awaiting Approval" && <span className="badge tone-good">Ready to approve</span>}
                      </span>
                    </button>
                  ))}
                </section>
              );
            })}
          </nav>
          <div className="queue-detail">
            {sel != null && <PacketView key={sel} jobId={sel} go={go} onChanged={load} />}
          </div>
        </div>
      )}

      <AnswerBank />
    </div>
  );
}

function DocLine({ label, d }: { label: string; d: DocumentRec | null }) {
  const { fail } = useToast();
  if (!d) return <div className="list-row"><span>{label}</span><span className="muted small">none</span></div>;
  return (
    <div className="list-row">
      <span>{label} <span className={`badge tone-${d.status === "approved" ? "good" : "warn"}`}>{d.status === "approved" ? "✓ Approved" : "⚠️ Draft"} · v{d.version}</span></span>
      <span className="btn-row">
        <button className="link" onClick={() => api.openDocumentPdf(d.id).catch(fail)}>Open PDF</button>
        <button className="link" onClick={() => api.revealDocument(d.id).catch(fail)}>Show in Finder</button>
      </span>
    </div>
  );
}

function PacketView({ jobId, go, onChanged }: { jobId: number; go: Nav; onChanged: () => void }) {
  const [p, setP] = useState<Packet | null>(null);
  const [note, setNote] = useState("");
  const [ref, setRef] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const { fail, notify } = useToast();

  const load = () => api.applicationPacket(jobId).then(setP).catch(fail);
  useEffect(() => { load(); }, [jobId]);
  if (!p) return <Card><p className="muted">Loading the packet…</p></Card>;
  const j = p.job;

  const decide = async (d: Decision, done: string) => {
    setBusy(true);
    try { await api.decideJob(j.id, d, note); notify("ok", done); setNote(""); onChanged(); await load(); }
    catch (e) { fail(e); } finally { setBusy(false); }
  };
  const openApplication = async () => {
    try { const url = await api.startApplication(j.id); notify("ok", `Opened ${new URL(url).host} in your browser`); onChanged(); await load(); }
    catch (e) { fail(e); }
  };
  const submitted = async () => {
    try {
      await api.changeStatus({ jobId: j.id, toStatus: "Submitted", note: "Submitted by you on the employer's site", confirmationRef: ref || null, userConfirmedSubmission: confirmed });
      notify("ok", `Marked as submitted. Follow-up reminder set for ${p.followUpDays} days from today.`);
      onChanged();
    } catch (e) { fail(e); }
  };

  return (
    <div className="stack">
      <Card title={<>{j.title} <StatusBadge status={j.status} /></>} actions={<button className="link" onClick={() => go({ page: "job", jobId: j.id })}>Open job page</button>}>
        <p className="muted">{j.employer}{j.score != null ? ` · compatibility ${j.score}/100` : ""}{j.internalCandidate ? " · internal candidate" : ""}</p>
        {p.blockers.length > 0 && j.status !== "Approved" && j.status !== "In Progress" ? (
          <div className="notes-box">
            <strong>⚠️ Before you can approve:</strong>
            <ul>{p.blockers.map((b) => <li key={b}>{b}</li>)}</ul>
          </div>
        ) : j.status === "Awaiting Approval" ? <p className="tone-text-good">✓ Everything is in place. The decision is yours.</p> : null}
        <DocLine label="Tailored resume" d={p.resume} />
        <DocLine label="Cover letter" d={p.coverLetter} />
      </Card>

      <Card title={`Application answers (${p.answers.length})`}>
        <p className="muted small">Proposed only from your standard answers, profile, preferences and the posting. Anything none of those cover is left for you to write. Edit any answer; what you save counts as confirmed.</p>
        {p.answers.map((a) => <AnswerRow key={a.id ?? a.question} a={a} onSaved={load} />)}
        <NewQuestion jobId={j.id} onSaved={load} />
      </Card>

      <Card title="Your decision">
        {j.status === "Documents Ready" && (
          <div className="btn-row"><button className="btn primary" disabled={busy} onClick={() => decide("send", "Sent to review")}>Send to review</button></div>
        )}
        {j.status === "Awaiting Approval" && <>
          <textarea rows={2} placeholder="Optional note (required when you ask for changes)" value={note} onChange={(e) => setNote(e.target.value)} />
          <div className="btn-row">
            <button className="btn primary" disabled={busy || p.blockers.length > 0} onClick={() => decide("approve", "Approved. You can open the application now.")}>Approve this application</button>
            <button className="btn" disabled={busy} onClick={() => decide("changes", "Moved back to Under Review")}>Request changes</button>
            <button className="btn" disabled={busy} onClick={() => decide("later", "Saved for later")}>Save for later</button>
            <button className="btn danger" disabled={busy} onClick={() => decide("reject", "Closed: you passed on this job")}>Pass on this job</button>
          </div>
        </>}
        {(j.status === "Approved" || j.status === "In Progress") && <>
          <ol className="apply-steps">
            <li><button className="btn primary" onClick={openApplication}>Open the application page</button> <span className="muted small">{j.applyUrl ?? j.postingUrl ?? "No link saved: add one on the job page"}</span></li>
            <li>Apply on the employer's site. Use the Copy buttons above for your answers and upload the approved PDFs.</li>
            <li>
              When the employer confirms they received it, record it:
              <div className="form-grid">
                <label className="check"><input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> The employer confirmed receipt (email or on-screen message)</label>
                <input placeholder="Confirmation or reference number (optional)" value={ref} onChange={(e) => setRef(e.target.value)} />
              </div>
              <div className="btn-row"><button className="btn primary" disabled={!confirmed && !ref.trim()} onClick={submitted}>Mark as submitted</button></div>
            </li>
          </ol>
        </>}
      </Card>
    </div>
  );
}

function AnswerRow({ a, onSaved }: { a: JobAnswer; onSaved: () => void }) {
  const [text, setText] = useState(a.answer);
  const { fail, notify } = useToast();
  useEffect(() => setText(a.answer), [a.answer]);
  const dirty = text.trim() !== a.answer.trim();
  const save = async () => { try { await api.saveJobAnswer({ ...a, answer: text }); notify("ok", "Answer saved"); onSaved(); } catch (e) { fail(e); } };
  const copy = async () => { try { await navigator.clipboard.writeText(text); notify("ok", "Copied"); } catch { fail("Couldn't copy. Select the text and press Cmd+C."); } };
  return (
    <div className={`answer ${a.status}`}>
      <label className="answer-q" htmlFor={`ans-${a.id}`}>{a.question}</label>
      <textarea id={`ans-${a.id}`} rows={a.key === "why_interested" ? 4 : 2} value={text} placeholder={a.status === "needs_input" ? "Only you can answer this one" : ""} onChange={(e) => setText(e.target.value)} />
      <div className="answer-meta">
        <span className="small">{STATUS_LABEL[a.status] ?? a.status} · <span className="muted">{a.source}</span></span>
        <span className="btn-row">
          {text.trim() && <button className="link" onClick={copy}>Copy</button>}
          {!a.key && a.id != null && <button className="link" onClick={() => api.deleteJobAnswer(a.id!).then(onSaved).catch(fail)}>Remove</button>}
          <button className="btn small primary" disabled={!dirty} onClick={save}>Save</button>
        </span>
      </div>
    </div>
  );
}

function NewQuestion({ jobId, onSaved }: { jobId: number; onSaved: () => void }) {
  const [q, setQ] = useState("");
  const [ans, setAns] = useState("");
  const { fail } = useToast();
  const add = async () => { try { await api.saveJobAnswer({ jobId, key: "", question: q, answer: ans, source: "You", status: "confirmed" }); setQ(""); setAns(""); onSaved(); } catch (e) { fail(e); } };
  return (
    <details className="new-question">
      <summary>Add a question from this application</summary>
      <div className="form-grid">
        <input placeholder="Question as the employer asks it" value={q} onChange={(e) => setQ(e.target.value)} />
        <textarea rows={2} placeholder="Your answer" value={ans} onChange={(e) => setAns(e.target.value)} />
      </div>
      <button className="btn small" disabled={!q.trim()} onClick={add}>Add question</button>
    </details>
  );
}

function AnswerBank() {
  const [rows, setRows] = useState<BankAnswer[]>([]);
  const { fail, notify } = useToast();
  useEffect(() => { api.answerBank().then(setRows).catch(() => {}); }, []);
  const editable = rows.filter((r) => ["work_authorized", "sponsorship", "start_date", "salary", "travel"].includes(r.key));
  if (!editable.length) return null;
  return (
    <Card title="Your standard answers" className="bank">
      <p className="muted small">Written once by you, reused as proposals on every application. Changing one here doesn't change answers you already confirmed on a job.</p>
      <div className="form-grid">
        {editable.map((r) => (
          <label key={r.key} className="field field-wide">
            <span>{r.question}</span>
            <input defaultValue={r.answer} placeholder={r.hint}
              onBlur={(e) => { if (e.target.value.trim() !== r.answer) api.saveBankAnswer(r.key, e.target.value).then(() => { notify("ok", "Standard answer saved"); setRows((x) => x.map((y) => y.key === r.key ? { ...y, answer: e.target.value.trim() } : y)); }).catch(fail); }} />
          </label>
        ))}
      </div>
    </Card>
  );
}
