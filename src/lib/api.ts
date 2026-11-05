import { invoke } from "@tauri-apps/api/core";
import type {
  Accomplishment, AppInfo, Certification, Contact, DashboardStats, DocumentImportResult, Experience, Interview, Job,
  AiSettings, AiUsageSummary, BankAnswer, Decision, JobAnswer, Packet, QueueItem, Generated, PreparedGeneration, Education, ImportReport, JobSource, RunSummary, SearchRun, SearchSettings, JobDetail, JobDraft, LogEntry, Preferences, ProfileBasics, ProfileBundle, Reminder, SaveJobResult, Skill, StatusChange,
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
  saveEducation: (item: Education) => call<number>("save_education", { item }),
  rescoreJobs: () => call<number>("rescore_jobs"),
  deleteProfileItem: (section: "experience" | "skills" | "certifications" | "accomplishments" | "education", id: number) =>
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

  listSources: () => call<JobSource[]>("list_sources"),
  saveSource: (source: JobSource) => call<number>("save_source", { source }),
  deleteSource: (id: number) => call<void>("delete_source", { id }),
  detectSource: (url: string) => call<[string, string] | null>("detect_source", { url }),
  listSearchRuns: () => call<SearchRun[]>("list_search_runs"),
  getSearchSettings: () => call<SearchSettings>("get_search_settings"),
  setSearchInterval: (hours: number) => call<void>("set_search_interval", { hours }),
  credentialStatus: (name: string) => call<boolean>("credential_status", { name }),
  saveCredential: (name: string, value: string) => call<void>("save_credential", { name, value }),
  deleteCredential: (name: string) => call<void>("delete_credential", { name }),
  getAiSettings: () => call<AiSettings>("get_ai_settings"),
  saveAiSettings: (settings: AiSettings) => call<void>("save_ai_settings", { settings }),
  aiUsageSummary: () => call<AiUsageSummary>("ai_usage_summary"),
  prepareGeneration: (jobId: number, kind: "resume" | "cover_letter", learnFromPrevious = false) =>
    call<PreparedGeneration>("prepare_generation", { jobId, kind, learnFromPrevious }),
  generateDocument: (jobId: number, kind: "resume" | "cover_letter", learnFromPrevious = false) =>
    call<Generated>("generate_document", { jobId, kind, learnFromPrevious }),
  saveDocumentEdit: (id: number, content: unknown) => call<Generated>("save_document_edit", { id, content }),
  approveDocument: (id: number, reviewed: boolean) => call<import("./types").DocumentRec>("approve_document", { id, reviewed }),
  openDocumentPdf: (id: number) => call<void>("open_document_pdf", { id }),
  saveDocumentCopy: (id: number, format: "pdf" | "docx", dest: string) => call<void>("save_document_copy", { id, format, dest }),
  revealDocument: (id: number) => call<void>("reveal_document", { id }),

  reviewQueue: () => call<QueueItem[]>("review_queue"),
  applicationPacket: (jobId: number) => call<Packet>("application_packet", { jobId }),
  decideJob: (jobId: number, decision: Decision, note: string) => call<Job>("decide_job", { jobId, decision, note }),
  saveJobAnswer: (answer: JobAnswer) => call<number>("save_job_answer", { answer }),
  deleteJobAnswer: (id: number) => call<void>("delete_job_answer", { id }),
  answerBank: () => call<BankAnswer[]>("answer_bank"),
  saveBankAnswer: (key: string, answer: string) => call<void>("save_bank_answer", { key, answer }),
  startApplication: (jobId: number) => call<string>("start_application", { jobId }),
  setFollowUpDays: (days: number) => call<void>("set_follow_up_days", { days }),
  runDiscovery: (sourceId?: number) => call<RunSummary[]>("run_discovery", { sourceId: sourceId ?? null }),

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
