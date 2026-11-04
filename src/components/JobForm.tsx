import type { JobDraft } from "../lib/types";
import { Field } from "./ui";

/** Editable job-posting form. Every importer lands here so the user verifies before saving. */
export function JobForm({ draft, onChange }: { draft: JobDraft; onChange: (d: JobDraft) => void }) {
  const set = <K extends keyof JobDraft>(k: K, v: JobDraft[K]) => onChange({ ...draft, [k]: v });
  const num = (s: string) => (s.trim() === "" ? null : Number(s.replace(/[$,]/g, "")));
  const opt = (s: string) => (s.trim() === "" ? null : s.trim());

  return (
    <div className="form-grid">
      {draft.detectionNotes.length > 0 && (
        <div className="notes-box field-wide">
          <strong>How these fields were filled — please verify:</strong>
          <ul>{draft.detectionNotes.map((n, i) => <li key={i}>{n}</li>)}</ul>
        </div>
      )}
      <Field label="Job title *"><input value={draft.title} onChange={(e) => set("title", e.target.value)} /></Field>
      <Field label="Employer *"><input value={draft.employer} onChange={(e) => set("employer", e.target.value)} /></Field>
      <Field label="Location"><input value={draft.locationText} onChange={(e) => set("locationText", e.target.value)} /></Field>
      <Field label="Employment type" hint="e.g. Full-time, Contract">
        <input value={draft.employmentType} onChange={(e) => set("employmentType", e.target.value)} />
      </Field>
      <Field label="Work arrangement">
        <select value={draft.remoteType} onChange={(e) => set("remoteType", e.target.value)}>
          <option value="remote">Fully remote</option>
          <option value="hybrid">Hybrid</option>
          <option value="onsite">On-site</option>
          <option value="unknown">Not stated</option>
        </select>
      </Field>
      <Field label="Can hire in Wisconsin?">
        <select value={draft.wisconsinEligibility} onChange={(e) => set("wisconsinEligibility", e.target.value)}>
          <option value="eligible">Yes — stated or US-wide</option>
          <option value="excluded">No — Wisconsin excluded</option>
          <option value="unknown">Not stated</option>
        </select>
      </Field>
      <Field label="Pay minimum">
        <input inputMode="decimal" value={draft.salaryMin ?? ""} onChange={(e) => set("salaryMin", num(e.target.value))} />
      </Field>
      <Field label="Pay maximum">
        <input inputMode="decimal" value={draft.salaryMax ?? ""} onChange={(e) => set("salaryMax", num(e.target.value))} />
      </Field>
      <Field label="Pay period">
        <select value={draft.salaryPeriod ?? ""} onChange={(e) => set("salaryPeriod", opt(e.target.value))}>
          <option value="">—</option>
          <option value="year">Per year</option>
          <option value="hour">Per hour</option>
          <option value="day">Per day</option>
          <option value="week">Per week</option>
          <option value="month">Per month</option>
        </select>
      </Field>
      <Field label="Date posted">
        <input type="date" value={draft.postedDate ?? ""} onChange={(e) => set("postedDate", opt(e.target.value))} />
      </Field>
      <Field label="Valid through" hint="Used to flag expired postings">
        <input type="date" value={draft.validThrough ?? ""} onChange={(e) => set("validThrough", opt(e.target.value))} />
      </Field>
      <Field label="Original posting URL" wide>
        <input value={draft.postingUrl ?? ""} placeholder="https://" onChange={(e) => set("postingUrl", opt(e.target.value))} />
      </Field>
      <Field label="Application URL" wide>
        <input value={draft.applyUrl ?? ""} placeholder="https://" onChange={(e) => set("applyUrl", opt(e.target.value))} />
      </Field>
      <Field label="Full job description" wide>
        <textarea rows={12} value={draft.description} onChange={(e) => set("description", e.target.value)} />
      </Field>
    </div>
  );
}
