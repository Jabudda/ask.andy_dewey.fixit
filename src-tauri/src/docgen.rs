//! Renders layout-neutral blocks to an ATS-friendly .docx (single column, real text, standard
//! headings, Arial) and a matching PDF. No tables, text boxes, images or headers/footers —
//! applicant tracking systems read these reliably.

use crate::error::{AppError, AppResult};
use crate::tailoring::Block;
use std::path::Path;

const FONT: &str = "Arial";

pub fn write_docx(blocks: &[Block], path: &Path) -> AppResult<()> {
    use docx_rs::*;
    let run = |t: &str| Run::new().add_text(t).fonts(RunFonts::new().ascii(FONT).hi_ansi(FONT).cs(FONT).east_asia(FONT));
    let mut doc = Docx::new();
    for b in blocks {
        let p = match b {
            Block::Name(t) => Paragraph::new().add_run(run(t).bold().size(32)).align(AlignmentType::Center),
            Block::Contact(t) => Paragraph::new().add_run(run(t).size(19)).align(AlignmentType::Center),
            Block::Headline(t) => Paragraph::new().add_run(run(t).bold().size(21)).align(AlignmentType::Center),
            Block::Heading(t) => Paragraph::new().add_run(run(&t.to_uppercase()).bold().size(22)),
            Block::Role { title, meta } => {
                doc = doc.add_paragraph(Paragraph::new().add_run(run(title).bold().size(21)));
                Paragraph::new().add_run(run(meta).italic().size(20))
            }
            Block::Para(t) => {
                let mut p = Paragraph::new();
                for (i, line) in t.split('\n').enumerate() {
                    if i > 0 {
                        p = p.add_run(Run::new().add_break(BreakType::TextWrapping));
                    }
                    p = p.add_run(run(line).size(21));
                }
                p
            }
            Block::Bullet(t) => Paragraph::new()
                .add_run(run(&format!("•\t{t}")).size(21))
                .indent(Some(360), Some(SpecialIndentType::Hanging(360)), None, None),
            Block::Gap => Paragraph::new(),
        };
        doc = doc.add_paragraph(p);
    }
    let file = std::fs::File::create(path)?;
    doc.build().pack(file).map_err(|e| AppError::other(format!("Could not write Word file: {e}")))?;
    Ok(())
}

fn font_family() -> AppResult<genpdf::fonts::FontFamily<genpdf::fonts::FontData>> {
    let dir = Path::new("/System/Library/Fonts/Supplemental");
    let load = |f: &str| {
        genpdf::fonts::FontData::load(dir.join(f), None).map_err(|e| AppError::other(format!("Could not load font {f}: {e}")))
    };
    Ok(genpdf::fonts::FontFamily {
        regular: load("Arial.ttf")?,
        bold: load("Arial Bold.ttf")?,
        italic: load("Arial Italic.ttf")?,
        bold_italic: load("Arial Bold Italic.ttf")?,
    })
}

pub fn write_pdf(blocks: &[Block], title: &str, path: &Path) -> AppResult<()> {
    use genpdf::{elements, style::Style, Alignment, Element};
    let mut doc = genpdf::Document::new(font_family()?);
    doc.set_title(title);
    doc.set_font_size(10);
    doc.set_line_spacing(1.2);
    let mut deco = genpdf::SimplePageDecorator::new();
    deco.set_margins(18);
    doc.set_page_decorator(deco);
    for b in blocks {
        match b {
            Block::Name(t) => doc.push(elements::Paragraph::new(t.as_str()).aligned(Alignment::Center).styled(Style::new().bold().with_font_size(17))),
            Block::Contact(t) => doc.push(elements::Paragraph::new(t.as_str()).aligned(Alignment::Center).styled(Style::new().with_font_size(9))),
            Block::Headline(t) => {
                doc.push(elements::Break::new(0.3));
                doc.push(elements::Paragraph::new(t.as_str()).aligned(Alignment::Center).styled(Style::new().bold().with_font_size(11)));
            }
            Block::Heading(t) => {
                doc.push(elements::Break::new(0.6));
                doc.push(elements::Paragraph::new(t.to_uppercase()).styled(Style::new().bold().with_font_size(11)));
            }
            Block::Role { title, meta } => {
                doc.push(elements::Break::new(0.35));
                doc.push(elements::Paragraph::new(title.as_str()).styled(Style::new().bold().with_font_size(11)));
                doc.push(elements::Paragraph::new(meta.as_str()).styled(Style::new().italic().with_font_size(10)));
            }
            Block::Para(t) => {
                for line in t.split('\n') {
                    doc.push(elements::Paragraph::new(line));
                }
                doc.push(elements::Break::new(0.25));
            }
            Block::Bullet(t) => doc.push(elements::BulletPoint::new(elements::Paragraph::new(t.as_str())).with_bullet("•").padded(genpdf::Margins::trbl(0, 0, 0.6, 2))),
            Block::Gap => doc.push(elements::Break::new(0.6)),
        }
    }
    doc.render_to_file(path).map_err(|e| AppError::other(format!("Could not write PDF: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tailoring::Block;

    fn sample() -> Vec<Block> {
        vec![
            Block::Name("Jordan Sample".into()),
            Block::Contact("Milwaukee, Wisconsin  |  jordan@example.com".into()),
            Block::Heading("Professional Experience".into()),
            Block::Role { title: "Business Operations Specialist".into(), meta: "Contoso  |  Nov 2023 – Present".into() },
            Block::Bullet("Built Smartsheet dashboards and testing trackers".into()),
            Block::Para("Hiring Team\nHSAG".into()),
        ]
    }

    #[test]
    fn docx_contains_real_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.docx");
        write_docx(&sample(), &p).unwrap();
        let text = crate::documents::docx_text(&std::fs::read(&p).unwrap()).unwrap();
        assert!(text.contains("Jordan Sample"));
        assert!(text.contains("PROFESSIONAL EXPERIENCE"));
        assert!(text.contains("Built Smartsheet dashboards and testing trackers"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pdf_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("r.pdf");
        write_pdf(&sample(), "Resume", &p).unwrap();
        let (text, warn) = crate::documents::extract_text(&p).unwrap();
        assert!(warn.is_none());
        assert!(text.contains("Jordan Sample"), "{text}");
        assert!(text.contains("Built Smartsheet dashboards"));
    }
}
