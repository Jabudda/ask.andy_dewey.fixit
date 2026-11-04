import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import type { AppInfo, LogEntry } from "../lib/types";
import { Card, confirmAction, useToast } from "../components/ui";

const stamp = () => new Date().toISOString().slice(0, 10);

export function DataLogs() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [q, setQ] = useState("");
  const { fail, notify } = useToast();

  const loadLogs = (query = q) => api.listLogs(query).then(setLogs).catch(fail);
  useEffect(() => { api.appInfo().then(setInfo).catch(fail); loadLogs(""); }, []);

  const exportAs = async (format: "csv" | "xlsx") => {
    try {
      const path = await save({ defaultPath: `next-chapter-applications-${stamp()}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
      if (!path) return;
      const n = await api.exportJobs(path, format);
      notify("ok", `Exported ${n} jobs`);
      loadLogs();
    } catch (e) { fail(e); }
  };

  const backup = async () => {
    try {
      const path = await save({ defaultPath: `next-chapter-backup-${stamp()}.sqlite`, filters: [{ name: "SQLite database", extensions: ["sqlite"] }] });
      if (!path) return;
      await api.backupDatabase(path);
      notify("ok", "Backup saved. Keep it somewhere private — it contains your personal data.");
      loadLogs();
    } catch (e) { fail(e); }
  };

  const restore = async () => {
    try {
      const path = await open({ multiple: false, filters: [{ name: "SQLite database", extensions: ["sqlite", "db"] }] });
      if (!path || Array.isArray(path)) return;
      if (!(await confirmAction("Replace ALL current data with this backup? A safety copy of your current data is saved first.", "Restore"))) return;
      const safety = await api.restoreDatabase(path);
      notify("ok", `Restored. Previous data saved to ${safety}`);
      loadLogs();
    } catch (e) { fail(e); }
  };

  const sample = async (load: boolean) => {
    try {
      if (load) {
        const n = await api.loadSampleData();
        notify("ok", n ? `Loaded ${n} sample jobs (employers marked “SAMPLE”)` : "Sample data is already loaded");
      } else {
        if (!(await confirmAction("Remove all sample jobs? Your real data is not touched.", "Remove"))) return;
        notify("ok", `Removed ${await api.removeSampleData()} sample jobs`);
      }
      loadLogs();
    } catch (e) { fail(e); }
  };

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Data &amp; logs</h1>
          <p className="muted">Everything is stored locally on this Mac. Nothing is uploaded.</p>
        </div>
      </div>

      <div className="two-col">
        <Card title="Export application history">
          <p className="muted small">All jobs with status, pay, eligibility, submission confirmation, interviews and notes.</p>
          <div className="btn-row">
            <button className="btn" onClick={() => exportAs("xlsx")}>Export Excel (.xlsx)</button>
            <button className="btn" onClick={() => exportAs("csv")}>Export CSV</button>
          </div>
        </Card>
        <Card title="Backup & restore">
          <p className="muted small">A backup is a single file containing your whole database (profile, jobs, history, document text).</p>
          <div className="btn-row">
            <button className="btn primary" onClick={backup}>Back up now</button>
            <button className="btn" onClick={restore}>Restore from backup…</button>
          </div>
        </Card>
      </div>

      <div className="two-col">
        <Card title="Sample data">
          <p className="muted small">Nine fictional postings (employer names end in “(SAMPLE)”) to explore the app. They are flagged in the database and can be removed in one click.</p>
          <div className="btn-row">
            <button className="btn" onClick={() => sample(true)}>Load sample data</button>
            <button className="btn danger" onClick={() => sample(false)}>Remove sample data</button>
          </div>
        </Card>
        <Card title="Storage">
          {info && (
            <dl className="kv">
              <dt>Version</dt><dd>{info.version}</dd>
              <dt>Database</dt><dd className="mono">{info.databasePath}</dd>
              <dt>Documents</dt><dd className="mono">{info.documentsDir}</dd>
            </dl>
          )}
        </Card>
      </div>

      <Card title="Application log" actions={
        <div className="inline-form">
          <input placeholder="Search logs…" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.key === "Enter" && loadLogs()} />
          <button className="btn small" onClick={() => loadLogs()}>Search</button>
        </div>
      }>
        <div className="table-scroll short">
          <table className="table compact">
            <thead><tr><th>Time</th><th>Level</th><th>Area</th><th>Message</th></tr></thead>
            <tbody>
              {logs.map((l) => (
                <tr key={l.id} className={`log-${l.level}`}>
                  <td className="nowrap mono">{l.ts}</td>
                  <td><span className={`badge tone-${l.level === "error" ? "bad" : l.level === "warn" ? "warn" : "neutral"}`}>{l.level}</span></td>
                  <td>{l.source}</td>
                  <td>{l.message}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}
