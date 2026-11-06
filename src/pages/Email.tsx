import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import type { EmailMessage, EmailStatus, Job } from "../lib/types";
import { Card, Empty, Field, confirmAction, useToast } from "../components/ui";
import { EmailItem, SuggestionActions, who } from "../components/EmailParts";
import type { Nav } from "../App";

const INTERVALS: [number, string][] = [[0, "Off (check only when I click)"], [15, "Every 15 minutes"], [30, "Every 30 minutes"], [60, "Every hour"], [120, "Every 2 hours"], [240, "Every 4 hours"]];
const FILTERS: [string, string][] = [["all", "All"], ["linked", "Linked to applications"], ["unlinked", "Employer mail not linked"], ["alert", "Job alerts"]];

function Setup({ status, onChanged }: { status: EmailStatus; onChanged: (s: EmailStatus) => void }) {
  const [id, setId] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const { fail, notify } = useToast();
  const link = (u: string) => openUrl(u).catch(fail);
  const save = () => api.emailSaveClient(id, secret).then((s) => { setId(""); setSecret(""); notify("ok", "Saved in your macOS Keychain"); onChanged(s); }).catch(fail);
  const connect = async () => {
    setBusy(true);
    notify("info", "Your browser is opening Google's sign-in. Choose your job-search Gmail account.");
    try { const s = await api.emailConnect(); notify("ok", `Connected to ${s.address}`); onChanged(s); } catch (e) { fail(e); } finally { setBusy(false); }
  };
  return (
    <Card title="Connect your job-search Gmail" actions={<span className="badge tone-warn">Not connected</span>}>
      <div className="alert alert-info small">
        <strong>What the app can do:</strong> read your mail, nothing else. It asks Google only for <em>read-only</em> access, so it cannot send,
        delete, label or mark anything as read. Emails are sorted on this Mac by rules; their content is never sent to Claude or anywhere else.
        Every status change it proposes waits for your OK.
      </div>
      <h4>One-time setup (about 10 minutes)</h4>
      <ol className="setup-steps">
        <li>Open the <button className="link" onClick={() => link("https://console.cloud.google.com/projectcreate")}>Google Cloud console ↗</button>, signed in as your job-search Gmail, and create a project called <strong>Next Chapter</strong>.</li>
        <li>Turn on the <button className="link" onClick={() => link("https://console.cloud.google.com/apis/library/gmail.googleapis.com")}>Gmail API ↗</button> for that project (Enable).</li>
        <li>Set up the <button className="link" onClick={() => link("https://console.cloud.google.com/auth/overview")}>Google Auth Platform ↗</button>: app name <em>Next Chapter</em>, audience <strong>External</strong>, your job-search address as the support and contact email. Under <em>Audience</em>, add that same address as a <strong>test user</strong>.</li>
        <li>Under <button className="link" onClick={() => link("https://console.cloud.google.com/auth/clients")}>Clients ↗</button>, create a client of type <strong>Desktop app</strong>. Copy its client ID and client secret into the boxes below.</li>
        <li>Click <strong>Connect Gmail</strong>. Google will warn that the app isn't verified; that's expected for an app only you use. Choose <em>Continue</em> and tick the Gmail box.</li>
      </ol>
      <p className="muted small">
        While the project's audience is in <em>Testing</em>, Google signs the app out after 7 days and you click Reconnect. To avoid that, set the
        audience to <em>In production</em> (Publish app). You'll still see the unverified-app warning once, and it stays a private app only you use.
      </p>
      <div className="form-grid">
        <Field label={status.configured ? "Client ID — saved in Keychain ✓ (enter again to replace)" : "Client ID"} hint="Ends in .apps.googleusercontent.com">
          <input value={id} onChange={(e) => setId(e.target.value)} autoComplete="off" placeholder="1234…apps.googleusercontent.com" />
        </Field>
        <Field label="Client secret" hint="Stored only in your macOS Keychain">
          <input type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" placeholder="GOCSPX-…" />
        </Field>
        <div className="btn-row"><button className="btn small" disabled={!id.trim() || (!status.configured && !secret.trim())} onClick={save}>Save</button></div>
      </div>
      <div className="btn-row" style={{ marginTop: 12 }}>
        <button className="btn primary" disabled={!status.configured || busy} onClick={connect}>{busy ? "Waiting for Google sign-in…" : status.address ? "Reconnect Gmail" : "Connect Gmail"}</button>
        {!status.configured && <span className="muted small">Save the client ID and secret first.</span>}
      </div>
      {status.lastError && <div className="alert alert-bad small" style={{ marginTop: 10 }}>{status.lastError}</div>}
    </Card>
  );
}

