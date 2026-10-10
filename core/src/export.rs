//! Exports: DOCX for the office, Markdown with the full transcript.
//!
//! Headings follow the minutes' language. The DOCX is written as plain OOXML
//! with the styles of the python-docx default template the earlier version used
//! (Title, Heading 1, List Bullet, Light Grid Accent 1, Calibri 11 pt).

use anyhow::Result;
use std::io::Write;
use std::path::Path;

use crate::minutes::Minutes;
use crate::prompts::label;
use crate::render::to_markdown;
use crate::store::Meeting;
use crate::text::py_round;
use crate::transcript::clock;

const FOREST: &str = "23362C";
const MUTED: &str = "65736B";

fn language(m: &Meeting) -> &str {
    m.notulen.as_ref().map_or("id", |n| n.language.as_str())
}

pub fn full_markdown(meeting: &Meeting, date: &str) -> String {
    let lang = language(meeting);
    let mut parts = Vec::new();
    match &meeting.notulen {
        Some(n) => parts.push(to_markdown(n, date, &meeting.title)),
        None => parts.push(format!("# {}\n_{date}_\n", meeting.title)),
    }
    if !meeting.transcript.is_empty() {
        parts.push(format!("## {}\n", label(lang, "transcript")));
        parts.extend(meeting.transcript.iter().map(|u| format!("**[{}] {}:** {}  ", clock(u.start), u.speaker, u.text)));
    }
    parts.join("\n") + "\n"
}

// ---------------------------------------------------------------------------
// DOCX
// ---------------------------------------------------------------------------

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 forbids most control characters.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[derive(Default)]
struct Run<'a> {
    text: &'a str,
    bold: bool,
    color: Option<&'a str>,
}

fn run(r: &Run) -> String {
    let mut props = String::new();
    if r.bold {
        props.push_str("<w:b/>");
    }
    if let Some(c) = r.color {
        props.push_str(&format!("<w:color w:val=\"{c}\"/>"));
    }
    let props = if props.is_empty() { String::new() } else { format!("<w:rPr>{props}</w:rPr>") };
    let space = if r.text.starts_with(' ') || r.text.ends_with(' ') { " xml:space=\"preserve\"" } else { "" };
    format!("<w:r>{props}<w:t{space}>{}</w:t></w:r>", esc(r.text))
}

fn para(style: Option<&str>, runs: &[Run]) -> String {
    let p = style.map_or(String::new(), |s| format!("<w:pPr><w:pStyle w:val=\"{s}\"/></w:pPr>"));
    format!("<w:p>{p}{}</w:p>", runs.iter().map(run).collect::<String>())
}

fn text_para(text: &str) -> String {
    para(None, &[Run { text, ..Default::default() }])
}

fn heading(text: &str, title: bool) -> String {
    para(Some(if title { "Title" } else { "Heading1" }), &[Run { text, color: Some(FOREST), ..Default::default() }])
}

fn bullets(items: &[String], none: &str) -> String {
    let none = [none.to_string()];
    let items = if items.is_empty() { &none[..] } else { items };
    items.iter().map(|i| para(Some("ListBullet"), &[Run { text: i, ..Default::default() }])).collect()
}

fn table(rows: &[[String; 4]]) -> String {
    let cell = |t: &str| format!("<w:tc><w:tcPr><w:tcW w:type=\"dxa\" w:w=\"2160\"/></w:tcPr>{}</w:tc>", text_para(t));
    let body: String = rows.iter().map(|r| format!("<w:tr>{}</w:tr>", r.iter().map(|c| cell(c)).collect::<String>())).collect();
    format!(
        "<w:tbl><w:tblPr><w:tblStyle w:val=\"LightGrid-Accent1\"/><w:tblW w:type=\"auto\" w:w=\"0\"/>\
         <w:tblLook w:firstColumn=\"1\" w:firstRow=\"1\" w:lastColumn=\"0\" w:lastRow=\"0\" w:noHBand=\"0\" w:noVBand=\"1\" w:val=\"04A0\"/>\
         </w:tblPr><w:tblGrid>{}</w:tblGrid>{body}</w:tbl>",
        "<w:gridCol w:w=\"2160\"/>".repeat(4)
    )
}

