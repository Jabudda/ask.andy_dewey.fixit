// Mirrors src-tauri/src/models.rs (serde camelCase).

export const STATUSES = [
  "Discovered",
  "Under Review",
  "Documents Ready",
  "Awaiting Approval",
  "Approved",
  "In Progress",
  "Submitted",
  "Interviewing",
  "Offer",
  "Rejected",
  "Withdrawn",
  "Closed",
] as const;
export type Status = (typeof STATUSES)[number];

export type RemoteType = "remote" | "hybrid" | "onsite" | "unknown";
export type Eligibility = "eligible" | "excluded" | "unknown";
export type SalaryFlag = "meets" | "partial" | "below" | "missing";

export interface JobDraft {
  title: string;
  employer: string;
  locationText: string;
  remoteType: RemoteType | string;
  wisconsinEligibility: Eligibility | string;
  salaryMin: number | null;
  salaryMax: number | null;
  salaryPeriod: string | null;
  employmentType: string;
  postedDate: string | null;
  validThrough: string | null;
  applyUrl: string | null;
  postingUrl: string | null;
  description: string;
  source: string;
  extractionMethod: string;
  detectionNotes: string[];
}

export interface Job extends JobDraft {
  id: number;
  salaryFlag: SalaryFlag | string;
  reviewFlags: string[];
  excludedReasons: string[];
  expired: boolean;
  status: Status | string;
  score: number | null;
  assessment: string | null;
  notes: string;
  submittedAt: string | null;
  confirmationRef: string | null;
  confirmationMethod: string | null;
  isSample: boolean;
  discoveredAt: string;
  updatedAt: string;
}

export interface SaveJobResult {
  id: number | null;
  duplicateOf: Job | null;
}

export interface StatusChange {
  jobId: number;
  toStatus: string;
  note?: string;
  confirmationRef?: string | null;
  userConfirmedSubmission?: boolean;
}

export interface StatusEvent {
  id: number;
  fromStatus: string | null;
  toStatus: string;
  note: string;
  changedAt: string;
}
export interface Contact { id?: number | null; jobId: number; name: string; role: string; email: string; phone: string; notes: string }
export interface Interview { id?: number | null; jobId: number; scheduledAt: string; kind: string; notes: string; outcome: string }
export interface Reminder { id?: number | null; jobId: number; dueDate: string; note: string; done: boolean }

export interface DocumentRec {
  id: number;
  kind: string;
  fileName: string;
  storedPath: string;
  sha256: string;
  extractedText: string;
  jobId: number | null;
  version: number;
  createdAt: string;
}

export interface JobDetail {
  job: Job;
  history: StatusEvent[];
  contacts: Contact[];
  interviews: Interview[];
  reminders: Reminder[];
  documents: DocumentRec[];
}

export interface Preferences {
  homeCity: string;
  homeState: string;
  remoteOnly: boolean;
  minAnnualSalary: number;
  hoursPerYear: number;
  employmentTypes: string[];
  targetTitles: string[];
  preferredFields: string[];
  keywords: string[];
  excludedKeywords: string[];
  followUpDays: number;
}

export interface ProfileBasics {
  fullName: string; email: string; phone: string; location: string;
  linkedinUrl: string; headline: string; summary: string; updatedAt?: string | null;
}
export interface Experience {
  id?: number | null; employer: string; title: string; location: string; startDate: string;
  endDate: string; isCurrent: boolean; achievements: string; sortOrder: number;
}
export interface Skill { id?: number | null; name: string; category: string; evidence: string }
export interface Certification { id?: number | null; name: string; issuer: string; dateEarned: string; expires: string; credentialId: string }
export interface Accomplishment { id?: number | null; title: string; description: string; evidence: string }

export interface ProfileBundle {
  basics: ProfileBasics;
  experience: Experience[];
  skills: Skill[];
  certifications: Certification[];
  accomplishments: Accomplishment[];
  documents: DocumentRec[];
}

export interface DocumentImportResult {
  document: DocumentRec;
  suggestedSkills: string[];
  warning: string | null;
}

export interface LabelCount { label: string; count: number }
export interface WeeklyActivity { weekStart: string; discovered: number; submitted: number; interviews: number }
export interface FollowUp { jobId: number; title: string; employer: string; reason: string; due: string }

export interface DashboardStats {
  totalJobs: number;
  strongMatches: number;
  unscored: number;
  awaitingApproval: number;
  submitted: number;
  interviewsScheduled: number;
  offers: number;
  rejections: number;
  needsFollowUp: number;
  excluded: number;
  needsReview: number;
  statusCounts: LabelCount[];
  scoreBuckets: LabelCount[];
  activity: WeeklyActivity[];
  followUps: FollowUp[];
  interviewRate: number | null;
  offerRate: number | null;
  milestoneDate: string;
}

export interface ImportJobLine {
  jobId: number; employer: string; title: string; status: string; applied: string | null; salary: string;
  action: string; posting: string | null; linkedDocuments: number; flags: string[];
}
export interface ImportDocLine { file: string; kind: string; job: string | null; action: string; textChars: number; note: string | null }
export interface ImportReport {
  dryRun: boolean; root: string; trackerFile: string | null; jobs: ImportJobLine[]; documents: ImportDocLine[];
  certifications: string[]; profileUpdates: string[]; suggestedSkills: string[]; warnings: string[]; skipped: string[];
}

export interface LogEntry { id: number; ts: string; level: string; source: string; message: string }
export interface AppInfo { version: string; dataDir: string; databasePath: string; documentsDir: string }
