import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { inTauri } from "./lib/api";
import { ToastProvider } from "./components/ui";
import { Dashboard } from "./pages/Dashboard";
import { Opportunities } from "./pages/Opportunities";
import { JobDetail } from "./pages/JobDetail";
import { ImportJob } from "./pages/ImportJob";
import { Profile } from "./pages/Profile";
import { Preferences } from "./pages/Preferences";
import { DataLogs } from "./pages/DataLogs";
import { Discovery } from "./pages/Discovery";
import { ReviewQueue } from "./pages/ReviewQueue";
import { Email } from "./pages/Email";
import type { RunSummary, SyncSummary } from "./lib/types";
import logo from "./assets/logo.svg";
import { useToast } from "./components/ui";

export type Route =
  | { page: "dashboard" }
  | { page: "jobs"; status?: string }
  | { page: "job"; jobId: number }
  | { page: "import" }
  | { page: "search" }
  | { page: "email" }
  | { page: "queue"; jobId?: number }
  | { page: "profile" }
  | { page: "prefs" }
  | { page: "data" };
export type Nav = (r: Route) => void;

const NAV: [Route["page"], string, string][] = [
  ["dashboard", "Dashboard", "◧"],
  ["jobs", "Opportunities", "☰"],
  ["queue", "Review queue", "✓"],
  ["search", "Job search", "⌕"],
  ["email", "Email", "✉"],
  ["import", "Add a job", "＋"],
  ["profile", "Career profile", "◉"],
  ["prefs", "Preferences", "⚙"],
  ["data", "Data & logs", "⛁"],
];

export const ZOOM_STEPS = [1, 1.1, 1.25, 1.4, 1.5, 1.75];
const DEFAULT_ZOOM = 1.25;

function readZoom(): number {
  try {
    const v = Number(localStorage.getItem("ui_zoom"));
    return ZOOM_STEPS.includes(v) ? v : DEFAULT_ZOOM;
  } catch {
    return DEFAULT_ZOOM;
  }
}

/** Larger text for easier reading: zooms the whole window (text, charts, spacing) together. */
function TextSize() {
  const [z, setZ] = useState(readZoom);
  useEffect(() => {
    try { localStorage.setItem("ui_zoom", String(z)); } catch { /* storage unavailable */ }
    if (inTauri) getCurrentWebview().setZoom(z).catch(() => {});
  }, [z]);
  const i = ZOOM_STEPS.indexOf(z);
  return (
    <div className="text-size" role="group" aria-label="Text size">
      <span>Text size</span>
      <button className="btn small" aria-label="Smaller text" disabled={i <= 0} onClick={() => setZ(ZOOM_STEPS[i - 1])}>A−</button>
      <span className="text-size-value">{Math.round(z * 100)}%</span>
      <button className="btn small" aria-label="Larger text" disabled={i >= ZOOM_STEPS.length - 1} onClick={() => setZ(ZOOM_STEPS[i + 1])}>A+</button>
    </div>
  );
}

/** Toast when a scheduled background search finishes. */
function SearchListener() {
  const { notify } = useToast();
  useEffect(() => {
    if (!inTauri) return;
    const un = listen<RunSummary[]>("discovery-finished", (e) => {
      const n = e.payload.reduce((a, r) => a + r.newJobs, 0);
      const errs = e.payload.filter((r) => r.error).length;
      if (n || errs) notify(errs ? "err" : "ok", `Scheduled search: ${n} new job${n === 1 ? "" : "s"}${errs ? `, ${errs} source(s) failed — see Job search` : ""}`);
    });
    return () => { un.then((f) => f()); };
  }, [notify]);
  return null;
}

/** Toast when a scheduled email check finds something worth knowing. */
function EmailListener() {
  const { notify } = useToast();
  useEffect(() => {
    if (!inTauri) return;
    const un = listen<SyncSummary>("email-synced", (e) => {
      const s = e.payload;
      if (s.jobsAdded || s.suggestions) notify("ok", `Email: ${s.jobsAdded} job${s.jobsAdded === 1 ? "" : "s"} added from alerts${s.suggestions ? `, ${s.suggestions} status change${s.suggestions === 1 ? "" : "s"} waiting for your OK (see Email)` : ""}`);
    });
    return () => { un.then((f) => f()); };
  }, [notify]);
  return null;
}

export function App() {
  const [route, setRoute] = useState<Route>({ page: "dashboard" });
  const [key, setKey] = useState(0);
  const go: Nav = (r) => { setRoute(r); setKey((k) => k + 1); };
  const active = route.page === "job" ? "jobs" : route.page;

  return (
    <ToastProvider>
      <SearchListener />
      <EmailListener />
      <div className="app">
        <nav className="sidebar">
          <div className="brand">
            <div className="logo-story" tabIndex={0} aria-describedby="logo-story-card">
              <img className="brand-mark" src={logo} alt="Next Chapter logo" width={44} height={44} />
              <div className="logo-story-card" id="logo-story-card" role="tooltip">
                <img src={logo} alt="" width={72} height={72} />
                <h3>The story behind the logo</h3>
                <dl>
                  <dt>The open book</dt><dd>Your story so far. The lines on the left page are the career you have already written.</dd>
                  <dt>The turning page</dt><dd>The next chapter, lifting up and to the right, the direction we read as forward.</dd>
                  <dt>The blank page</dt><dd>Left empty on purpose: the chapter you have not written yet.</dd>
                  <dt>The rising sun</dt><dd>A new start that grows out of the book's spine, out of what you have already done, not a clean break from it.</dd>
                  <dt>The colors</dt><dd>Navy and teal for steadiness and calm; amber and coral for optimism, and a solid base to stand on.</dd>
                </dl>
              </div>
            </div>
            <div><div className="brand-name">Next Chapter</div><div className="brand-sub">Job search, under your control</div></div>
          </div>
          {NAV.map(([page, label, icon]) => (
            <button key={page} className={`nav-item ${active === page ? "active" : ""}`} onClick={() => go({ page } as Route)}>
              <span className="nav-icon" aria-hidden>{icon}</span>{label}
            </button>
          ))}
          <TextSize />
          <div className="sidebar-foot">Local-only · Phase 3</div>
        </nav>
        <main className="main" key={key}>
          {!inTauri && (
            <div className="alert alert-warn">This is the desktop app's interface. Run it with <code>npm run app:dev</code> to connect to the local database.</div>
          )}
          {route.page === "dashboard" && <Dashboard go={go} />}
          {route.page === "jobs" && <Opportunities go={go} initialStatus={route.status} />}
          {route.page === "job" && <JobDetail id={route.jobId} go={go} />}
          {route.page === "import" && <ImportJob go={go} />}
          {route.page === "search" && <Discovery go={go} />}
          {route.page === "email" && <Email go={go} />}
          {route.page === "queue" && <ReviewQueue go={go} initialJobId={route.jobId} />}
          {route.page === "profile" && <Profile />}
          {route.page === "prefs" && <Preferences />}
          {route.page === "data" && <DataLogs />}
        </main>
      </div>
    </ToastProvider>
  );
}