fn body(meeting: &Meeting, m: &Minutes, date: &str, with_transcript: bool) -> String {
    let lang = language(meeting);
    let t = |k| label(lang, k);
    let mut out = String::new();
    let title = [meeting.title.as_str(), m.title.as_str(), t("untitled")].into_iter().find(|s| !s.is_empty()).unwrap_or("");
    out += &heading(title, true);
    if !date.is_empty() {
        out += &para(None, &[Run { text: date, color: Some(MUTED), ..Default::default() }]);
    }

    out += &heading(t("summary"), false);
    out += &text_para(if m.overview.is_empty() { "-" } else { &m.overview });
    if !m.highlights.is_empty() {
        out += &bullets(&m.highlights, t("none"));
    }

    out += &heading(t("actions"), false);
    if m.actions.is_empty() {
        out += &text_para(t("none"));
    } else {
        let mut rows = vec![[t("owner"), t("task"), t("due_col"), t("time")].map(String::from)];
        for a in &m.actions {
            rows.push([
                if a.owner.is_empty() { "-".into() } else { a.owner.clone() },
                format!("{}{}", if a.done { "✓ " } else { "" }, a.task),
                if a.due.is_empty() { "-".into() } else { a.due.clone() },
                if a.refs.is_empty() { "-".into() } else { clock(a.start) },
            ]);
        }
        out += &table(&rows);
    }

    out += &heading(t("decisions"), false);
    out += &bullets(&m.decisions.iter().map(|d| d.text.clone()).collect::<Vec<_>>(), t("none"));
    if !m.questions.is_empty() {
        out += &heading(t("questions"), false);
        out += &bullets(&m.questions.iter().map(|q| q.text.clone()).collect::<Vec<_>>(), t("none"));
    }
    if !m.chapters.is_empty() {
        out += &heading(t("chapters"), false);
        for c in &m.chapters {
            let time = format!("{}  ", clock(c.start));
            out += &para(None, &[Run { text: &time, color: Some(MUTED), ..Default::default() }, Run { text: &c.title, bold: true, ..Default::default() }]);
            out += &text_para(&c.summary);
        }
    }
    if !m.speakers.is_empty() {
        out += &heading(t("speakers"), false);
        let lines: Vec<String> =
            m.speakers.iter().map(|s| format!("{}: {}% {}", s.name, py_round(s.share * 100.0, 0) as i64, t("talk"))).collect();
        out += &bullets(&lines, t("none"));
    }
    if with_transcript && !meeting.transcript.is_empty() {
        out += "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";
        out += &heading(t("transcript"), false);
        for u in &meeting.transcript {
            let label = format!("[{}] {}: ", clock(u.start), u.speaker);
            out += &para(None, &[Run { text: &label, bold: true, color: Some(MUTED) }, Run { text: &u.text, ..Default::default() }]);
        }
    }
    out
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;
const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/></Relationships>"#;
const SECTION: &str = r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800" w:header="720" w:footer="720" w:gutter="0"/><w:cols w:space="720"/><w:docGrid w:linePitch="360"/></w:sectPr>"#;

fn core_props(title: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Lariksa</dc:creator></cp:coreProperties>"#,
        esc(title)
    )
}

