import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { AiSettings, AiUsageSummary } from "../lib/types";
import { Card, Field, confirmAction, useToast } from "./ui";

const MODELS: [string, string][] = [
  ["claude-opus-5-5", "Claude Opus 5.5 — best writing (≈ $0.10–0.30 per document)"],
  ["claude-sonnet-5-5", "Claude Sonnet 5.5 — about half the cost"],
  ["claude-haiku-5-5", "Claude Haiku 5.5 — cheapest, weakest letters"],
];

/** Opt-in AI writing: disclosure, Keychain key, model, effort, voice notes, spend. */
export function AiSettingsCard() {
  const [s, setS] = useState<AiSettings | null>(null);
  const [hasKey, setHasKey] = useState(false);
  const [key, setKey] = useState("");
  const [usage, setUsage] = useState<AiUsageSummary | null>(null);
  const [agree, setAgree] = useState(false);
  const { fail, notify } = useToast();

  const load = () => {
    api.getAiSettings().then((x) => { setS(x); setAgree(!!x.acknowledgedAt); }).catch(fail);
    api.credentialStatus("anthropic").then(setHasKey).catch(() => setHasKey(false));
    api.aiUsageSummary().then(setUsage).catch(() => {});
  };
  useEffect(load, []);
  if (!s) return null;

  const save = async (next: AiSettings, msg?: string) => {
    try { await api.saveAiSettings(next); setS(next); if (msg) notify("ok", msg); } catch (e) { fail(e); }
  };
  const saveKey = async () => {
    try { await api.saveCredential("anthropic", key.trim()); setKey(""); setHasKey(true); notify("ok", "Anthropic API key saved in your macOS Keychain"); } catch (e) { fail(e); }
  };
  const removeKey = async () => {
    if (await confirmAction("Remove the Anthropic API key from your Keychain? AI writing stops until you add it again.", "Remove")) {
      api.deleteCredential("anthropic").then(() => setHasKey(false)).catch(fail);
    }
  };
  const enable = (on: boolean) => save({ ...s, enabled: on, acknowledgedAt: on ? (s.acknowledgedAt ?? new Date().toISOString()) : s.acknowledgedAt }, on ? "AI writing is on" : "AI writing is off");

  return (
    <Card title="AI writing (Claude) — optional" actions={s.enabled ? <span className="badge tone-good">✓ On</span> : <span className="badge tone-muted">Off</span>}>
      <div className="alert alert-info small">
        <strong>What is sent to Anthropic, and only when you click Generate:</strong> the job posting, the fit analysis, and the verified facts from your
        Career profile (work history, skills, certifications, education, summary), plus the voice notes below.
        <strong> Never sent:</strong> your email, phone, address, notes, other jobs, application history or documents.
        Every generation shows its estimated cost first. You can review exactly what will be sent before confirming. Typical cost with Opus 5.5 is
        $0.10–$0.30 per document, billed to your own Anthropic account. Anthropic's API terms apply to what you send.
      </div>
      <label className="check"><input type="checkbox" checked={agree} onChange={(e) => setAgree(e.target.checked)} /> I understand what is sent and that it costs money</label>
      <div className="btn-row">
        <button className="btn small primary" disabled={!agree || s.enabled || !hasKey} onClick={() => enable(true)}>Turn on AI writing</button>
        {s.enabled && <button className="btn small" onClick={() => enable(false)}>Turn off</button>}
        {!hasKey && <span className="muted small">Add an API key first.</span>}
      </div>

      <div className="form-grid">
        <Field label={hasKey ? "Anthropic API key — saved in Keychain ✓ (enter a new one to replace)" : "Anthropic API key"} hint="From console.anthropic.com → API keys. Stored only in your macOS Keychain.">
          <div className="inline-form">
            <input className="grow" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder="sk-ant-…" />
            <button className="btn small" disabled={key.trim().length < 20} onClick={saveKey}>Save key</button>
            {hasKey && <button className="btn small danger" onClick={removeKey}>Remove</button>}
          </div>
        </Field>
        <Field label="Model">
          <select value={s.model} onChange={(e) => save({ ...s, model: e.target.value })}>
            {MODELS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
        </Field>
        <Field label="Effort" hint="Higher thinks longer and costs more. High suits resumes and letters.">
          <select value={s.effort} onChange={(e) => save({ ...s, effort: e.target.value })}>
            {["low", "medium", "high", "xhigh"].map((x) => <option key={x} value={x}>{x}</option>)}
          </select>
        </Field>
        <Field label="Your voice (used for cover letters)" wide hint="Describe how you write. Drawn from your own writing sample, and edit it freely. Nothing here is used as a claim about your work.">
          <textarea rows={5} value={s.styleNotes} onChange={(e) => setS({ ...s, styleNotes: e.target.value })} onBlur={() => save(s)} />
        </Field>
      </div>
      {usage && <p className="muted small">AI usage: {usage.calls} request{usage.calls === 1 ? "" : "s"} · ≈ ${usage.monthUsd.toFixed(2)} this month · ≈ ${usage.totalUsd.toFixed(2)} total (estimated from list prices).</p>}
    </Card>
  );
}
