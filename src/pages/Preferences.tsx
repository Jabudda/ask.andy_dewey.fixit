import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { Preferences as Prefs } from "../lib/types";
import { parseList } from "../lib/format";
import { Card, Field, useToast } from "../components/ui";
import { AiSettingsCard } from "../components/AiSettingsCard";

const TYPES = ["Full-time", "Part-time", "Contract", "Temporary"];

export function Preferences() {
  const [p, setP] = useState<Prefs | null>(null);
  const [lists, setLists] = useState<Record<string, string>>({});
  const { fail, notify } = useToast();

  useEffect(() => {
    api.getPreferences().then((x) => {
      setP(x);
      setLists({
        targetTitles: x.targetTitles.join("\n"),
        preferredFields: x.preferredFields.join("\n"),
        keywords: x.keywords.join("\n"),
        searchQueries: x.searchQueries.join("\n"),
        excludedKeywords: x.excludedKeywords.join("\n"),
      });
    }).catch(fail);
  }, []);
  if (!p) return <div className="page"><p className="muted">Loading…</p></div>;

  const save = async () => {
    const next: Prefs = {
      ...p,
      targetTitles: parseList(lists.targetTitles),
      preferredFields: parseList(lists.preferredFields),
      keywords: parseList(lists.keywords),
      excludedKeywords: parseList(lists.excludedKeywords),
      searchQueries: parseList(lists.searchQueries),
    };
    try {
      const n = await api.savePreferences(next);
      setP(next);
      notify("ok", `Preferences saved. ${n} saved job${n === 1 ? "" : "s"} re-checked and re-scored.`);
    } catch (e) { fail(e); }
  };

  const listField = (k: string, label: string, hint: string) => (
    <Field label={label} hint={hint}>
      <textarea rows={7} value={lists[k] ?? ""} onChange={(e) => setLists({ ...lists, [k]: e.target.value })} />
    </Field>
  );

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>Employment preferences</h1>
          <p className="muted">Mandatory rules exclude jobs that clearly violate them; anything unclear is flagged for your review instead of assumed.</p>
        </div>
        <button className="btn primary" onClick={save}>Save preferences</button>
      </div>

      <Card title="Mandatory requirements">
        <div className="form-grid">
          <Field label="Home city"><input value={p.homeCity} onChange={(e) => setP({ ...p, homeCity: e.target.value })} /></Field>
          <Field label="Home state" hint="Remote jobs must be able to employ residents of this state"><input value={p.homeState} onChange={(e) => setP({ ...p, homeState: e.target.value })} /></Field>
          <Field label="Minimum annual pay (USD)">
            <input inputMode="numeric" value={p.minAnnualSalary} onChange={(e) => setP({ ...p, minAnnualSalary: Number(e.target.value.replace(/[$,]/g, "")) || 0 })} />
          </Field>
          <Field label="Hours per year for hourly roles" hint={`$${Math.ceil(p.minAnnualSalary / (p.hoursPerYear || 2080))}/hr ≈ your minimum`}>
            <input inputMode="numeric" value={p.hoursPerYear} onChange={(e) => setP({ ...p, hoursPerYear: Number(e.target.value) || 0 })} />
          </Field>
          <label className="check"><input type="checkbox" checked={p.remoteOnly} onChange={(e) => setP({ ...p, remoteOnly: e.target.checked })} /> 100% remote only (exclude hybrid and on-site)</label>
          <div className="field">
            <span className="field-label">Employment types</span>
            <div className="btn-row">
              {TYPES.map((t) => (
                <label key={t} className="check">
                  <input type="checkbox" checked={p.employmentTypes.includes(t)}
                    onChange={(e) => setP({ ...p, employmentTypes: e.target.checked ? [...p.employmentTypes, t] : p.employmentTypes.filter((x) => x !== t) })} />
                  {t}
                </label>
              ))}
            </div>
          </div>
          <Field label="Follow up after (days without an update)">
            <input inputMode="numeric" value={p.followUpDays} onChange={(e) => setP({ ...p, followUpDays: Number(e.target.value) || 14 })} />
          </Field>
        </div>
      </Card>

      <div className="two-col">
        <Card title="Search targets (discovery & scoring)">
          <div className="form-grid one">
            {listField("targetTitles", "Job titles", "One per line. Used for relevance and title scoring")}
            {listField("preferredFields", "Preferred fields", "One per line")}
            {listField("searchQueries", "Searches sent to public job boards", "One per line, e.g. “smartsheet”, “operations coordinator”. Up to 10 are used")}
            <Field label="Ignore postings older than (days)">
              <input inputMode="numeric" value={p.maxPostingAgeDays} onChange={(e) => setP({ ...p, maxPostingAgeDays: Number(e.target.value) || 30 })} />
            </Field>
          </div>
        </Card>
        <Card title="Keywords">
          <div className="form-grid one">
            {listField("keywords", "Skills & terms to look for", "One per line")}
            {listField("excludedKeywords", "Exclude postings containing", "e.g. “security clearance”, “commission only”")}
          </div>
        </Card>
      </div>
      <AiSettingsCard />

      <Card title="Compatibility scoring weights" actions={<button className="btn small" onClick={() => setP({ ...p, scoringWeights: { title: 15, skills: 25, required: 20, preferred: 5, compensation: 10, remote: 15, seniority: 5, certifications: 5 } })}>Reset to defaults</button>}>
        <p className="muted small">How much each criterion counts toward the 0–100 score. Any numbers work, because they're relative. Set one to 0 to ignore it. Saving re-scores every job.</p>
        <div className="form-grid">
          {([
            ["title", "Title & field alignment"], ["skills", "Skills & tools"], ["required", "Required qualifications"],
            ["preferred", "Preferred qualifications"], ["compensation", "Compensation"], ["remote", "Remote eligibility"],
            ["seniority", "Seniority & scope"], ["certifications", "Certifications"],
          ] as [keyof Prefs["scoringWeights"], string][]).map(([k, label]) => (
            <Field key={k} label={`${label}: ${p.scoringWeights[k]}`}>
              <input type="range" min={0} max={40} step={1} value={p.scoringWeights[k]}
                onChange={(e) => setP({ ...p, scoringWeights: { ...p.scoringWeights, [k]: Number(e.target.value) } })} />
            </Field>
          ))}
        </div>
      </Card>
    </div>
  );
}
