import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import type { ImportReport } from "../lib/types";
import { docKindLabel } from "../lib/format";
import { Card, StatusBadge, useToast } from "./ui";

/** Preview-then-import of an existing job-search folder (Command Center layout). */
export function FolderImport({ onImported }: { onImported?: () => void }) {
  const [path, setPath] = useState<string | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const { fail, notify } = useToast();

  const choose = async () => {
    const p = await open({ directory: true, multiple: false, title: "Choose your job-search folder" }).catch(fail);
    if (typeof p === "string") { setPath(p); setReport(null); preview(p); }
  };

  const preview = async (p = path) => {
    if (!p) return;
    setBusy(true);
    try { setReport(await api.importCommandCenter(p, true)); } catch (e) { fail(e); } finally { setBusy(false); }
  };

  const doImport = async () => {
    if (!path) return;
    setBusy(true);
    try {
      const r = await api.importCommandCenter(path, false);
      setReport(r);
      const added = r.jobs.filter((j) => j.action === "added").length;
      notify("ok", `Imported ${added} job${added === 1 ? "" : "s"} and ${r.documents.filter((d) => d.action !== "already imported").length} documents. Your folder was not changed.`);
      onImported?.();
    } catch (e) { fail(e); } finally { setBusy(false); }
  };

  const addSkill = async (name: string) => {
    try {
      await api.saveSkill({ name, category: "", evidence: "Found in master resume; confirmed by me" });
      setReport((r) => r && { ...r, suggestedSkills: r.suggestedSkills.filter((s) => s !== name) });
    } catch (e) { fail(e); }
  };

  const kinds = report
    ? Object.entries(report.documents.reduce<Record<string, number>>((m, d) => {
        if (d.action !== "already imported") m[d.kind] = (m[d.kind] ?? 0) + 1;
        return m;
      }, {}))
    : [];
  const newJobs = report?.jobs.filter((j) => j.action === "added").length ?? 0;
  const newDocs = report?.documents.filter((d) => d.action !== "already imported").length ?? 0;

  return (
    <Card title="Import an existing job-search folder" actions={
      <div className="btn-row">
        <button className="btn" onClick={choose} disabled={busy}>{path ? "Choose another folder…" : "Choose folder…"}</button>
        {path && report?.dryRun && <button className="btn primary" onClick={doImport} disabled={busy || (newJobs === 0 && newDocs === 0)}>Import now</button>}
      </div>
    }>
      <p className="muted small">
        Reads your tracker workbook (Tracker and Interviews sheets), resumes, sent applications, saved postings, certificates, portfolio and reviews.
        You see a full preview first. Files are copied into the app (videos over 25 MB are linked where they are). Your folder is never changed, and running it again only adds what's new.
      </p>
      {path && <div className="small mono">{path}</div>}
      {busy && <p className="muted">Reading files… this can take a few seconds for PDFs.</p>}
      {report && !busy && (
        <>
          <div className={`alert ${report.dryRun ? "alert-info" : "alert-ok"}`}>
            <strong>{report.dryRun ? "Preview — nothing has been saved yet." : "Import complete."}</strong>{" "}
            {newJobs} new job{newJobs === 1 ? "" : "s"}, {newDocs} new document{newDocs === 1 ? "" : "s"}
            {report.trackerFile && <> · tracker: <span className="mono">{report.trackerFile}</span></>}
          </div>

          <div className="table-scroll short">
            <table className="table compact">
              <thead><tr><th>Employer</th><th>Position</th><th>Status</th><th>Applied</th><th>Pay</th><th className="num">Docs</th><th>Posting text</th><th>Checks</th><th /></tr></thead>
              <tbody>
                {report.jobs.map((j) => (
                  <tr key={`${j.employer}-${j.title}`}>
                    <td>{j.employer}</td><td>{j.title}</td><td><StatusBadge status={j.status} /></td>
                    <td className="nowrap">{j.applied ?? "—"}</td><td className="nowrap">{j.salary}</td>
                    <td className="num">{j.linkedDocuments}</td>
                    <td>{j.posting ? "✓ found" : <span className="muted">—</span>}</td>
                    <td className="small">{j.flags.length ? j.flags.map((f, i) => <div key={i}>{f}</div>) : <span className="muted">no issues</span>}</td>
                    <td><span className={`badge ${j.action === "added" ? "tone-good" : "tone-muted"}`}>{j.action === "added" ? (report.dryRun ? "will add" : "added") : "already in app"}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="two-col">
            <div>
              <h3>Documents</h3>
              <ul className="small">{kinds.map(([k, n]) => <li key={k}>{docKindLabel(k)}: {n}</li>)}</ul>
              {report.certifications.length > 0 && <><h3>Certifications</h3><ul className="small">{report.certifications.map((c, i) => <li key={i}>{c}</li>)}</ul></>}
              {report.profileUpdates.length > 0 && <><h3>Profile</h3><ul className="small">{report.profileUpdates.map((c, i) => <li key={i}>{c}</li>)}</ul></>}
            </div>
            <div>
              {report.warnings.length > 0 && <><h3>Needs your attention</h3><ul className="small">{report.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul></>}
              <details className="small"><summary>Skipped ({report.skipped.length})</summary><ul>{report.skipped.map((w, i) => <li key={i}>{w}</li>)}</ul></details>
              <details className="small"><summary>Every file ({report.documents.length})</summary>
                <ul>{report.documents.map((d, i) => <li key={i}><span className="mono">{d.file}</span> — {docKindLabel(d.kind)}{d.job ? ` → ${d.job}` : ""} · {d.action}{d.note ? ` · ${d.note}` : ""}</li>)}</ul>
              </details>
            </div>
          </div>

          {!report.dryRun && report.suggestedSkills.length > 0 && (
            <div className="alert alert-info">
              <strong>Skills found in your master resume.</strong> Add the ones that are accurate. Nothing is added until you click it:
              <div className="chips">{report.suggestedSkills.map((s) => <button key={s} className="chip add" onClick={() => addSkill(s)}>+ {s}</button>)}</div>
            </div>
          )}
        </>
      )}
    </Card>
  );
}
