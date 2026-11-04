import { useState } from "react";
import { inTauri } from "./lib/api";
import { ToastProvider } from "./components/ui";
import { Dashboard } from "./pages/Dashboard";
import { Opportunities } from "./pages/Opportunities";
import { JobDetail } from "./pages/JobDetail";
import { ImportJob } from "./pages/ImportJob";
import { Profile } from "./pages/Profile";
import { Preferences } from "./pages/Preferences";
import { DataLogs } from "./pages/DataLogs";

export type Route =
  | { page: "dashboard" }
  | { page: "jobs"; status?: string }
  | { page: "job"; jobId: number }
  | { page: "import" }
  | { page: "profile" }
  | { page: "prefs" }
  | { page: "data" };
export type Nav = (r: Route) => void;

const NAV: [Route["page"], string, string][] = [
  ["dashboard", "Dashboard", "◧"],
  ["jobs", "Opportunities", "☰"],
  ["import", "Add a job", "＋"],
  ["profile", "Career profile", "◉"],
  ["prefs", "Preferences", "⚙"],
  ["data", "Data & logs", "⛁"],
];

export function App() {
  const [route, setRoute] = useState<Route>({ page: "dashboard" });
  const [key, setKey] = useState(0);
  const go: Nav = (r) => { setRoute(r); setKey((k) => k + 1); };
  const active = route.page === "job" ? "jobs" : route.page;

  return (
    <ToastProvider>
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
          <div className="sidebar-foot">Local-only · Phase 1</div>
        </nav>
        <main className="main" key={key}>
          {!inTauri && (
            <div className="alert alert-warn">This is the desktop app's interface. Run it with <code>npm run app:dev</code> to connect to the local database.</div>
          )}
          {route.page === "dashboard" && <Dashboard go={go} />}
          {route.page === "jobs" && <Opportunities go={go} initialStatus={route.status} />}
          {route.page === "job" && <JobDetail id={route.jobId} go={go} />}
          {route.page === "import" && <ImportJob go={go} />}
          {route.page === "profile" && <Profile />}
          {route.page === "prefs" && <Preferences />}
          {route.page === "data" && <DataLogs />}
        </main>
      </div>
    </ToastProvider>
  );
}