/// Write the notulen (and transcript) as a Word document. Requires notulen.
pub fn write_docx(meeting: &Meeting, target: &Path, date: &str) -> Result<()> {
    let m = meeting.notulen.as_ref().ok_or_else(|| anyhow::anyhow!("{}", crate::i18n::tr("Notulen belum tersedia.", "No minutes yet.")))?;
    let document = format!(
        "{}<w:body>{}{SECTION}</w:body></w:document>",
        include_str!("../assets/docx/document_head.xml"),
        body(meeting, m, date, true)
    );
    let tmp = target.with_extension("docx.tmp");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in [
            ("[Content_Types].xml", CONTENT_TYPES.to_string()),
            ("_rels/.rels", ROOT_RELS.to_string()),
            ("docProps/core.xml", core_props(&meeting.title)),
            ("word/_rels/document.xml.rels", DOC_RELS.to_string()),
            ("word/document.xml", document),
            ("word/styles.xml", include_str!("../assets/docx/styles.xml").to_string()),
            ("word/numbering.xml", include_str!("../assets/docx/numbering.xml").to_string()),
            ("word/theme/theme1.xml", include_str!("../assets/docx/theme1.xml").to_string()),
        ] {
            z.start_file(name, opts)?;
            z.write_all(data.as_bytes())?;
        }
        z.finish()?;
    }
    std::fs::rename(&tmp, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minutes::{Action, Point};
    use crate::transcript::{Utterance, ME, OTHERS};
    use std::io::Read;

    fn meeting(with_notulen: bool) -> Meeting {
        Meeting {
            id: "x".into(),
            title: "Rapat".into(),
            created_at: "2026-10-09T10:00:00".into(),
            status: "done".into(),
            duration: 0.0,
            source: "record".into(),
            language: "id".into(),
            error: String::new(),
            transcript: vec![Utterance::new(ME, 0.0, 2.0, "Halo"), Utterance::new(OTHERS, 3.0, 5.0, "Pagi")],
            notulen: with_notulen.then(|| Minutes {
                title: "Kickoff".into(),
                overview: "Mulai.".into(),
                decisions: vec![Point::new("Go", 0.0, vec![])],
                actions: vec![Action::new("Siapkan deck", "Sari", "Senin", 3.0, vec![2])],
                ..Default::default()
            }),
            timings: Default::default(),
        }
    }

    fn document_xml(path: &Path) -> String {
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut s = String::new();
        z.by_name("word/document.xml").unwrap().read_to_string(&mut s).unwrap();
        s
    }

    /// Paragraph texts and table cells, the way python-docx reads them back.
    fn texts(xml: &str) -> (Vec<String>, Vec<Vec<String>>) {
        let t = regex::Regex::new(r"<w:t(?: [^>]*)?>([^<]*)</w:t>").unwrap();
        let para_text = |p: &str| t.captures_iter(p).map(|c| c[1].to_string()).collect::<String>();
        let without_tables = regex::Regex::new(r"(?s)<w:tbl>.*?</w:tbl>").unwrap().replace_all(xml, "");
        let paras = without_tables.split("</w:p>").map(para_text).collect();
        let rows = xml
            .split("<w:tr>")
            .skip(1)
            .map(|r| r.split("</w:tr>").next().unwrap().split("</w:tc>").filter(|c| c.contains("<w:t")).map(para_text).collect())
            .collect();
        (paras, rows)
    }

    #[test]
    fn docx_contains_notulen_table_and_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("n.docx");
        write_docx(&meeting(true), &target, "9 Okt 2026").unwrap();
        let (paras, rows) = texts(&document_xml(&target));
        let text = paras.join("\n");
        assert!(text.contains("Rapat") && text.contains("Mulai.") && text.contains("[00:03] Peserta: Pagi"));
        assert_eq!(rows[1], ["Sari", "Siapkan deck", "Senin", "00:03"]);
    }

    #[test]
    fn english_docx_has_english_headings() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = meeting(true);
        m.notulen.as_mut().unwrap().language = "en".into();
        let target = dir.path().join("en.docx");
        write_docx(&m, &target, "").unwrap();
        let (paras, rows) = texts(&document_xml(&target));
        let text = paras.join("\n");
        assert!(text.contains("Summary") && text.contains("Transcript") && !text.contains("Ringkasan"));
        assert_eq!(rows[0], ["Owner", "Task", "Due", "Time"]);
    }

    #[test]
    fn docx_escapes_model_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = meeting(true);
        m.notulen.as_mut().unwrap().overview = "A & B <ok> \u{1}".into();
        let target = dir.path().join("x.docx");
        write_docx(&m, &target, "").unwrap();
        assert!(document_xml(&target).contains("A &amp; B &lt;ok&gt; "));
    }

    #[test]
    fn markdown_includes_transcript_even_without_notulen() {
        let md = full_markdown(&meeting(false), "9 Okt");
        assert!(md.starts_with("# Rapat"));
        assert!(md.contains("**[00:00] Saya:** Halo"));
    }
}
