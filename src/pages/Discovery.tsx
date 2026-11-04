import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import type { JobSource, RunSummary, SearchRun, SearchSettings } from "../lib/types";
import { Card, Empty, Field, confirmAction, useToast } from "../components/ui";
import type { Nav } from "../App";

const KIND_LABEL: Record<string, string> = {
  greenhouse: "Greenhouse board", lever: "Lever board", ashby: "Ashby board",
  remotive: "Remotive", himalayas: "Himalayas", jobicy: "Jobicy", usajobs: "USAJOBS (federal)",
};
const KEYWORD = ["remotive", "himalayas", "jobicy"];

export function Discovery({ go }: { go: Nav }) {
  const [sources, setSources] = useState<JobSource[]>([]);
  const [runs, setRuns] = useState<SearchRun[]>([]);
  const [settings, setSettings] = useState<SearchSettings | null>(null);
  const [results, setResults] = useState<RunSummary[] | null>(null);
  const [running, setRunning] = useState<number | "all" | null>(null);
  const [url, setUrl] = useState("");
  const [draft, setDraft] = useState<JobSource>({ kind: "greenhouse", identifier: "", label: "", enabled: true });
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [fedEmail, setFedEmail] = useState("");
  const { fail, notify } = useToast();

  const usajobs = sources.find((s) => s.kind === "usajobs");
  const loadKey = () => api.credentialStatus("usajobs").then(setHasKey).catch(() => setHasKey(false));
  useEffect(() => { loadKey(); }, []);
  useEffect(() => { if (usajobs && !fedEmail) setFedEmail(usajobs.identifier); }, [usajobs?.identifier]);
  const connectUsajobs = async () => {
    try {
      if (keyInput.trim()) await api.saveCredential("usajobs", keyInput.trim());
      await api.saveSource({ id: usajobs?.id, kind: "usajobs", identifier: fedEmail, label: "USAJOBS — federal jobs, remote & open to the public", enabled: true });
      setKeyInput("");
      notify("ok", "USAJOBS connected. The key is stored in your macOS Keychain.");
      loadKey(); load();
    } catch (e) { fail(e); }
  };
  const removeKey = async () => {
    if (!(await confirmAction("Remove the USAJOBS API key from your Keychain? USAJOBS searches will stop until you add it again.", "Remove"))) return;
    try { await api.deleteCredential("usajobs"); loadKey(); } catch (e) { fail(e); }
  };

  const load = () => Promise.all([api.listSources(), api.listSearchRuns(), api.getSearchSettings()])
    .then(([s, r, st]) => { setSources(s); setRuns(r); setSettings(st); }).catch(fail);
  useEffect(() => { load(); }, []);

  const run = async (id?: number) => {
    setRunning(id ?? "all");
    setResults(null);
    try {
      const r = await api.runDiscovery(id);
      setResults(r);
      const n = r.reduce((a, x) => a + x.newJobs, 0);
      const errs = r.filter((x) => x.error).length;
      notify(errs ? "err" : "ok", `${n} new job${n === 1 ? "" : "s"} found${errs ? ` · ${errs} source${errs === 1 ? "" : "s"} failed` : ""}`);
      load();
    } catch (e) { fail(e); } finally { setRunning(null); }
  };

  const detect = async () => {
    try {
      const hit = await api.detectSource(url);
      if (!hit) { notify("info", "That link isn't a Greenhouse, Lever or Ashby board. Choose the type and enter the board name below."); return; }
      setDraft({ ...draft, kind: hit[0], identifier: hit[1] });
      notify("ok", `Recognised a ${KIND_LABEL[hit[0]]}: “${hit[1]}”`);
    } catch (e) { fail(e); }
  };

  const add = async () => {
    try {
      await api.saveSource(draft);
      setDraft({ kind: "greenhouse", identifier: "", label: "", enabled: true });
      setUrl("");
      notify("ok", "Source added. Run it to test the connection.");
      load();
    } catch (e) { fail(e); }
  };

  const toggle = (s: JobSource) => api.saveSource({ ...s, enabled: !s.enabled }).then(load).catch(fail);
  const remove = async (s: JobSource) => {
    if (await confirmAction(`Remove ${s.label || s.identifier}? Jobs already found are kept.`, "Remove")) api.deleteSource(s.id!).then(load).catch(fail);
  };
  const setInterval = (h: number) => api.setSearchInterval(h).then(() => { notify("ok", h ? `Searching every ${h} hours while the app is open` : "Scheduled searches off"); load(); }).catch(fail);

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Job search</h1>
          <p className="muted">Searches public job boards and the career boards of employers you choose. Only documented public APIs are used. There's no logging in and no scraping, and pages that block automated access are skipped.</p>
        </div>
        <button className="btn primary" disabled={running !== null} onClick={() => run()}>{running === "all" ? "Searching…" : "Search all sources now"}</button>
      </div>

      {results && (
        <Card title="Latest search">
          <table className="table compact">
            <thead><tr><th>Source</th><th className="num">Found</th><th className="num">New</th><th className="num">Not relevant / old</th><th className="num">Failed requirements</th><th className="num">Duplicates</th><th>Status</th></tr></thead>
            <tbody>
              {results.map((r) => (
                <tr key={r.sourceId}>
                  <td>{r.label}</td><td className="num">{r.found}</td><td className="num strong">{r.newJobs}</td><td className="num">{r.filtered}</td>
                  <td className="num">{r.excluded}</td><td className="num">{r.duplicates}</td>
                  <td>{r.error ? <span className="badge tone-bad">✕ {r.error}</span> : r.skippedReason ? <span className="muted small">{r.skippedReason}</span> : <span className="badge tone-good">✓ ok</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {results.some((r) => r.newTitles.length) && (
            <div className="small">
              <strong>New:</strong> {results.flatMap((r) => r.newTitles).slice(0, 12).join(" · ")}{" "}
              <button className="link" onClick={() => go({ page: "jobs", status: "Discovered" })}>Review them</button>
            </div>
          )}
        </Card>
      )}

      <div className="two-col">
        <Card title="Schedule">
          <Field label="Search automatically while the app is open" hint="Runs in the background every N hours. Public boards are checked at most every 4 hours. Searches run only while the app is open.">
            <select value={settings?.intervalHours ?? 12} onChange={(e) => setInterval(Number(e.target.value))}>
              <option value={0}>Off — manual only</option>
              <option value={6}>Every 6 hours</option>
              <option value={12}>Every 12 hours</option>
              <option value={24}>Daily</option>
              <option value={48}>Every 2 days</option>
            </select>
          </Field>
          <p className="muted small">Last scheduled search: {settings?.lastScheduled ?? "not yet"}. Searches use your <button className="link" onClick={() => go({ page: "prefs" })}>Preferences</button>: target titles, keywords, search queries, maximum posting age, and the mandatory rules. Postings that clearly fail those rules are counted but not saved.</p>
        </Card>
        <Card title="Watch an employer's job board">
          <Field label="Paste a link to any job at the company" hint="Works for employers that use Greenhouse, Lever or Ashby. Many companies do (e.g. Smartsheet, Vetcove, Harvey, Civitech).">
            <div className="inline-form">
              <input className="grow" placeholder="https://job-boards.greenhouse.io/smartsheet/jobs/…" value={url} onChange={(e) => setUrl(e.target.value)} />
              <button className="btn small" disabled={!url} onClick={detect}>Recognise</button>
            </div>
          </Field>
          <div className="form-grid">
            <Field label="Type">
              <select value={draft.kind} onChange={(e) => setDraft({ ...draft, kind: e.target.value })}>
                {Object.entries(KIND_LABEL).filter(([k]) => !KEYWORD.includes(k)).map(([k, l]) => <option key={k} value={k}>{l}</option>)}
              </select>
            </Field>
            <Field label="Board name"><input value={draft.identifier} placeholder="smartsheet" onChange={(e) => setDraft({ ...draft, identifier: e.target.value })} /></Field>
            <Field label="Company name (shown on jobs)"><input value={draft.label} placeholder="Smartsheet" onChange={(e) => setDraft({ ...draft, label: e.target.value })} /></Field>
          </div>
          <button className="btn small primary" disabled={!draft.identifier.trim()} onClick={add}>Add source</button>
        </Card>
      </div>

      <Card title="USAJOBS (federal jobs)" actions={hasKey ? <span className="badge tone-good">✓ Key saved in Keychain</span> : <span className="badge tone-warn">! Not connected</span>}>
        <p className="muted small">Searches remote federal postings that are open to the public, using your own free USAJOBS API key. The key is kept in the macOS Keychain and is never shown again, stored in the database, exported or backed up. USAJOBS requires your registered email with every request. The first search may ask you to allow access to the Keychain; choose “Always Allow”.</p>
        <div className="form-grid">
          <Field label="Email you registered the key with"><input value={fedEmail} placeholder="you@example.com" onChange={(e) => setFedEmail(e.target.value)} /></Field>
          <Field label={hasKey ? "Replace API key (optional)" : "API key"}><input type="password" autoComplete="off" value={keyInput} onChange={(e) => setKeyInput(e.target.value)} /></Field>
        </div>
        <div className="btn-row">
          <button className="btn small primary" disabled={!fedEmail.includes("@") || (!hasKey && !keyInput.trim())} onClick={connectUsajobs}>{usajobs ? "Save" : "Connect USAJOBS"}</button>
          {hasKey && <button className="btn small danger" onClick={removeKey}>Remove key</button>}
          <button className="link small" onClick={() => openUrl("https://developer.usajobs.gov/apirequest/").catch(fail)}>Get a free key ↗</button>
        </div>
      </Card>

      <Card title={`Sources (${sources.length})`}>
        {sources.length === 0 ? <Empty>No sources yet.</Empty> : (
          <table className="table">
            <thead><tr><th>On</th><th>Source</th><th>Type</th><th>Last checked</th><th>Status</th><th /></tr></thead>
            <tbody>
              {sources.map((s) => (
                <tr key={s.id}>
                  <td><input type="checkbox" checked={s.enabled} onChange={() => toggle(s)} aria-label={`Enable ${s.label}`} /></td>
                  <td>{s.label || s.identifier}{!KEYWORD.includes(s.kind) && <div className="muted small mono">{s.identifier}</div>}</td>
                  <td>{KIND_LABEL[s.kind] ?? s.kind}{KEYWORD.includes(s.kind) && <div className="muted small">keyword search</div>}</td>
                  <td className="nowrap">{s.lastRunAt ?? <span className="muted">never</span>}</td>
                  <td>{s.lastStatus === "error" ? <span className="badge tone-bad" title={s.lastError ?? ""}>✕ {s.lastError?.slice(0, 60)}</span> : s.lastStatus === "ok" ? <span className="badge tone-good">✓ ok</span> : <span className="muted">—</span>}</td>
                  <td className="nowrap">
                    <button className="link" disabled={running !== null} onClick={() => run(s.id!)}>{running === s.id ? "Running…" : "Run"}</button>{" "}
                    <button className="link danger" onClick={() => remove(s)}>Remove</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="muted small">Listings from Jobicy are credited to Jobicy, and their apply buttons go to the original posting. Remotive's free API currently returns only its latest few postings and ignores search terms.</p>
      </Card>

      <Card title="Search history">
        {runs.length === 0 ? <Empty>No searches yet.</Empty> : (
          <div className="table-scroll short">
            <table className="table compact">
              <thead><tr><th>Started</th><th>Source</th><th>Trigger</th><th className="num">Found</th><th className="num">New</th><th className="num">Filtered</th><th className="num">Failed rules</th><th className="num">Dupes</th><th>Result</th></tr></thead>
              <tbody>
                {runs.map((r) => (
                  <tr key={r.id} className={r.error ? "log-error" : ""}>
                    <td className="nowrap mono">{r.startedAt}</td>
                    <td>{sources.find((s) => s.id === r.sourceId)?.label ?? r.connector}</td>
                    <td>{r.trigger}</td>
                    <td className="num">{r.found}</td><td className="num">{r.newJobs}</td><td className="num">{r.filtered}</td><td className="num">{r.excluded}</td><td className="num">{r.duplicates}</td>
                    <td>{r.error ? <span className="badge tone-bad">✕ {r.error}</span> : <span className="badge tone-good">✓</span>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
