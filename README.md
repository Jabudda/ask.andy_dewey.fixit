# Project Next Chapter

A local-first macOS desktop app for running a job search: keep a verified career profile, import and check job postings against your requirements, and track every application through to the outcome. You stay in control of every application, and the app makes no claims about you that you have not verified.

Built with **Tauri v2 · React · TypeScript · Rust · SQLite**. It runs natively on Apple Silicon.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the architecture, database design and phase plan.

---

## Phase 3 status (this release): tailored resumes and cover letters

| Area | What works |
|---|---|
| **AI writing (opt-in)** | Claude **Opus 5.5** by default; Sonnet 5.5 or Haiku 5.5 are optional. Turn it on in **Preferences → AI writing** after reading the disclosure. The API key lives in the **macOS Keychain**. Requests use structured JSON output, explicit effort, and server-side **refusal fallbacks**. Every call is logged with its tokens and estimated cost |
| **What is sent** | Only the job posting, the fit analysis, the verified profile facts and your voice notes, and only when you click Generate. **Never sent:** your email, phone, address, notes, other jobs or documents. Before each generation, **See exactly what will be sent** shows the full prompt, and the estimated cost appears before you confirm |
| **Integrity by construction** | Employers, titles, locations, dates, education and certifications are placed by the app from your profile; the model never writes them. Every model-written line must cite profile fact IDs. The **claim checker blocks** uncited lines, numbers missing from the cited fact, and tools, skills or certifications absent from your profile. Stock phrases are flagged for review |
| **Documents** | ATS-friendly **Word (.docx)** and **PDF** files (single column, Arial, standard headings), saved per employer and role with version numbers. Older roles are condensed under "Additional Experience", so your history shows no gaps |
| **Edit, compare, approve** | Edit headline, summary, skills, bullets or letter paragraphs; each save is a new re-checked version. Lines you add yourself are marked "written by you" for a final check. **Compare** shows the master resume beside the tailored one, with changes highlighted. **Approve** requires zero blocked lines plus confirmation of the review items, and moves the job to *Documents Ready* |
| **Your voice** | Upload a writing sample (Career profile → Writing sample) and describe your voice in Preferences. Cover letters follow it, using at most one light personal touch. Nothing in the sample is ever used as a claim about your work |

**Not yet:** in-app rich-text editing (edit the .docx in Word if you need formatting changes); proposed answers to application questions (Phase 4).

## Phase 2 status: discovery and scoring

### Working and covered by automated tests
| Area | What works |
|---|---|
| **Job sources** | Documented public APIs only: **Greenhouse**, **Lever** and **Ashby** company job boards; **Himalayas** (US-eligible remote jobs, keyword search); **Jobicy** (remote US jobs, keyword search; listings credited to Jobicy, applications go to the original URL); **Remotive** (latest postings). There's no logging in, no page scraping and no bot-protection bypass |
| **USAJOBS (federal)** | Remote federal postings **open to the public** (`HiringPath=public`), using your own free API key. The key is stored in the **macOS Keychain** (service `com.projectnextchapter.app`) and never in the database, logs, exports, backups or this repo. Federal grade, clearance and who-may-apply details are added to each description. The first search may show a Keychain prompt; choose **Always Allow** |
| **Watch an employer** | Paste any job link from the company and **Job search** recognises the board automatically (for example Smartsheet on Greenhouse, Vetcove and Harvey on Ashby, Civitech on Lever) |
| **Discovery pipeline** | Fetch, then keep only relevant postings (title alignment or several of your keywords) that are within your maximum posting age and not expired. Postings that fail a mandatory rule (on-site or hybrid, Wisconsin excluded, pay below minimum, excluded keyword) are counted but not saved. Duplicates are caught by employer, title and canonical URL. New jobs are saved as Discovered and scored |
| **Scheduled searches** | Every 6, 12, 24 or 48 hours while the app is open (default 12; Off is available). Shared public boards are polled at most every 4 hours. Each run is recorded with found, new, filtered, failed-rules, duplicates and the exact error. Failed sources show red in **Job search** and in the log |
| **Compatibility score (0–100)** | Eight weighted criteria you can adjust in Preferences: title and field, skills and tools, required qualifications, preferred qualifications, compensation, remote eligibility, seniority (inferred) and certifications. Credit comes **only from your verified profile and master resume**. Requirements it can't check are listed as "your judgment" and count below neutral. Thin postings are capped at "consider". Failing a mandatory rule caps the score at 39, and a clearly missing required qualification (degree, clearance, certification) caps it at 59 |
| **Hiring-manager assessment** | Per job: a recommendation (apply, consider or skip), why you fit, requirements met, partially met, gaps, items that couldn't be checked, and the assumptions it made |
| **Profile** | Education section, used for degree checks. Jobs are re-scored automatically whenever your profile, master resume or preferences change |
| **Folder import (improved)** | PDF line wraps are rejoined. A saved "posting" that contains your own name (an application page showing your resume) is never used as a job description |

