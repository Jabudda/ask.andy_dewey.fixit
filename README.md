# Project Next Chapter

A local-first macOS desktop app for running a job search: keep a verified career profile, import and check job postings against your requirements, and track every application through to the outcome. You stay in control of every application, and the app makes no claims about you that you have not verified.

Built with **Tauri v2 · React · TypeScript · Rust · SQLite**. It runs natively on Apple Silicon.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the architecture, database design and phase plan.

---

## Phase 1 status (this release)

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

* **Rust (40 tests):** parsing (JSON-LD, pasted text, remote, Wisconsin, pay, employment type), eligibility rules, duplicate detection, status guards, preferences re-check, milestone isolation, dashboard metrics, follow-ups, profile CRUD, document import and extraction, CSV/XLSX export, backup/restore round-trip, rejection of non-backup files, log search, real PDF text extraction, and folder import (dry run writes nothing, statuses and dates, folder-based matching, no matching on document text, certificate parsing, idempotent re-run).
* **TypeScript (8 tests):** countdown, money and pay formatting, annualisation, list parsing, filtering and sorting.

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