export function Email({ go }: { go: Nav }) {
  const [status, setStatus] = useState<EmailStatus | null>(null);
  const [pending, setPending] = useState<EmailMessage[]>([]);
  const [items, setItems] = useState<EmailMessage[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [filter, setFilter] = useState("all");
  const [checking, setChecking] = useState(false);
  const { fail, notify } = useToast();

  const load = () => {
    api.emailStatus().then(setStatus).catch(fail);
    api.emailList("pending").then(setPending).catch(fail);
    api.emailList(filter).then(setItems).catch(fail);
  };
  useEffect(() => { api.listJobs().then((j) => setJobs(j.filter((x) => !x.isSample))).catch(() => {}); }, []);
  useEffect(load, [filter]);

  if (!status) return <div className="page"><p className="muted">Loading…</p></div>;

  const check = async () => {
    setChecking(true);
    try {
      const s = await api.emailSync();
      notify("ok", `${s.newMessages} new email${s.newMessages === 1 ? "" : "s"}: ${s.jobsAdded} job${s.jobsAdded === 1 ? "" : "s"} added from alerts, ${s.linked} linked to applications, ${s.suggestions} waiting for your OK${s.moreWaiting ? ". More are waiting; check again to continue" : ""}`);
      load();
    } catch (e) { fail(e); load(); } finally { setChecking(false); }
  };
  const disconnect = async () => {
    if (!(await confirmAction("Disconnect Gmail? The app's access is revoked at Google and removed from your Keychain. Emails already stored here stay until you delete them.", "Disconnect"))) return;
    api.emailDisconnect().then((s) => { setStatus(s); notify("ok", "Gmail disconnected"); }).catch(fail);
  };
  const forget = async () => {
    if (!(await confirmAction(`Delete the ${status.total} emails stored in Next Chapter? Your Gmail account isn't touched, and jobs already added stay. The next check reads the last 30 days again.`, "Delete stored emails"))) return;
    api.emailForget().then((n) => { notify("ok", `Removed ${n} stored emails`); load(); }).catch(fail);
  };
  const expired = !!status.lastError && /expired|revoked/i.test(status.lastError);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Email</h1>
          <p className="muted">{status.connected ? <>Read-only monitoring of <strong>{status.address}</strong></> : "Read-only monitoring of your job-search Gmail"}</p>
        </div>
        {status.connected && (
          <div className="head-actions">
            <button className="btn primary" disabled={checking || status.syncing} onClick={check}>{checking || status.syncing ? "Checking…" : "Check now"}</button>
          </div>
        )}
      </div>

      {(!status.connected || expired) && <Setup status={status} onChanged={(s) => { setStatus(s); load(); }} />}

      {status.connected && (
        <Card title="Gmail" actions={status.lastError ? <span className="badge tone-bad">Last check failed</span> : <span className="badge tone-good">✓ Connected, read-only</span>}>
          <div className="stat-row">
            <div><div className="muted small">Last check</div><strong>{status.lastSync || "Not yet"}</strong></div>
            <div><div className="muted small">Emails stored</div><strong>{status.total}</strong></div>
            <div><div className="muted small">Job alerts</div><strong>{status.alerts}</strong></div>
            <div><div className="muted small">Jobs from alerts</div><strong>{status.jobsFromEmail}</strong></div>
            <div><div className="muted small">Linked to applications</div><strong>{status.linked}</strong></div>
          </div>
          {status.lastError && !expired && <div className="alert alert-bad small">{status.lastError}</div>}
          <div className="form-grid">
            <Field label="Check automatically" hint="Only while the app is open">
              <select value={status.intervalMinutes} onChange={(e) => api.emailSetInterval(Number(e.target.value)).then(setStatus).catch(fail)}>
                {INTERVALS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
              </select>
            </Field>
            <div className="btn-row">
              <button className="btn small" onClick={disconnect}>Disconnect</button>
              <button className="btn small danger" disabled={status.total === 0} onClick={forget}>Delete stored emails</button>
            </div>
          </div>
        </Card>
      )}

      <Card title={`Waiting for your OK (${pending.length})`}>
        {pending.length === 0 ? (
          <Empty>No status changes to review. When an employer confirms, invites you to interview or turns you down, the change waits here for you.</Empty>
        ) : pending.map((m) => (
          <div key={m.id} className="list-row suggestion-row">
            <div>
              <strong>{m.subject || "(no subject)"}</strong>
              <div className="muted small">{who(m)} · {m.sentAt.slice(0, 16)}</div>
              <div className="small">
                <button className="link" onClick={() => go({ page: "job", jobId: m.jobId! })}>{m.jobLabel}</button>
                <span className="muted"> · now {m.jobStatus} → suggested <strong>{m.suggestion}</strong></span>
              </div>
              {m.matchNote && <div className="muted small">{m.matchNote}</div>}
            </div>
            <SuggestionActions m={m} onChanged={load} />
          </div>
        ))}
      </Card>

      <Card title="Email activity" actions={
        <div className="btn-row">
          {FILTERS.map(([v, l]) => <button key={v} className={`btn small ${filter === v ? "primary" : ""}`} onClick={() => setFilter(v)}>{l}</button>)}
        </div>
      }>
        {items.length === 0 ? (
          <Empty>{status.connected ? "Nothing here yet. Click Check now." : "Connect Gmail to start."}</Empty>
        ) : (
          <div className="email-list">{items.map((m) => <EmailItem key={m.id} m={m} jobs={jobs} go={go} onChanged={load} />)}</div>
        )}
      </Card>
    </div>
  );
}

/** Email activity for one job (Job detail page). */
export function JobEmailCard({ jobId, go, onChanged }: { jobId: number; go: Nav; onChanged: () => void }) {
  const [items, setItems] = useState<EmailMessage[] | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const load = () => api.emailList(`job:${jobId}`).then(setItems).catch(() => setItems([]));
  useEffect(() => { load(); api.listJobs().then((j) => setJobs(j.filter((x) => !x.isSample))).catch(() => {}); }, [jobId]);
  if (!items || items.length === 0) return null;
  const sent = items.filter((m) => m.direction === "out").length;
  return (
    <Card title={`Email (${items.length - sent} received, ${sent} sent)`}>
      <div className="email-list">
        {items.map((m) => <EmailItem key={m.id} m={m} jobs={jobs} go={go} showJob={false} onChanged={() => { load(); onChanged(); }} />)}
      </div>
    </Card>
  );
}