### Not yet (planned)
* **AI-written fit assessments.** Scoring stays rule-based and fully explainable; AI is used only for documents.
* **Background searches while the app is closed** (would need an explicitly authorised macOS LaunchAgent).
* **Phases 3–5** (documents, application assistance, packaging) as in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Known limitations
* Remote federal jobs open to the public are scarce (a handful at any time), so USAJOBS often adds nothing new.
* Remotive's free API currently ignores search terms and returns only its latest ~11 postings, so it rarely contributes.
* Scores are heuristics built on keyword evidence. They don't read nuance, so always read the "your judgment" items.
* Some company boards return hundreds of jobs per run, which is normal; only relevant ones are saved.

### Developer tools (examples)
* `cargo run --example discover_preview -- <db> greenhouse:smartsheet ashby:vetcove` runs a live discovery against any database file and prints the top scores.
* `cargo run --example load_profile -- <db> <profile.json>` loads a verified profile from JSON. Keep that file outside the repo.
* `cargo run --example import_apply -- <db> <documents-dir> <folder>` runs a real folder import.

## Phase 1 status

### Working and covered by automated tests
| Area | What works |
|---|---|
| **App shell** | Native dark-mode macOS window with Dashboard, Opportunities, Add a job, Career profile, Preferences, Data & logs |
| **Local database** | SQLite in `~/Library/Application Support/com.projectnextchapter.app/`, versioned migrations, WAL, foreign keys |
| **Career profile** | Upload a master resume and supporting documents (.docx, .pdf, .txt and .md are read in full; PDFs use macOS PDFKit; versions kept), contact details and summary, verified work history (achievements one per line), skills, certifications, accomplishments |
| **Skill suggestions** | Skills found in an uploaded document are *offered* for you to confirm. Nothing is added automatically |
| **Preferences** | Home state, 100% remote, minimum pay with hourly annualisation, employment types, titles, fields, keywords, excluded keywords, follow-up interval. Saving re-checks every saved job |
| **Job import** | **From a link:** reads the page's schema.org `JobPosting` data, with a plain-text fallback that is flagged for review. **Paste:** recognises labelled fields and finds pay, remote status and state eligibility in the text. **Manual entry.** Every import opens as an editable draft first |
| **Requirement checks** | On-site/hybrid → excluded. Wisconsin excluded or not in an eligible-states list → excluded. Pay below your minimum → excluded. Missing pay, unstated remote status or location eligibility, expired postings and non-preferred job types → flagged for review, never assumed |
| **Duplicate detection** | Matches on normalised employer + title, and on the canonical posting URL (tracking parameters removed). You can still save a duplicate if it really is a different posting |
| **Tracking** | All 12 statuses, full status history, notes, recruiter contacts, interviews, follow-up reminders, submission confirmation/reference number |
| **Human-in-the-loop rules** (enforced in the backend) | *Approved* can only be set from *Awaiting Approval*. *Submitted* needs your confirmation or a reference number, and records which one. Awaiting Approval offers Approve / Request changes / Save for later / Reject |
| **Dashboard** | Countdown to an editable milestone (default 31 Jan 2027; changing it never alters history), 8 KPI tiles, pipeline, 12-week activity chart with a table view, score distribution, follow-up list, submitted→interview and submitted→offer rates |
| **Data** | Excel (.xlsx) and CSV export (CSV is protected against formula injection), online backup, validated restore with an automatic safety copy, searchable application log |
| **Folder import** | **Data & logs → Import an existing job-search folder** reads a “Command Center”-style folder: the tracker workbook (Tracker and Interviews sheets) becomes jobs with your original dates, statuses, pay, contacts and follow-ups. Sent resumes and cover letters, saved postings and confirmations are linked to the right job through their application folder. Saved posting text becomes the job description and is re-checked for state restrictions. Certificates become certifications (title, date and credential ID read from the PDF). Portfolio and reviews are stored as profile documents. A full preview comes first, your folder is never changed, videos over 25 MB are linked in place, and re-running only adds what is new |
| **Sample data** | 9 fictional postings (employers labelled “(SAMPLE)”, `is_sample = 1`) that can be loaded and removed in one click, plus `sample-data/sample_master_resume.txt` |

### Not in Phase 1 (planned; see the architecture doc)
* **Compatibility scores (0–100) and hiring-manager assessments.** These are Phase 2. Every job shows as *Unscored* rather than getting an estimated score.
* **Automatic job discovery and scheduled searches.** Phase 2. The connector and `search_runs` tables already exist.
* **Tailored resume and cover letter generation (DOCX/PDF), editing, comparison.** Phase 3.
* **Browser-assisted application completion and email monitoring.** Phase 4. In Phase 1, **Open employer application ↗** opens the official page in your default browser.
* **AI provider and Keychain storage.** Phase 2. Phase 1 makes no AI calls and stores no credentials.
* **A signed and notarised DMG.** Phase 5. This needs your Apple Developer ID.

