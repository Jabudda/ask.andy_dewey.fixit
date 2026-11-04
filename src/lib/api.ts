import { invoke } from "@tauri-apps/api/core";
import type {
  Accomplishment, AppInfo, Certification, Contact, DashboardStats, DocumentImportResult, Experience, Interview, Job,
  ImportReport, JobDetail, JobDraft, LogEntry, Preferences, ProfileBasics, ProfileBundle, Reminder, SaveJobResult, Skill, StatusChange,
} from "./types";

/** True when running inside the Tauri shell (false in a plain browser tab). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) {
    return Promise.reject(new Error("Project Next Chapter must be run as the desktop app (npm run app:dev)."));
  }
  return invoke<T>(cmd, args);
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),

  getPreferences: () => call<Preferences>("get_preferences"),
  savePreferences: (prefs: Preferences) => call<number>("save_preferences", { prefs }),
  setMilestone: (date: string) => call<void>("set_milestone", { date }),

  getProfile: () => call<ProfileBundle>("get_profile"),
  saveProfileBasics: (basics: ProfileBasics) => call<void>("save_profile_basics", { basics }),
  saveExperience: (item: Experience) => call<number>("save_experience", { item }),
  saveSkill: (item: Skill) => call<number>("save_skill", { item }),
  saveCertification: (item: Certification) => call<number>("save_certification", { item }),
  saveAccomplishment: (item: Accomplishment) => call<number>("save_accomplishment", { item }),
  deleteProfileItem: (section: "experience" | "skills" | "certifications" | "accomplishments", id: number) =>
    call<void>("delete_profile_item", { section, id }),
  importDocument: (path: string, kind: string) => call<DocumentImportResult>("import_document", { path, kind }),
  deleteDocument: (id: number) => call<void>("delete_document", { id }),
  openDocument: (id: number) => call<void>("open_document", { id }),
  importCommandCenter: (path: string, dryRun: boolean) => call<ImportReport>("import_command_center", { path, dryRun }),

  parsePastedJob: (text: string) => call<JobDraft>("parse_pasted_job", { text }),
  fetchJobFromUrl: (url: string) => call<JobDraft>("fetch_job_from_url", { url }),
  saveJob: (draft: JobDraft, force = false) => call<SaveJobResult>("save_job", { draft, force }),
  updateJob: (id: number, draft: JobDraft) => call<Job>("update_job", { id, draft }),
  listJobs: () => call<Job[]>("list_jobs"),
  getJobDetail: (id: number) => call<JobDetail>("get_job_detail", { id }),
  deleteJob: (id: number) => call<void>("delete_job", { id }),
  changeStatus: (change: StatusChange) => call<Job>("change_status", { change }),
  updateNotes: (id: number, notes: string) => call<void>("update_notes", { id, notes }),
  saveContact: (item: Contact) => call<number>("save_contact", { item }),
  saveInterview: (item: Interview) => call<number>("save_interview", { item }),
  saveReminder: (item: Reminder) => call<number>("save_reminder", { item }),
  deleteJobItem: (kind: "contacts" | "interviews" | "reminders", id: number) => call<void>("delete_job_item", { kind, id }),

  dashboardStats: () => call<DashboardStats>("dashboard_stats"),

  exportJobs: (path: string, format: "csv" | "xlsx") => call<number>("export_jobs", { path, format }),
  backupDatabase: (path: string) => call<void>("backup_database", { path }),
  restoreDatabase: (path: string) => call<string>("restore_database", { path }),
  listLogs: (query: string) => call<LogEntry[]>("list_logs", { query }),
  loadSampleData: () => call<number>("load_sample_data"),
  removeSampleData: () => call<number>("remove_sample_data"),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
