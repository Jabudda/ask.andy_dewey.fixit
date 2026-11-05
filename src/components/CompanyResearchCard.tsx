import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../lib/api";
import type { CompanyResearch, CompanyTerm } from "../lib/types";
import { shortDate } from "../lib/format";
import { Card, useToast } from "./ui";

const GROUPS: { kind: string; label: string }[] = [
  { kind: "mission", label: "Mission" },
  { kind: "vision", label: "Vision" },
  { kind: "value", label: "Values" },
  { kind: "tagline", label: "How they describe themselves" },
  { kind: "phrase", label: "Phrases they repeat" },
  { kind: "yours", label: "Added by you" },
];

/** Same terms, same choices: nothing to save. */
export function sameTerms(a: CompanyTerm[], b: CompanyTerm[]): boolean {
  return a.length === b.length && a.every((t, i) => t.term === b[i].term && t.kind === b[i].kind && t.selected === b[i].selected);
}

export function CompanyResearchCard({ jobId, employer }: { jobId: number; employer: string }) {
  const [research, setResearch] = useState<CompanyResearch | null>(null);
  const [terms, setTerms] = useState<CompanyTerm[]>([]);
  const [website, setWebsite] = useState("");
  const [busy, setBusy] = useState(false);
  const [own, setOwn] = useState("");
  const { fail, notify } = useToast();

  const show = (r: CompanyResearch | null) => { setResearch(r); setTerms(r?.terms ?? []); if (r?.website) setWebsite(r.website); };
  useEffect(() => {
    api.companyResearch(jobId).then((v) => { show(v.research); if (!v.research?.website) setWebsite(v.suggestedWebsite ?? ""); }).catch(fail);
  }, [jobId]);

  const read = async () => {
    if (!website.trim()) return;
    setBusy(true);
    try {
      const r = await api.researchCompany(jobId, website.trim());
      show(r);
      notify("ok", `Read ${r.pages.length} page${r.pages.length === 1 ? "" : "s"} and found ${r.terms.length} terms. Untick any you don't want.`);
    } catch (e) { fail(e); } finally { setBusy(false); }
  };
  const save = async () => {
    try { show(await api.saveCompanyTerms(jobId, terms)); notify("ok", "Saved. Your next resume and cover letter will use the ticked terms."); } catch (e) { fail(e); }
  };
  const toggle = (i: number) => setTerms(terms.map((t, j) => (j === i ? { ...t, selected: !t.selected } : t)));
  const remove = (i: number) => setTerms(terms.filter((_, j) => j !== i));
  const add = () => {
    const t = own.trim();
    if (!t || terms.some((x) => x.term.toLowerCase() === t.toLowerCase())) { setOwn(""); return; }
    setTerms([...terms, { term: t, kind: "yours", sourceUrl: "", context: "", selected: true }]);
    setOwn("");
  };
  const dirty = !sameTerms(terms, research?.terms ?? []);
  const chosen = terms.filter((t) => t.selected).length;

  return (
    <Card title="Company language" actions={dirty && <button className="btn small primary" onClick={save}>Save choices</button>}>
      <p className="muted small">
        Reads a few public pages of {employer}'s own website (about, mission, values, careers) so your resume and cover letter can
        speak their language. It follows the site's robots.txt rules and sends nothing about you. Claude uses a term only where your
        own experience backs it up.
      </p>
      <div className="row-inline">
        <input value={website} onChange={(e) => setWebsite(e.target.value)} placeholder="https://www.company.com" onKeyDown={(e) => e.key === "Enter" && read()} />
        <button className="btn small" disabled={busy || !website.trim()} onClick={read}>{busy ? "Reading…" : research ? "Read again" : "Read website"}</button>
      </div>
      {research && research.pages.length > 0 && (
        <details className="small">
          <summary>Read {research.pages.length} page{research.pages.length === 1 ? "" : "s"} on {shortDate(research.fetchedAt)}</summary>
          <ul>{research.pages.map((p) => <li key={p.url}><button className="link" onClick={() => openUrl(p.url).catch(fail)}>{p.title || p.url}</button></li>)}</ul>
          {research.skipped.length > 0 && <p className="muted">Skipped: {research.skipped.join("; ")}</p>}
        </details>
      )}
      {research && research.pages.length > 0 && terms.filter((t) => t.kind !== "yours").length === 0 && (
        <div className="alert alert-warn">Little text could be read. Some sites build their pages with JavaScript. Add the terms you want below.</div>
      )}
      {GROUPS.map((g) => {
        const items = terms.map((t, i) => ({ t, i })).filter(({ t }) => t.kind === g.kind);
        if (items.length === 0) return null;
        const long = g.kind === "mission" || g.kind === "vision" || g.kind === "tagline";
        return (
          <div key={g.kind} className="company-group">
            <div className="muted small strong">{g.label}</div>
            {long
              ? items.map(({ t, i }) => (
                <label key={i} className="company-statement" title={t.sourceUrl}>
                  <input type="checkbox" checked={t.selected} onChange={() => toggle(i)} /> <span>{t.term}</span>
                </label>
              ))
              : <div className="chips">{items.map(({ t, i }) => (
                <span key={i} className={`chip term ${t.selected ? "on" : ""}`} title={t.context || t.sourceUrl}>
                  <label><input type="checkbox" checked={t.selected} onChange={() => toggle(i)} /> {t.term}</label>
                  {g.kind === "yours" && <button aria-label={`Remove ${t.term}`} onClick={() => remove(i)}>×</button>}
                </span>
              ))}</div>}
          </div>
        );
      })}
      <div className="row-inline">
        <input value={own} onChange={(e) => setOwn(e.target.value)} placeholder="Add a term of your own (e.g. from a recruiter call)" onKeyDown={(e) => e.key === "Enter" && add()} />
        <button className="btn small" disabled={!own.trim()} onClick={add}>Add</button>
      </div>
      {terms.length > 0 && <p className="muted small">{chosen} of {terms.length} terms ticked{dirty ? " (not saved yet)" : ""}. Regenerate the resume and cover letter to use them.</p>}
    </Card>
  );
}