### Known limitations
* Many career sites render postings with JavaScript or block automated requests. When that happens the app tells you so, and you can paste the posting text instead. It never tries to get around a block.
* Remote, state and pay detection is rule-based and deliberately conservative. Detection notes show where each value came from, so check them.
* PDFs without a text layer (scanned images) are stored but cannot be read. OCR is not included.
* Backups contain the full database including document text. The original uploaded files stay in the `documents/` folder and are not inside the backup file.

---

## Install and run

### Prerequisites (one time)
1. **Xcode Command Line Tools:** `xcode-select --install`
2. **Rust:** the official installer from https://rustup.rs. If it was installed without changing your shell profile, add `export PATH="$HOME/.cargo/bin:$PATH"` to `~/.zshrc`.
3. **Node.js 20 or later:** https://nodejs.org

### Development

```bash
npm install
```

```bash
npm run app:dev
```

`app:dev` starts the Vite dev server and opens the native window with hot reload.

### Build the Mac app

```bash
npm run app:build
```

The output is `src-tauri/target/release/bundle/macos/Project Next Chapter.app` plus a `.dmg` in `bundle/dmg/`. Drag the app to **Applications**. The build is unsigned, so on first launch you need to right-click the app and choose **Open**, then **Open** again.

### Tests

```bash
npm test
```

```bash
npm run test:rust
```

* **Rust (81 tests):** includes the AI request shape (effort, structured output, fallbacks, no thinking or temperature overrides), refusal and truncation handling, cost maths, opt-in gating, fact extraction (no contact details), the claim checker (invented numbers, tools, uncited and unknown sources, user-written lines), rendering from profile facts, DOCX and PDF output read back, and the full generate → block → edit → approve flow; plus includes scoring (evidence-only credit, caps, weights, thin postings, years merging), all six connectors parsed from recorded API shapes, discovery filtering and run records, failure reporting, scheduling, and parsing (JSON-LD, pasted text, remote, Wisconsin, pay, employment type), eligibility rules, duplicate detection, status guards, preferences re-check, milestone isolation, dashboard metrics, follow-ups, profile CRUD, document import and extraction, CSV/XLSX export, backup/restore round-trip, rejection of non-backup files, log search, real PDF text extraction, and folder import (dry run writes nothing, statuses and dates, folder-based matching, no matching on document text, certificate parsing, idempotent re-run).
* **TypeScript (10 tests):** includes how edited lines keep or lose their citations, plus countdown, money and pay formatting, annualisation, list parsing, filtering and sorting.

## Using the app (first 10 minutes)
1. **Career profile →** upload your master resume (.docx). Confirm the suggested skills that are accurate, then add your positions with achievements in your own words.
2. **Preferences →** check the defaults (Milwaukee, WI · 100% remote · $50,000 minimum · all four employment types) and save.
3. **Add a job →** paste a posting link and click **Fetch**, or paste the text. Review the draft and the detection notes, then **Save job**.
4. **Job detail →** read the requirement checks, then move the job through the statuses. After you submit on the employer's site, choose *Submitted*, tick the confirmation box and enter the reference number if one was shown.
5. **Data & logs →** export to Excel or back up the database.

To explore before entering real data: **Data & logs → Load sample data**. Remove it with **Remove sample data**.

### Previewing a folder import from the terminal
`cargo run --manifest-path src-tauri/Cargo.toml --example import_preview -- "/path/to/your folder"` prints the import report as JSON. It runs against an empty in-memory database and writes nothing.

## Backup and restore
* **Back up:** Data & logs → **Back up now** writes a single `.sqlite` file using SQLite's online backup API, which is safe while the app is running. Store it somewhere private, because it contains your personal data.
* **Restore:** Data & logs → **Restore from backup…** checks the file (schema, version, integrity), saves a safety copy of your current data to `…/com.projectnextchapter.app/backups/pre-restore-*.sqlite`, and then replaces the data.
* **Manual copy:** quit the app and copy `~/Library/Application Support/com.projectnextchapter.app/`.

## Troubleshooting
| Symptom | Fix |
|---|---|
| “must be run as the desktop app” banner | You opened the Vite page in a browser. Use `npm run app:dev`. |
| `cargo: command not found` | Run `export PATH="$HOME/.cargo/bin:$PATH"`, or reinstall Rust from rustup.rs. |
| “The site refused automated access (HTTP 403/429)” | The site blocks automated requests. Open the posting in your browser and use **Paste posting text**. |
| Link import gives a page title and messy text | The page has no structured job data. Edit the draft or paste the posting text instead. |
| “created by a newer version of the app” | Update the app. Older builds refuse to downgrade a newer database. |
| Something went wrong | Search the log in **Data & logs** (errors appear in red). The database file is listed under **Storage**. |
| macOS says the app is damaged or can't be opened | It is unsigned. Right-click and choose **Open**, or run `xattr -dr com.apple.quarantine "/Applications/Project Next Chapter.app"`. |

## Privacy
* No telemetry and no accounts. The only network request in Phase 1 is fetching a posting URL that you enter.
* `.gitignore` excludes databases, documents, exports, backups and `.env` files. Only sample data under `sample-data/` is tracked.
* Push only to a **private** GitHub repository, for example `gh repo create project-next-chapter --private --source . --push`.
