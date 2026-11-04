import { createContext, useCallback, useContext, useState, type ReactNode } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { errorText } from "../lib/api";

// ── Toasts ──
type Toast = { id: number; kind: "ok" | "err" | "info"; text: string };
const ToastCtx = createContext<{ notify: (kind: Toast["kind"], text: string) => void; fail: (e: unknown) => void }>({
  notify: () => {},
  fail: () => {},
});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const notify = useCallback((kind: Toast["kind"], text: string) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, kind, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "err" ? 9000 : 4000);
  }, []);
  const fail = useCallback((e: unknown) => notify("err", errorText(e)), [notify]);
  return (
    <ToastCtx.Provider value={{ notify, fail }}>
      {children}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.kind}`} onClick={() => setToasts((x) => x.filter((y) => y.id !== t.id))}>
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}
export const useToast = () => useContext(ToastCtx);

// ── Layout primitives ──
export function Card({ title, actions, children, className = "" }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`card ${className}`}>
      {(title || actions) && (
        <header className="card-head">
          {title && <h3>{title}</h3>}
          {actions && <div className="card-actions">{actions}</div>}
        </header>
      )}
      {children}
    </section>
  );
}

export function Field({ label, children, hint, wide }: { label: string; children: ReactNode; hint?: string; wide?: boolean }) {
  return (
    <label className={`field ${wide ? "field-wide" : ""}`}>
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="empty">{children}</div>;
}

// ── Badges ──
const STATUS_TONE: Record<string, string> = {
  Discovered: "neutral", "Under Review": "neutral", "Documents Ready": "info", "Awaiting Approval": "warn",
  Approved: "info", "In Progress": "info", Submitted: "accent", Interviewing: "accent", Offer: "good",
  Rejected: "bad", Withdrawn: "muted", Closed: "muted",
};
export function StatusBadge({ status }: { status: string }) {
  return <span className={`badge tone-${STATUS_TONE[status] ?? "neutral"}`}>{status}</span>;
}

const REMOTE_LABEL: Record<string, [string, string]> = {
  remote: ["Remote", "good"], hybrid: ["Hybrid", "bad"], onsite: ["On-site", "bad"], unknown: ["Unknown", "warn"],
};
export function RemoteBadge({ value }: { value: string }) {
  const [l, t] = REMOTE_LABEL[value] ?? ["Unknown", "warn"];
  return <span className={`badge tone-${t}`}>{icon(t)} {l}</span>;
}

const ELIG_LABEL: Record<string, [string, string]> = {
  eligible: ["Eligible", "good"], excluded: ["Not eligible", "bad"], unknown: ["Unconfirmed", "warn"],
};
export function EligibilityBadge({ value }: { value: string }) {
  const [l, t] = ELIG_LABEL[value] ?? ["Unconfirmed", "warn"];
  return <span className={`badge tone-${t}`}>{icon(t)} {l}</span>;
}

const SALARY_LABEL: Record<string, [string, string]> = {
  meets: ["Meets minimum", "good"], partial: ["Range straddles", "warn"], below: ["Below minimum", "bad"], missing: ["Not listed", "warn"],
};
export function SalaryBadge({ value }: { value: string }) {
  const [l, t] = SALARY_LABEL[value] ?? ["Not listed", "warn"];
  return <span className={`badge tone-${t}`}>{icon(t)} {l}</span>;
}

/** Status meaning is never carried by color alone. */
function icon(tone: string) {
  return tone === "good" ? "✓" : tone === "bad" ? "✕" : tone === "warn" ? "!" : "";
}

/** Native macOS confirmation sheet (window.confirm is not reliable inside the WebView). */
export async function confirmAction(message: string, okLabel = "Continue"): Promise<boolean> {
  try {
    return await ask(message, { title: "Project Next Chapter", kind: "warning", okLabel, cancelLabel: "Cancel" });
  } catch {
    return window.confirm(message);
  }
}
