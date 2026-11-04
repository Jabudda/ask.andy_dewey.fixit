import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import type { CoverLetterContent, DocumentRec, Finding, Line, PreparedGeneration, ProfileBundle, ResumeContent } from "../lib/types";
import { Card, Field, confirmAction, useToast } from "./ui";

type Kind = "resume" | "cover_letter";
const DOC_KIND: Record<Kind, string> = { resume: "generated_resume", cover_letter: "generated_cover_letter" };
const LABEL: Record<Kind, string> = { resume: "Tailored resume", cover_letter: "Cover letter" };
const USER = "USER";

/** Keeps citations for lines that still exist; lines the user adds are marked as their own. */
export function mergeLines(oldLines: Line[], texts: string[]): Line[] {
  return texts
    .map((t) => t.trim())
    .filter(Boolean)
    .map((text, i) => {
      const same = oldLines.find((l) => l.text.trim() === text);
      if (same) return { text, sources: same.sources };
      const atIndex = oldLines[i];
      return atIndex && atIndex.sources.length && !atIndex.sources.includes(USER) ? { text, sources: atIndex.sources } : { text, sources: [USER] };
    });
}

function FindingList({ findings }: { findings: Finding[] }) {
  if (!findings.length) return <p className="small tone-text-good">✓ Every line traces to your verified profile.</p>;
  const blockers = findings.filter((f) => f.severity === "blocker");
  const review = findings.filter((f) => f.severity !== "blocker");
  return (
    <div className="small">
      {blockers.length > 0 && <div className="alert alert-bad"><strong>✕ {blockers.length} line{blockers.length > 1 ? "s" : ""} can't be approved yet</strong> — edit or remove them:
        <ul>{blockers.map((f, i) => <li key={i}><em>{f.location}</em>: {f.message}<div className="muted">“{f.text}”</div></li>)}</ul></div>}
      {review.length > 0 && <div className="alert alert-warn"><strong>! {review.length} to review</strong>
        <ul>{review.map((f, i) => <li key={i}><em>{f.location}</em>: {f.message}</li>)}</ul></div>}
    </div>
  );
}

export function DocumentsPanel({ jobId, documents, onChanged }: { jobId: number; documents: DocumentRec[]; onChanged: () => void }) {
  const [profile, setProfile] = useState<ProfileBundle | null>(null);
  const [aiOn, setAiOn] = useState(false);
  const { fail } = useToast();
  useEffect(() => {
    api.getProfile().then(setProfile).catch(fail);
    api.getAiSettings().then((s) => setAiOn(s.enabled)).catch(() => setAiOn(false));
  }, []);
  const master = profile?.documents.find((d) => d.kind === "master_resume")?.extractedText ?? "";
  return (
    <Card title="Application documents">
      {!aiOn && <p className="muted small">Turn on AI writing in Preferences to generate tailored documents. Your existing documents for this job are listed below.</p>}
      {(["resume", "cover_letter"] as Kind[]).map((k) => (
        <KindSection key={k} kind={k} jobId={jobId} aiOn={aiOn} profile={profile} master={master}
          versions={documents.filter((d) => d.kind === DOC_KIND[k]).sort((a, b) => b.version - a.version)} onChanged={onChanged} />
      ))}
    </Card>
  );
}

