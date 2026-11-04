//! Career-document intake: text extraction and skill *suggestions* (never auto-verified).

use crate::error::{AppError, AppResult};
use crate::parse::decode_entities;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

pub const SKILL_VOCABULARY: &[&str] = &[
    "Smartsheet", "Smartsheet Control Center", "Smartsheet Data Shuttle", "Excel", "Office Scripts", "VBA",
    "Power Automate", "Power BI", "Power Apps", "Power Query", "SharePoint", "Microsoft Teams", "Microsoft 365",
    "Outlook", "OneDrive", "Visio", "Microsoft Project", "Google Sheets", "SQL", "Python", "Tableau", "Looker",
    "Jira", "Confluence", "ServiceNow", "Zendesk", "Salesforce", "Asana", "Monday.com", "Trello", "Active Directory",
    "Azure AD", "Intune", "ITIL", "Help Desk", "Technical Support", "Troubleshooting", "Root Cause Analysis",
    "User Acceptance Testing", "UAT", "Test Scripts", "Requirements Gathering", "Documentation",
    "Standard Operating Procedures", "Process Improvement", "Process Mapping", "Lean", "Six Sigma", "Kaizen",
    "Change Management", "Project Coordination", "Project Management", "Agile", "Scrum", "Kanban",
    "Stakeholder Management", "Stakeholder Communication", "Vendor Management", "Crisis Management",
    "Incident Management", "Emergency Management", "Business Continuity", "Data Analysis", "Data Cleanup",
    "Data Cleansing", "Data Validation", "Data Entry", "Reporting", "Dashboards", "KPI", "Pivot Tables",
    "XLOOKUP", "VLOOKUP", "Training", "Onboarding", "Workflow Automation", "Budget Tracking", "Scheduling",
];

static RE_PARA_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</w:p>").unwrap());
static RE_TAB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<w:tab\s*/>").unwrap());
static RE_BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<w:br\s*/>|<w:cr\s*/>").unwrap());
static RE_XML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Plain text from a Word document's main body (word/document.xml).
pub fn docx_text(bytes: &[u8]) -> AppResult<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| AppError::invalid(format!("Not a valid .docx file: {e}")))?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(|_| AppError::invalid("This .docx has no word/document.xml"))?
        .read_to_string(&mut xml)?;
    let s = RE_PARA_END.replace_all(&xml, "\n");
    let s = RE_TAB.replace_all(&s, "\t");
    let s = RE_BR.replace_all(&s, "\n");
    let s = RE_XML_TAG.replace_all(&s, "");
    let text = decode_entities(&s);
    Ok(text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string())
}

/// Returns (text, warning).
pub fn extract_text(path: &Path) -> AppResult<(String, Option<String>)> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let bytes = std::fs::read(path)?;
    match ext.as_str() {
        "docx" => Ok((docx_text(&bytes)?, None)),
        "txt" | "md" | "markdown" | "text" => Ok((String::from_utf8_lossy(&bytes).trim().to_string(), None)),
        "pdf" | "doc" | "rtf" | "pages" | "png" | "jpg" | "jpeg" => Ok((
            String::new(),
            Some(format!(
                "The .{ext} file was stored, but text extraction for this format is not available yet. \
                 Upload a .docx or .txt copy so the app can read its contents."
            )),
        )),
        _ => Err(AppError::invalid(format!("Unsupported file type: .{ext}"))),
    }
}

/// Skills from the vocabulary that literally appear in the text. These are suggestions only.
pub fn suggest_skills(text: &str) -> Vec<String> {
    SKILL_VOCABULARY
        .iter()
        .filter(|s| {
            Regex::new(&format!(r"(?i)(?:^|[^A-Za-z0-9]){}(?:$|[^A-Za-z0-9])", regex::escape(s)))
                .map(|r| r.is_match(text))
                .unwrap_or(false)
        })
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub fn make_docx(paragraphs: &[&str]) -> Vec<u8> {
        let body: String = paragraphs
            .iter()
            .map(|p| format!("<w:p><w:r><w:t>{p}</w:t></w:r></w:p>"))
            .collect();
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("word/document.xml", opts).unwrap();
            zw.write_all(xml.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn reads_docx_paragraphs() {
        let bytes = make_docx(&["Jordan Sample", "Built Smartsheet dashboards &amp; Excel Office Scripts"]);
        let t = docx_text(&bytes).unwrap();
        assert_eq!(t, "Jordan Sample\nBuilt Smartsheet dashboards & Excel Office Scripts");
    }

    #[test]
    fn rejects_non_docx() {
        assert!(docx_text(b"not a zip").is_err());
    }

    #[test]
    fn extract_by_extension() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("resume.txt");
        std::fs::write(&txt, "Led UAT for SharePoint migration").unwrap();
        assert_eq!(extract_text(&txt).unwrap().0, "Led UAT for SharePoint migration");
        let pdf = dir.path().join("resume.pdf");
        std::fs::write(&pdf, b"%PDF-1.4").unwrap();
        let (t, w) = extract_text(&pdf).unwrap();
        assert!(t.is_empty() && w.is_some());
        let exe = dir.path().join("x.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert!(extract_text(&exe).is_err());
    }

    #[test]
    fn suggests_only_present_skills() {
        let s = suggest_skills("Automated reports with Excel Office Scripts and Smartsheet; ran UAT.");
        assert!(s.contains(&"Excel".to_string()));
        assert!(s.contains(&"Office Scripts".to_string()));
        assert!(s.contains(&"Smartsheet".to_string()));
        assert!(s.contains(&"UAT".to_string()));
        assert!(!s.contains(&"Python".to_string()));
        // word boundaries: "SQLite" is not "SQL"
        assert!(!suggest_skills("Used SQLite").contains(&"SQL".to_string()));
    }

    #[test]
    fn hashes() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
