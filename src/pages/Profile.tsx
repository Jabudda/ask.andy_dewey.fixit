import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import type { Accomplishment, Certification, DocumentRec, Experience, ProfileBundle, Skill } from "../lib/types";
import { shortDate } from "../lib/format";
import { Card, Empty, Field, confirmAction, useToast } from "../components/ui";

const DOC_KINDS: [string, string][] = [
  ["master_resume", "Master resume"],
  ["career_document", "Supporting career document"],
  ["certification", "Certification"],
  ["accomplishment", "Accomplishment example"],
];

export function Profile() {
  const [p, setP] = useState<ProfileBundle | null>(null);
  const [suggested, setSuggested] = useState<string[]>([]);
  const [viewDoc, setViewDoc] = useState<DocumentRec | null>(null);
  const { fail, notify } = useToast();
  const load = () => api.getProfile().then(setP).catch(fail);
  useEffect(() => { load(); }, []);
  if (!p) return <div className="page"><p className="muted">Loading…</p></div>;

  const upload = async (kind: string) => {
    try {
      const path = await open({ multiple: false, filters: [{ name: "Documents", extensions: ["docx", "txt", "md", "pdf"] }] });
      if (!path || Array.isArray(path)) return;
      const r = await api.importDocument(path, kind);
      if (r.warning) notify("info", r.warning);
      else notify("ok", `Imported ${r.document.fileName} (${r.document.extractedText.length.toLocaleString()} characters read)`);
      setSuggested(r.suggestedSkills);
      setViewDoc(r.document);
      load();
    } catch (e) { fail(e); }
  };

  const addSkill = async (name: string) => {
    try { await api.saveSkill({ name, category: "", evidence: "Found in uploaded document; confirmed by me" }); setSuggested((s) => s.filter((x) => x !== name)); load(); } catch (e) { fail(e); }
  };

  const masters = p.documents.filter((d) => d.kind === "master_resume");

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Career profile</h1>
          <p className="muted">Your verified record. Later phases build resumes and cover letters only from what is here — never from guesses.</p>
        </div>
      </div>

      <Card title="Documents" actions={
        <div className="btn-row">{DOC_KINDS.map(([k, l]) => <button key={k} className={`btn small ${k === "master_resume" ? "primary" : ""}`} onClick={() => upload(k)}>Upload {l.toLowerCase()}</button>)}</div>
      }>
        <p className="muted small">.docx, .txt and .md are read in full. PDFs are stored but not yet read (Phase 3) — upload a .docx copy too.</p>
        {p.documents.length === 0 ? <Empty>No documents yet. Start with your master resume.</Empty> : (
          <table className="table">
            <thead><tr><th>File</th><th>Type</th><th className="num">Version</th><th>Added</th><th className="num">Text</th><th /></tr></thead>
            <tbody>
              {p.documents.map((d) => (
                <tr key={d.id}>
                  <td>{d.fileName}{d.kind === "master_resume" && masters[0]?.id === d.id && <span className="tag">current master</span>}</td>
                  <td>{DOC_KINDS.find(([k]) => k === d.kind)?.[1] ?? d.kind}</td>
                  <td className="num">v{d.version}</td>
                  <td>{shortDate(d.createdAt)}</td>
                  <td className="num">{d.extractedText ? `${d.extractedText.length.toLocaleString()} chars` : "—"}</td>
                  <td className="nowrap">
                    {d.extractedText && <button className="link" onClick={() => setViewDoc(d)}>View text</button>}{" "}
                    <button className="link danger" onClick={async () => { if (await confirmAction(`Remove ${d.fileName} from the app?`, "Remove")) api.deleteDocument(d.id).then(load).catch(fail); }}>Remove</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {suggested.length > 0 && (
          <div className="alert alert-info">
            <strong>Skills found in that document.</strong> They are <em>not</em> added until you confirm each one is accurate:
            <div className="chips">{suggested.map((s) => <button key={s} className="chip add" onClick={() => addSkill(s)}>+ {s}</button>)}</div>
          </div>
        )}
        {viewDoc && (
          <details open className="doc-view">
            <summary>{viewDoc.fileName} — extracted text <button className="link" onClick={() => setViewDoc(null)}>Close</button></summary>
            <pre className="description">{viewDoc.extractedText || "(no text could be read)"}</pre>
          </details>
        )}
      </Card>

      <Basics p={p} onSaved={load} />
      <ExperienceSection items={p.experience} onChanged={load} />
      <SkillsSection items={p.skills} onChanged={load} />
      <div className="two-col">
        <CertSection items={p.certifications} onChanged={load} />
        <AccSection items={p.accomplishments} onChanged={load} />
      </div>
    </div>
  );
}

function Basics({ p, onSaved }: { p: ProfileBundle; onSaved: () => void }) {
  const [b, setB] = useState(p.basics);
  const { fail, notify } = useToast();
  useEffect(() => setB(p.basics), [p.basics.updatedAt]);
  const set = (k: keyof typeof b, v: string) => setB({ ...b, [k]: v });
  return (
    <Card title="Contact & summary" actions={<button className="btn small primary" onClick={() => api.saveProfileBasics(b).then(() => { notify("ok", "Profile saved"); onSaved(); }).catch(fail)}>Save</button>}>
      <div className="form-grid">
        <Field label="Full name"><input value={b.fullName} onChange={(e) => set("fullName", e.target.value)} /></Field>
        <Field label="Email"><input value={b.email} onChange={(e) => set("email", e.target.value)} /></Field>
        <Field label="Phone"><input value={b.phone} onChange={(e) => set("phone", e.target.value)} /></Field>
        <Field label="Location"><input value={b.location} onChange={(e) => set("location", e.target.value)} /></Field>
        <Field label="LinkedIn URL"><input value={b.linkedinUrl} onChange={(e) => set("linkedinUrl", e.target.value)} /></Field>
        <Field label="Headline"><input value={b.headline} placeholder="Operations & data professional" onChange={(e) => set("headline", e.target.value)} /></Field>
        <Field label="Professional summary" wide><textarea rows={4} value={b.summary} onChange={(e) => set("summary", e.target.value)} /></Field>
      </div>
    </Card>
  );
}

function ExperienceSection({ items, onChanged }: { items: Experience[]; onChanged: () => void }) {
  const blank: Experience = { employer: "", title: "", location: "", startDate: "", endDate: "", isCurrent: false, achievements: "", sortOrder: 0 };
  const [x, setX] = useState<Experience | null>(null);
  const { fail } = useToast();
  const save = () => x && api.saveExperience(x).then(() => { setX(null); onChanged(); }).catch(fail);
  return (
    <Card title="Verified work history" actions={<button className="btn small" onClick={() => setX(blank)}>+ Add position</button>}>
      <p className="muted small">Enter titles, employers and dates exactly as they should appear. Write one achievement per line — only things you actually did.</p>
      {items.length === 0 && !x && <Empty>No positions yet.</Empty>}
      {items.map((e) => (
        <div key={e.id} className="exp">
          <div className="list-row">
            <div><strong>{e.title}</strong> · {e.employer}<div className="muted small">{e.startDate} – {e.isCurrent ? "Present" : e.endDate}{e.location && ` · ${e.location}`}</div></div>
            <span className="btn-row">
              <button className="link" onClick={() => setX(e)}>Edit</button>
              <button className="link danger" onClick={async () => { if (await confirmAction(`Delete ${e.title} at ${e.employer}?`, "Delete")) api.deleteProfileItem("experience", e.id!).then(onChanged).catch(fail); }}>Delete</button>
            </span>
          </div>
          {e.achievements && <ul className="bullets">{e.achievements.split("\n").filter(Boolean).map((a, i) => <li key={i}>{a.replace(/^[•\-*]\s*/, "")}</li>)}</ul>}
        </div>
      ))}
      {x && (
        <div className="editor">
          <div className="form-grid">
            <Field label="Job title *"><input value={x.title} onChange={(e) => setX({ ...x, title: e.target.value })} /></Field>
            <Field label="Employer *"><input value={x.employer} onChange={(e) => setX({ ...x, employer: e.target.value })} /></Field>
            <Field label="Location"><input value={x.location} onChange={(e) => setX({ ...x, location: e.target.value })} /></Field>
            <Field label="Start (YYYY-MM)"><input value={x.startDate} placeholder="2019-06" onChange={(e) => setX({ ...x, startDate: e.target.value })} /></Field>
            <Field label="End (YYYY-MM)"><input value={x.endDate} disabled={x.isCurrent} placeholder="2027-01" onChange={(e) => setX({ ...x, endDate: e.target.value })} /></Field>
            <label className="check"><input type="checkbox" checked={x.isCurrent} onChange={(e) => setX({ ...x, isCurrent: e.target.checked })} /> Current position</label>
            <Field label="Achievements (one per line)" wide><textarea rows={7} value={x.achievements} onChange={(e) => setX({ ...x, achievements: e.target.value })} /></Field>
          </div>
          <div className="btn-row"><button className="btn primary small" onClick={save}>Save position</button><button className="btn small" onClick={() => setX(null)}>Cancel</button></div>
        </div>
      )}
    </Card>
  );
}

function SkillsSection({ items, onChanged }: { items: Skill[]; onChanged: () => void }) {
  const [name, setName] = useState("");
  const [category, setCategory] = useState("");
  const { fail } = useToast();
  const add = () => api.saveSkill({ name, category, evidence: "" }).then(() => { setName(""); onChanged(); }).catch(fail);
  return (
    <Card title={`Verified skills & tools (${items.length})`}>
      <div className="chips">
        {items.map((s) => (
          <span key={s.id} className="chip" title={s.evidence || undefined}>
            {s.name}{s.category && <span className="muted"> · {s.category}</span>}
            <button aria-label={`Remove ${s.name}`} onClick={() => api.deleteProfileItem("skills", s.id!).then(onChanged).catch(fail)}>×</button>
          </span>
        ))}
        {items.length === 0 && <span className="muted">None yet — add skills or confirm suggestions from your resume.</span>}
      </div>
      <div className="inline-form">
        <input placeholder="Skill or tool, e.g. Smartsheet" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && name.trim() && add()} />
        <input placeholder="Category (optional)" value={category} onChange={(e) => setCategory(e.target.value)} />
        <button className="btn small" disabled={!name.trim()} onClick={add}>Add skill</button>
      </div>
    </Card>
  );
}

function CertSection({ items, onChanged }: { items: Certification[]; onChanged: () => void }) {
  const blank: Certification = { name: "", issuer: "", dateEarned: "", expires: "", credentialId: "" };
  const [x, setX] = useState(blank);
  const { fail } = useToast();
  return (
    <Card title="Certifications">
      {items.map((c) => (
        <div key={c.id} className="list-row">
          <div><strong>{c.name}</strong>{c.issuer && ` · ${c.issuer}`}<div className="muted small">{[c.dateEarned && `Earned ${c.dateEarned}`, c.expires && `Expires ${c.expires}`, c.credentialId && `ID ${c.credentialId}`].filter(Boolean).join(" · ")}</div></div>
          <button className="link danger" onClick={() => api.deleteProfileItem("certifications", c.id!).then(onChanged).catch(fail)}>Remove</button>
        </div>
      ))}
      <div className="form-grid">
        <Field label="Name"><input value={x.name} onChange={(e) => setX({ ...x, name: e.target.value })} /></Field>
        <Field label="Issuer"><input value={x.issuer} onChange={(e) => setX({ ...x, issuer: e.target.value })} /></Field>
        <Field label="Earned"><input type="month" value={x.dateEarned} onChange={(e) => setX({ ...x, dateEarned: e.target.value })} /></Field>
        <Field label="Expires"><input type="month" value={x.expires} onChange={(e) => setX({ ...x, expires: e.target.value })} /></Field>
      </div>
      <button className="btn small" disabled={!x.name.trim()} onClick={() => api.saveCertification(x).then(() => { setX(blank); onChanged(); }).catch(fail)}>Add certification</button>
    </Card>
  );
}

function AccSection({ items, onChanged }: { items: Accomplishment[]; onChanged: () => void }) {
  const blank: Accomplishment = { title: "", description: "", evidence: "" };
  const [x, setX] = useState(blank);
  const { fail } = useToast();
  return (
    <Card title="Accomplishment examples">
      {items.map((a) => (
        <div key={a.id} className="list-row">
          <div><strong>{a.title}</strong>{a.description && <div className="small">{a.description}</div>}{a.evidence && <div className="muted small">Evidence: {a.evidence}</div>}</div>
          <button className="link danger" onClick={() => api.deleteProfileItem("accomplishments", a.id!).then(onChanged).catch(fail)}>Remove</button>
        </div>
      ))}
      <div className="form-grid">
        <Field label="Title" wide><input value={x.title} placeholder="Automated weekly incident report" onChange={(e) => setX({ ...x, title: e.target.value })} /></Field>
        <Field label="What you did and the result" wide><textarea rows={3} value={x.description} onChange={(e) => setX({ ...x, description: e.target.value })} /></Field>
        <Field label="Evidence (optional)" wide hint="Where this can be verified, e.g. performance review, email, report"><input value={x.evidence} onChange={(e) => setX({ ...x, evidence: e.target.value })} /></Field>
      </div>
      <button className="btn small" disabled={!x.title.trim()} onClick={() => api.saveAccomplishment(x).then(() => { setX(blank); onChanged(); }).catch(fail)}>Add accomplishment</button>
    </Card>
  );
}
