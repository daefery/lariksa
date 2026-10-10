//! Render minutes as Markdown and WhatsApp text, with headings in the minutes' language.

use crate::minutes::{Action, Minutes};
use crate::prompts::label;
use crate::text::py_round;
use crate::transcript::clock;

fn pct(share: f64) -> i64 {
    py_round(share * 100.0, 0) as i64
}

pub fn to_markdown(m: &Minutes, date: &str, title: &str) -> String {
    let t = |k| label(&m.language, k);
    let heading = [title, &m.title, t("untitled")].into_iter().find(|s| !s.is_empty()).unwrap_or("");
    let mut out = vec![format!("# {heading}")];
    if !date.is_empty() {
        out.push(format!("_{date}_"));
    }
    out.extend(["".into(), format!("## {}", t("summary")), if m.overview.is_empty() { "-".into() } else { m.overview.clone() }]);
    if !m.highlights.is_empty() {
        out.push(String::new());
        out.extend(m.highlights.iter().map(|h| format!("- {h}")));
    }
    out.extend(["".into(), format!("## {}", t("actions"))]);
    if m.actions.is_empty() {
        out.push(format!("- {}", t("none")));
    }
    for a in &m.actions {
        let mut line = format!(
            "- [{}] **{}**: {}",
            if a.done { "x" } else { " " },
            if a.owner.is_empty() { t("unassigned") } else { &a.owner },
            a.task
        );
        if !a.due.is_empty() {
            line += &format!(" _({}: {})_", t("due"), a.due);
        }
        if !a.refs.is_empty() {
            line += &format!(" `{}`", clock(a.start));
        }
        out.push(line);
    }
    out.extend(["".into(), format!("## {}", t("decisions"))]);
    if m.decisions.is_empty() {
        out.push(format!("- {}", t("none")));
    }
    for d in &m.decisions {
        out.push(format!("- {}{}", d.text, if d.refs.is_empty() { String::new() } else { format!(" `{}`", clock(d.start)) }));
    }
    if !m.questions.is_empty() {
        out.extend(["".into(), format!("## {}", t("questions"))]);
        for q in &m.questions {
            out.push(format!("- {}{}", q.text, if q.refs.is_empty() { String::new() } else { format!(" `{}`", clock(q.start)) }));
        }
    }
    if !m.chapters.is_empty() {
        out.extend(["".into(), format!("## {}", t("chapters"))]);
        for c in &m.chapters {
            out.extend(["".into(), format!("### `{}` {}", clock(c.start), c.title), c.summary.clone()]);
        }
    }
    if !m.speakers.is_empty() {
        out.extend(["".into(), format!("## {}", t("speakers"))]);
        out.extend(m.speakers.iter().map(|s| format!("- {}: {}% {}", s.name, pct(s.share), t("talk"))));
    }
    out.join("\n") + "\n"
}

/// Actions grouped by owner, in order of first appearance.
fn by_owner(m: &Minutes) -> Vec<(String, Vec<&Action>)> {
    let mut groups: Vec<(String, Vec<&Action>)> = Vec::new();
    for a in &m.actions {
        match groups.iter_mut().find(|(o, _)| *o == a.owner) {
            Some((_, items)) => items.push(a),
            None => groups.push((a.owner.clone(), vec![a])),
        }
    }
    groups
}

/// Short and skimmable: summary, highlights, actions by person, decisions.
pub fn to_whatsapp(m: &Minutes, date: &str, title: &str) -> String {
    let t = |k| label(&m.language, k);
    let heading = [title, &m.title, t("untitled")].into_iter().find(|s| !s.is_empty()).unwrap_or("");
    let mut out = vec![format!("*{heading}*{}", if date.is_empty() { String::new() } else { format!(" - {date}") })];
    if !m.overview.is_empty() {
        out.extend(["".into(), m.overview.clone()]);
    }
    if !m.highlights.is_empty() {
        out.push(String::new());
        out.extend(m.highlights.iter().map(|h| format!("• {h}")));
    }
    if !m.actions.is_empty() {
        out.extend(["".into(), format!("*{}*", t("actions"))]);
        for (owner, items) in by_owner(m) {
            out.push(format!("_{}_", if owner.is_empty() { t("unassigned") } else { &owner }));
            for a in items {
                out.push(format!("• {}{}", a.task, if a.due.is_empty() { String::new() } else { format!(" ({}: {})", t("due"), a.due) }));
            }
        }
    }
    if !m.decisions.is_empty() {
        out.extend(["".into(), format!("*{}*", t("decisions"))]);
        out.extend(m.decisions.iter().map(|d| format!("• {}", d.text)));
    }
    if !m.questions.is_empty() {
        out.extend(["".into(), format!("*{}*", t("questions"))]);
        out.extend(m.questions.iter().map(|q| format!("• {}", q.text)));
    }
    out.join("\n").trim().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minutes::{Chapter, Point, SpeakerStat};

    fn sample(language: &str) -> Minutes {
        Minutes {
            title: "Sinkronisasi rilis".into(),
            overview: "Tim membahas rilis.".into(),
            highlights: vec!["Rilis Jumat".into()],
            chapters: vec![Chapter { title: "Rilis".into(), summary: "Raka menjelaskan.".into(), start: 65.0, refs: vec![2] }],
            actions: vec![
                Action::new("Cek bug login", "Budi", "Kamis", 3.0, vec![2]),
                Action::new("Siapkan deck", "", "", 0.0, vec![]),
                Action::new("Deploy", "Budi", "", 9.0, vec![3]),
            ],
            decisions: vec![Point::new("Rilis hari Jumat", 3725.0, vec![9])],
            questions: vec![],
            speakers: vec![SpeakerStat { name: "Budi".into(), seconds: 10.0, share: 0.625, turns: 2 }],
            language: language.into(),
            version: 2,
        }
    }

    #[test]
    fn markdown_matches_python_output() {
        let md = to_markdown(&sample("id"), "9 Okt 2026", "");
        let want = "# Sinkronisasi rilis\n_9 Okt 2026_\n\n## Ringkasan\nTim membahas rilis.\n\n- Rilis Jumat\n\n## Action items\n- [ ] **Budi**: Cek bug login _(deadline: Kamis)_ `00:03`\n- [ ] **Tanpa PIC**: Siapkan deck\n- [ ] **Budi**: Deploy `00:09`\n\n## Keputusan\n- Rilis hari Jumat `1:02:05`\n\n## Pembahasan\n\n### `01:05` Rilis\nRaka menjelaskan.\n\n## Peserta\n- Budi: 62% bicara\n";
        assert_eq!(md, want);
    }

    #[test]
    fn whatsapp_groups_by_owner_in_english() {
        let wa = to_whatsapp(&sample("en"), "", "Weekly");
        let want = "*Weekly*\n\nTim membahas rilis.\n\n• Rilis Jumat\n\n*Action items*\n_Budi_\n• Cek bug login (due: Kamis)\n• Deploy\n_Unassigned_\n• Siapkan deck\n\n*Decisions*\n• Rilis hari Jumat\n";
        assert_eq!(wa, want);
    }
}