function KindSection({ kind, jobId, aiOn, profile, master, versions, onChanged }: {
  kind: Kind; jobId: number; aiOn: boolean; profile: ProfileBundle | null; master: string; versions: DocumentRec[]; onChanged: () => void;
}) {
  const [prep, setPrep] = useState<PreparedGeneration | null>(null);
  const [busy, setBusy] = useState(false);
  const [notes, setNotes] = useState("");
  const [mode, setMode] = useState<"none" | "edit" | "compare">("none");
  const { fail, notify } = useToast();
  const latest = versions[0];
  const blockers = latest?.claimReport.filter((f) => f.severity === "blocker").length ?? 0;

  const start = async () => {
    try { setPrep(await api.prepareGeneration(jobId, kind)); } catch (e) { fail(e); }
  };
  const generate = async () => {
    setBusy(true);
    try {
      const g = await api.generateDocument(jobId, kind);
      setPrep(null);
      setNotes(g.notesForCandidate);
      notify("ok", `${LABEL[kind]} v${g.document.version} written · ≈ $${g.costUsd.toFixed(2)}${g.servedByFallback ? " (served by fallback model)" : ""}`);
      onChanged();
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const approve = async () => {
    if (!latest) return;
    const needsReview = latest.claimReport.some((f) => f.severity !== "blocker");
    if (needsReview && !(await confirmAction("I've read the flagged items and this document is accurate.", "Approve"))) return;
    try { await api.approveDocument(latest.id, needsReview); notify("ok", `${LABEL[kind]} approved`); onChanged(); } catch (e) { fail(e); }
  };

  return (
    <div className="doc-kind">
      <div className="list-row">
        <div>
          <strong>{LABEL[kind]}</strong>{" "}
          {latest ? <>
            <span className={`badge tone-${latest.status === "approved" ? "good" : "warn"}`}>{latest.status === "approved" ? "✓ Approved" : "Draft"} · v{latest.version}</span>
            {blockers > 0 && <span className="badge tone-bad">✕ {blockers} blocked</span>}
          </> : <span className="muted small">none yet</span>}
        </div>
        <div className="btn-row">
          {latest && <>
            <button className="link" onClick={() => api.openDocument(latest.id).catch(fail)}>Word</button>
            <button className="link" onClick={() => api.openDocumentPdf(latest.id).catch(fail)}>PDF</button>
            <button className="link" onClick={() => setMode(mode === "edit" ? "none" : "edit")}>Edit</button>
            {kind === "resume" && <button className="link" onClick={() => setMode(mode === "compare" ? "none" : "compare")}>Compare</button>}
            {latest.status !== "approved" && <button className="btn small primary" disabled={blockers > 0} onClick={approve}>Approve</button>}
          </>}
          <button className="btn small" disabled={!aiOn || busy} onClick={start}>{latest ? "Regenerate" : "Generate"}</button>
        </div>
      </div>

      {prep && (
        <div className="editor small">
          <div><strong>Ready to write with {prep.model}</strong> (effort {prep.effort}) · {prep.factCount} verified facts · estimated ≈ ${prep.estimatedUsd.toFixed(2)}</div>
          <details><summary>See exactly what will be sent</summary>
            <pre className="description mono">{prep.system}{"\n\n────────\n\n"}{prep.user}</pre></details>
          <div className="btn-row">
            <button className="btn small primary" disabled={busy} onClick={generate}>{busy ? "Writing… usually 30–90 seconds" : "Send and write"}</button>
            <button className="btn small" disabled={busy} onClick={() => setPrep(null)}>Cancel</button>
          </div>
        </div>
      )}

      {notes && <p className="small muted"><strong>Writer's notes:</strong> {notes}</p>}
      {latest && <FindingList findings={latest.claimReport} />}
      {latest && mode === "edit" && profile && <Editor kind={kind} doc={latest} profile={profile} onSaved={() => { setMode("none"); onChanged(); }} />}
      {latest && mode === "compare" && <Compare master={master} tailored={latest.extractedText} />}
      {versions.length > 1 && <p className="muted small">Versions: {versions.map((v) => `v${v.version} (${v.status})`).join(" · ")}</p>}
    </div>
  );
}

function Editor({ kind, doc, profile, onSaved }: { kind: Kind; doc: DocumentRec; profile: ProfileBundle; onSaved: () => void }) {
  const [c, setC] = useState<any>(doc.contentJson);
  const { fail, notify } = useToast();
  if (!c) return <p className="muted small">This version has no editable content.</p>;
  const save = async () => {
    try {
      const g = await api.saveDocumentEdit(doc.id, c);
      notify(g.findings.some((f) => f.severity === "blocker") ? "info" : "ok", `Saved as v${g.document.version} and re-checked`);
      onSaved();
    } catch (e) { fail(e); }
  };
  if (kind === "resume") {
    const r = c as ResumeContent;
    const posLabel = (id: string) => { const e = profile.experience[Number(id.slice(1))]; return e ? `${e.title} — ${e.employer}` : id; };
    return (
      <div className="editor">
        <Field label="Headline"><input value={r.headline} onChange={(e) => setC({ ...r, headline: e.target.value })} /></Field>
        <Field label="Summary"><textarea rows={3} value={r.summary.text} onChange={(e) => setC({ ...r, summary: mergeLines([r.summary], [e.target.value])[0] ?? { text: "", sources: [] } })} /></Field>
        <Field label="Core skills (comma-separated, must be in your verified skills)">
          <input value={r.skills.join(", ")} onChange={(e) => setC({ ...r, skills: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} />
        </Field>
        {r.positions.map((p, i) => (
          <div key={p.position_id} className="field">
            <label className="check"><input type="checkbox" checked={p.include} onChange={(e) => {
              const positions = [...r.positions]; positions[i] = { ...p, include: e.target.checked }; setC({ ...r, positions });
            }} /> <strong>{posLabel(p.position_id)}</strong></label>
            {p.include && <textarea rows={Math.max(2, p.bullets.length + 1)} value={p.bullets.map((b) => b.text).join("\n")} onChange={(e) => {
              const positions = [...r.positions]; positions[i] = { ...p, bullets: mergeLines(p.bullets, e.target.value.split("\n")) }; setC({ ...r, positions });
            }} />}
          </div>
        ))}
        <p className="muted small">One bullet per line. Lines you add yourself are marked “written by you” for a final check.</p>
        <button className="btn small primary" onClick={save}>Save as new version</button>
      </div>
    );
  }
  const l = c as CoverLetterContent;
  return (
    <div className="editor">
      <Field label="Greeting"><input value={l.greeting} onChange={(e) => setC({ ...l, greeting: e.target.value })} /></Field>
      <Field label="Body (blank line between paragraphs)">
        <textarea rows={14} value={l.paragraphs.map((p) => p.text).join("\n\n")} onChange={(e) => setC({ ...l, paragraphs: mergeLines(l.paragraphs, e.target.value.split(/\n\s*\n/)) })} />
      </Field>
      <Field label="Closing"><input value={l.closing} onChange={(e) => setC({ ...l, closing: e.target.value })} /></Field>
      <p className="muted small">{l.paragraphs.reduce((n, p) => n + p.text.split(/\s+/).filter(Boolean).length, 0)} words</p>
      <button className="btn small primary" onClick={save}>Save as new version</button>
    </div>
  );
}

function Compare({ master, tailored }: { master: string; tailored: string }) {
  const masterLines = useMemo(() => new Set(master.split("\n").map((l) => l.trim().replace(/^[•\-*]\s*/, "").toLowerCase()).filter(Boolean)), [master]);
  if (!master) return <p className="muted small">Upload a master resume in Career profile to compare.</p>;
  return (
    <div className="compare">
      <div><h3>Master resume</h3><pre className="description">{master}</pre></div>
      <div><h3>Tailored version <span className="muted small">(changed or new lines highlighted)</span></h3>
        <pre className="description">{tailored.split("\n").map((l, i) => {
          const changed = l.trim() && !masterLines.has(l.trim().replace(/^[•\-*]\s*/, "").toLowerCase());
          return <span key={i} className={changed ? "diff-new" : ""}>{l}{"\n"}</span>;
        })}</pre>
      </div>
    </div>
  );
}
