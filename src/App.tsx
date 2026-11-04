import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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
import type { RunSummary } from "./lib/types";
import { useToast } from "./components/ui";

export type Route =
  | { page: "dashboard" }
  | { page: "jobs"; status?: string }
  | { page: "job"; jobId: number }
  | { page: "import" }
  | { page: "search" }
  | { page: "profile" }
  | { page: "prefs" }
  | { page: "data" };
export type Nav = (r: Route) => void;

const NAV: [Route["page"], string, string][] = [
  ["dashboard", "Dashboard", "◧"],
  ["jobs", "Opportunities", "☰"],
  ["search", "Job search", "⌕"],
  ["import", "Add a job", "＋"],
  ["profile", "Career profile", "◉"],
  ["prefs", "Preferences", "⚙"],
  ["data", "Data & logs", "⛁"],
];

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

export function App() {
  const [route, setRoute] = useState<Route>({ page: "dashboard" });
  const [key, setKey] = useState(0);
  const go: Nav = (r) => { setRoute(r); setKey((k) => k + 1); };
  const active = route.page === "job" ? "jobs" : route.page;

  return (
    <ToastProvider>
      <SearchListener />
      <div className="app">
        <nav className="sidebar">
          <div className="brand">
            <span className="brand-mark">▍</span>
            <div><div className="brand-name">Next Chapter</div><div className="brand-sub">Job search, under your control</div></div>
          </div>
          {NAV.map(([page, label, icon]) => (
            <button key={page} className={`nav-item ${active === page ? "active" : ""}`} onClick={() => go({ page } as Route)}>
              <span className="nav-icon" aria-hidden>{icon}</span>{label}
            </button>
          ))}
          <div className="sidebar-foot">Local-only · Phase 2</div>
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
          {route.page === "profile" && <Profile />}
          {route.page === "prefs" && <Preferences />}
          {route.page === "data" && <DataLogs />}
        </main>
      </div>
    </ToastProvider>
  );
}
