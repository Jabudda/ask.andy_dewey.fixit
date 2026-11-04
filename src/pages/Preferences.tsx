import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { Preferences as Prefs } from "../lib/types";
import { parseList } from "../lib/format";
import { Card, Field, useToast } from "../components/ui";

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
    };
    try {
      const n = await api.savePreferences(next);
      setP(next);
      notify("ok", `Preferences saved. ${n} saved job${n === 1 ? "" : "s"} re-checked against the new rules.`);
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
        <Card title="Search targets (used by Phase 2 discovery & scoring)">
          <div className="form-grid one">
            {listField("targetTitles", "Job titles", "One per line")}
            {listField("preferredFields", "Preferred fields", "One per line")}
          </div>
        </Card>
        <Card title="Keywords">
          <div className="form-grid one">
            {listField("keywords", "Skills & terms to look for", "One per line")}
            {listField("excludedKeywords", "Exclude postings containing", "e.g. “security clearance”, “commission only”")}
          </div>
        </Card>
      </div>
    </div>
  );
}
