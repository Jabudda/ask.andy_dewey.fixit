import { useState } from "react";
import { api } from "../lib/api";
import type { Job, JobDraft } from "../lib/types";
import { emptyDraft } from "../lib/format";
import { JobForm } from "../components/JobForm";
import { Card, Field, StatusBadge, useToast } from "../components/ui";
import type { Nav } from "../App";

type Mode = "url" | "paste" | "manual";

export function ImportJob({ go }: { go: Nav }) {
  const [mode, setMode] = useState<Mode>("url");
  const [url, setUrl] = useState("");
  const [text, setText] = useState("");
  const [draft, setDraft] = useState<JobDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [dup, setDup] = useState<Job | null>(null);
  const { fail, notify } = useToast();

  const run = async (fn: () => Promise<JobDraft>) => {
    setBusy(true);
    setDup(null);
    try { setDraft(await fn()); } catch (e) { fail(e); } finally { setBusy(false); }
  };

  const save = async (force: boolean) => {
    if (!draft) return;
    setBusy(true);
    try {
      const r = await api.saveJob(draft, force);
      if (r.duplicateOf) { setDup(r.duplicateOf); return; }
      notify("ok", "Job saved");
      go({ page: "job", jobId: r.id! });
    } catch (e) { fail(e); } finally { setBusy(false); }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Add a job</h1>
          <p className="muted">Import from the employer's posting link, paste the posting text, or type it in. You review every field before it is saved.</p>
        </div>
      </div>

      <div className="tabs">
        {(["url", "paste", "manual"] as Mode[]).map((m) => (
          <button key={m} className={`tab ${mode === m ? "active" : ""}`} onClick={() => { setMode(m); setDraft(m === "manual" ? emptyDraft() : null); setDup(null); }}>
            {m === "url" ? "From a link" : m === "paste" ? "Paste posting text" : "Enter manually"}
          </button>
        ))}
      </div>

      {mode === "url" && (
        <Card>
          <Field label="Job posting URL" hint="Works best with employer career sites and ATS pages (Greenhouse, Lever, Workday, iCIMS, SmartRecruiters…) that publish schema.org job data. The app makes one request to this page; it never logs in or bypasses site protections.">
            <div className="inline-form">
              <input className="grow" value={url} placeholder="https://careers.employer.com/jobs/123" onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && url && run(() => api.fetchJobFromUrl(url))} />
              <button className="btn primary" disabled={!url || busy} onClick={() => run(() => api.fetchJobFromUrl(url))}>{busy ? "Fetching…" : "Fetch"}</button>
            </div>
          </Field>
        </Card>
      )}

      {mode === "paste" && (
        <Card>
          <Field label="Posting text" hint="Copy the whole posting from your browser. Lines like “Title:”, “Company:”, “Location:” are recognised; anything guessed is marked for you to check.">
            <textarea rows={10} value={text} onChange={(e) => setText(e.target.value)} />
          </Field>
          <button className="btn primary" disabled={text.trim().length < 20 || busy} onClick={() => run(() => api.parsePastedJob(text))}>Read posting</button>
        </Card>
      )}

      {draft && (
        <Card title="Review before saving" actions={<button className="btn primary" disabled={busy} onClick={() => save(false)}>Save job</button>}>
          {dup && (
            <div className="alert alert-warn">
              <strong>This looks like a job you already saved:</strong>
              <div className="list-row">
                <span>#{dup.id} {dup.title} — {dup.employer} <StatusBadge status={dup.status} /></span>
                <span className="btn-row">
                  <button className="btn small" onClick={() => go({ page: "job", jobId: dup.id })}>Open existing</button>
                  <button className="btn small" onClick={() => save(true)}>It's a different posting — save anyway</button>
                </span>
              </div>
            </div>
          )}
          <JobForm draft={draft} onChange={setDraft} />
        </Card>
      )}
    </div>
  );
}
