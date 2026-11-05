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
    for (i, b) in blocks.iter().enumerate() {
        // Word keeps a heading with what follows it, and a role with all of its bullets.
        let next_is_bullet = matches!(blocks.get(i + 1), Some(Block::Bullet(_)));
        let p = match b {
            Block::Name(t) => Paragraph::new().add_run(run(t).bold().size(32)).align(AlignmentType::Center),
            Block::Contact(t) => Paragraph::new().add_run(run(t).size(19)).align(AlignmentType::Center),
            Block::Headline(t) => Paragraph::new().add_run(run(t).bold().size(21)).align(AlignmentType::Center),
            Block::Heading(t) => Paragraph::new().add_run(run(&t.to_uppercase()).bold().size(22)).keep_next(true),
            Block::Role { title, meta } => {
                doc = doc.add_paragraph(Paragraph::new().add_run(run(title).bold().size(21)).keep_next(true).keep_lines(true));
                Paragraph::new().add_run(run(meta).italic().size(20)).keep_next(next_is_bullet).keep_lines(true)
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
                .indent(Some(360), Some(SpecialIndentType::Hanging(360)), None, None)
                .keep_lines(true)
                .keep_next(next_is_bullet),
            Block::Project { title, text } => Paragraph::new().add_run(run(&format!("{title}: ")).bold().size(21)).add_run(run(text).size(21)).keep_lines(true),
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

const MARGIN_MM: f64 = 18.0;
/// US Letter (215.9 × 279.4 mm), the standard for US employers.
const PAPER: genpdf::PaperSize = genpdf::PaperSize::Letter;
const PAPER_HEIGHT_MM: f64 = 279.4;

/// Blocks that must stay on one page: a role with all of its bullets, and a heading with the
/// first thing under it. Everything else is a group of one.
pub fn keep_together_groups(blocks: &[Block]) -> Vec<std::ops::Range<usize>> {
    let mut groups: Vec<std::ops::Range<usize>> = vec![];
    // (group has a role, group is only headings so far)
    let mut state = (false, false);
    for (i, b) in blocks.iter().enumerate() {
        let extend = match b {
            Block::Heading(_) => state.1,
            Block::Role { .. } => state.1,
            Block::Bullet(_) => state.0,
            Block::Name(_) | Block::Contact(_) | Block::Headline(_) => false,
            Block::Para(_) | Block::Project { .. } | Block::Gap => state.1,
        };
        match groups.last_mut() {
            Some(g) if extend => g.end = i + 1,
            _ => groups.push(i..i + 1),
        }
        state = match b {
            Block::Heading(_) => (false, true),
            Block::Role { .. } => (true, false),
            Block::Bullet(_) => (state.0, false),
            _ => (false, false),
        };
    }
    groups
}

fn push_block(layout: &mut genpdf::elements::LinearLayout, b: &Block) {
    use genpdf::{elements, style::Style, Alignment, Element};
    match b {
        Block::Name(t) => layout.push(elements::Paragraph::new(t.as_str()).aligned(Alignment::Center).styled(Style::new().bold().with_font_size(17))),
        Block::Contact(t) => layout.push(elements::Paragraph::new(t.as_str()).aligned(Alignment::Center).styled(Style::new().with_font_size(9))),
        Block::Headline(t) => {
            layout.push(elements::Break::new(0.3));
            layout.push(elements::Paragraph::new(t.as_str()).aligned(Alignment::Center).styled(Style::new().bold().with_font_size(11)));
        }
        Block::Heading(t) => {
            layout.push(elements::Break::new(0.6));
            layout.push(elements::Paragraph::new(t.to_uppercase()).styled(Style::new().bold().with_font_size(11)));
        }
        Block::Role { title, meta } => {
            layout.push(elements::Break::new(0.35));
            layout.push(elements::Paragraph::new(title.as_str()).styled(Style::new().bold().with_font_size(11)));
            layout.push(elements::Paragraph::new(meta.as_str()).styled(Style::new().italic().with_font_size(10)));
        }
        Block::Para(t) => {
            for line in t.split('\n') {
                layout.push(elements::Paragraph::new(line));
            }
            layout.push(elements::Break::new(0.25));
        }
        Block::Bullet(t) => layout.push(elements::BulletPoint::new(elements::Paragraph::new(t.as_str())).with_bullet("•").padded(genpdf::Margins::trbl(0, 0, 0.6, 2))),
        Block::Project { title, text } => {
            let mut para = elements::Paragraph::default();
            para.push_styled(format!("{title}: "), Style::new().bold());
            para.push(text.as_str());
            layout.push(para);
            layout.push(elements::Break::new(0.2));
        }
        Block::Gap => layout.push(elements::Break::new(0.6)),
    }
}

type SplitLog = std::rc::Rc<std::cell::RefCell<Vec<usize>>>;

/// Renders a group and records it if it began part-way down a page and ran onto the next one.
struct Probe {
    inner: genpdf::elements::LinearLayout,
    group: usize,
    first: bool,
    log: SplitLog,
}

impl genpdf::Element for Probe {
    fn render(&mut self, context: &genpdf::Context, area: genpdf::render::Area<'_>, style: genpdf::style::Style) -> Result<genpdf::RenderResult, genpdf::error::Error> {
        let at_top = area.size().height >= genpdf::Mm::from(PAPER_HEIGHT_MM - 2.0 * MARGIN_MM - 1.0);
        let r = self.inner.render(context, area, style)?;
        if self.first && r.has_more && !at_top {
            self.log.borrow_mut().push(self.group);
        }
        self.first = false;
        Ok(r)
    }
}

fn render_pdf(blocks: &[Block], title: &str, breaks_before: &std::collections::BTreeSet<usize>) -> AppResult<(Vec<u8>, Vec<usize>)> {
    use genpdf::elements;
    let mut doc = genpdf::Document::new(font_family()?);
    doc.set_title(title);
    doc.set_paper_size(PAPER);
    doc.set_font_size(10);
    doc.set_line_spacing(1.2);
    let mut deco = genpdf::SimplePageDecorator::new();
    deco.set_margins(MARGIN_MM);
    doc.set_page_decorator(deco);
    let log: SplitLog = Default::default();
    for (g, range) in keep_together_groups(blocks).into_iter().enumerate() {
        if breaks_before.contains(&g) {
            doc.push(elements::PageBreak::new());
        }
        let mut layout = elements::LinearLayout::vertical();
        for b in &blocks[range] {
            push_block(&mut layout, b);
        }
        doc.push(Probe { inner: layout, group: g, first: true, log: log.clone() });
    }
    let mut out = vec![];
    doc.render(&mut out).map_err(|e| AppError::other(format!("Could not write PDF: {e}")))?;
    let splits = log.borrow().clone();
    Ok((out, splits))
}

/// Lays the PDF out, then moves any role (or heading) that would straddle a page break to the
/// next page, repeating until nothing straddles. Returns the PDF and the groups moved.
pub fn layout_pdf(blocks: &[Block], title: &str) -> AppResult<(Vec<u8>, Vec<usize>)> {
    let mut breaks = std::collections::BTreeSet::new();
    for _ in 0..8 {
        let (bytes, splits) = render_pdf(blocks, title, &breaks)?;
        let new: Vec<usize> = splits.into_iter().filter(|g| !breaks.contains(g)).collect();
        if new.is_empty() {
            return Ok((bytes, breaks.into_iter().collect()));
        }
        // Fix the earliest split first: moving it shifts everything after it.
        breaks.insert(new[0]);
    }
    let (bytes, _) = render_pdf(blocks, title, &breaks)?;
    Ok((bytes, breaks.into_iter().collect()))
}

pub fn write_pdf(blocks: &[Block], title: &str, path: &Path) -> AppResult<()> {
    let (bytes, _) = layout_pdf(blocks, title)?;
    std::fs::write(path, bytes)?;
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

    #[test]
    fn roles_and_headings_are_grouped_with_what_follows() {
        let b = vec![
            Block::Name("N".into()),
            Block::Heading("Experience".into()),
            Block::Role { title: "A".into(), meta: "m".into() },
            Block::Bullet("1".into()),
            Block::Bullet("2".into()),
            Block::Role { title: "B".into(), meta: "m".into() },
            Block::Heading("Projects".into()),
            Block::Project { title: "P".into(), text: "t".into() },
            Block::Project { title: "Q".into(), text: "t".into() },
        ];
        assert_eq!(keep_together_groups(&b), vec![0..1, 1..5, 5..6, 6..8, 8..9]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn no_role_straddles_a_page_break() {
        let mut b = vec![Block::Name("Jordan Sample".into()), Block::Heading("Professional Experience".into())];
        for r in 0..9 {
            b.push(Block::Role { title: format!("Role {r}"), meta: "Contoso  |  2020 – 2024".into() });
            for k in 0..5 {
                b.push(Block::Bullet(format!("Role {r} bullet {k}: built dashboards and trackers that kept leadership informed and teams on schedule across sites.")));
            }
        }
        let (_, moved) = layout_pdf(&b, "Resume").unwrap();
        assert!(!moved.is_empty(), "this layout needs at least one role moved to the next page");
        // After moving, a fresh layout pass finds nothing straddling a page.
        let set: std::collections::BTreeSet<usize> = moved.iter().copied().collect();
        let (_, splits) = render_pdf(&b, "Resume", &set).unwrap();
        assert!(splits.iter().all(|g| set.contains(g)), "{splits:?}");
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
