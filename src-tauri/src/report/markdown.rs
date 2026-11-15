//! The report as Markdown. Every text that reaches this module is already cleaned and masked;
//! here it is only escaped, so a session title, a file path or a commit subject can never turn
//! into Markdown structure, and an unclosed code fence or raw HTML in an assistant text cannot
//! swallow the rest of the report. Raw HTML in prompts and assistant texts is shown as text, as
//! the terminal showed it, never rendered by a viewer.

use super::{Item, Labels, Report};
use crate::civil::civil_from_days;

/// `2026-11-14 11:05 UTC+01:00`: the local time of the window that asked for the report.
pub fn local_time(millis: i64, offset_minutes: i32) -> String {
    let local = millis.div_euclid(1000) + i64::from(offset_minutes) * 60;
    let (year, month, day) = civil_from_days(local.div_euclid(86_400));
    let rest = local.rem_euclid(86_400);
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let offset = offset_minutes.unsigned_abs();
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC{sign}{:02}:{:02}",
        rest / 3600,
        rest / 60 % 60,
        offset / 60,
        offset % 60
    )
}

/// `45 s`, `12 min`, `2 h 05 min`.
pub fn duration(millis: i64) -> String {
    let seconds = millis.max(0) / 1000;
    if seconds < 60 {
        format!("{seconds} s")
    } else if seconds < 3600 {
        format!("{} min", seconds / 60)
    } else {
        format!("{} h {:02} min", seconds / 3600, seconds / 60 % 60)
    }
}

/// One line of plain text: line breaks become spaces, Markdown's punctuation is escaped.
pub fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\r' | '\n' | '\t' => out.push(' '),
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~' | '!' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// A code span that holds `text` verbatim on one line: its fence is longer than any run of
/// backticks inside.
pub fn code(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if matches!(c, '\r' | '\n') { ' ' } else { c })
        .collect();
    let mut longest = 0;
    let mut run = 0;
    for c in line.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if line.starts_with('`') || line.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{line}{pad}{fence}")
}

/// A fence line (CommonMark): its mark and length, whether it may open a code block (a backtick
/// fence's info string holds no backtick) and whether it may close one (nothing follows).
struct Fence {
    mark: char,
    count: usize,
    opens: bool,
    closes: bool,
}

fn fence(line: &str) -> Option<Fence> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let mark = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let count = rest.chars().take_while(|c| *c == mark).count();
    if count < 3 {
        return None;
    }
    // Both marks are ASCII, so `count` is a byte index.
    let after = &rest[count..];
    Some(Fence {
        mark,
        count,
        opens: mark == '~' || !after.contains('`'),
        closes: after.trim().is_empty(),
    })
}

/// An indented code line: four spaces or a tab before the text.
fn is_indented_code(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

/// One line of prose outside code blocks: `<` becomes `&lt;` outside code spans, so raw HTML
/// in an agent's text shows as the text the terminal showed and an unclosed `<!--` or `<pre>`
/// cannot hide the rest of the report. `![` and `](` are escaped there too: an image would load
/// from wherever the agent's text points as soon as the report is opened, and an inline link
/// would hide its target behind other words. Backslash escapes stay as they are.
fn escape_html(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if chars.get(i + 1) == Some(&'<') => {
                out.push_str("\\\\&lt;");
                i += 2;
            }
            '\\' => {
                out.push('\\');
                if let Some(next) = chars.get(i + 1) {
                    out.push(*next);
                }
                i += 2;
            }
            '<' => {
                out.push_str("&lt;");
                i += 1;
            }
            '!' if chars.get(i + 1) == Some(&'[') => {
                out.push_str("!\\[");
                i += 2;
            }
            ']' if chars.get(i + 1) == Some(&'(') => {
                out.push_str("]\\(");
                i += 2;
            }
            '`' => {
                let run = chars[i..].iter().take_while(|c| **c == '`').count();
                // A code span ends at the next run of exactly as many backticks.
                let mut end = None;
                let mut j = i + run;
                while j < chars.len() {
                    if chars[j] == '`' {
                        let other = chars[j..].iter().take_while(|c| **c == '`').count();
                        if other == run {
                            end = Some(j + other);
                            break;
                        }
                        j += other;
                    } else {
                        j += 1;
                    }
                }
                let stop = end.unwrap_or(i + run);
                out.extend(&chars[i..stop]);
                i = stop;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// An agent's or the user's Markdown as it may enter the report: a fenced code block still open
/// at its end is closed, so it cannot swallow what follows, and raw HTML outside code shows as
/// text ([`escape_html`]). Code blocks and code spans stay verbatim.
pub fn prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut open: Option<(char, usize)> = None;
    let mut indented = false;
    let mut previous_blank = true;
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let blank = line.trim().is_empty();
        match (open, fence(line)) {
            (Some((mark, count)), Some(f)) if f.mark == mark && f.count >= count && f.closes => {
                open = None;
                out.push_str(line);
            }
            (Some(_), _) => out.push_str(line),
            (None, Some(f)) if f.opens => {
                open = Some((f.mark, f.count));
                indented = false;
                out.push_str(line);
            }
            (None, _) => {
                // An indented code block starts after a blank line and goes on while indented;
                // a blank line inside it does not end it (the next indented line follows one).
                indented = !blank && is_indented_code(line) && (previous_blank || indented);
                if indented || blank {
                    out.push_str(line);
                } else {
                    out.push_str(&escape_html(line));
                }
            }
        }
        previous_blank = blank;
    }
    if let Some((mark, count)) = open {
        out.push('\n');
        out.push_str(&mark.to_string().repeat(count));
    }
    out
}

/// A list item, so consecutive tool calls form one compact list.
fn is_list_item(item: &Item) -> bool {
    matches!(item, Item::Tool { .. } | Item::SubAgent { .. })
}

fn quote(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn item(item: &Item, labels: &Labels) -> String {
    match item {
        Item::Prompt(text) => quote(&prose(text)),
        Item::Command(command) => format!("> {}", code(command)),
        Item::Text(text) => prose(text),
        Item::Tool { name, detail } => match detail {
            Some(detail) => format!("- **{}** {}", inline(name), code(detail)),
            None => format!("- **{}**", inline(name)),
        },
        Item::SubAgent {
            description,
            agent_type,
            tokens,
            ..
        } => {
            let mut line = format!("- **{}**", inline(&labels.sub_agent));
            if let Some(kind) = agent_type {
                line.push_str(&format!(" ({})", inline(kind)));
            }
            if let Some(description) = description {
                line.push_str(&format!(": {}", inline(description)));
            }
            if let Some(tokens) = tokens {
                let count = labels.tokens.replace("{count}", &tokens.to_string());
                line.push_str(&format!(", {}", inline(&count)));
            }
            line
        }
        Item::Compacted => format!("---\n\n_{}_", inline(&labels.compacted)),
        Item::Gap => format!("_{}_", inline(&labels.earlier_left_out)),
    }
}

/// The whole document. `offset_minutes`: the window's offset from UTC, for the times shown.
pub fn render(report: &Report, labels: &Labels, offset_minutes: i32) -> String {
    let mut out = format!("# {}\n\n", inline(&report.title));
    let mut fact = |label: &str, value: String| {
        out.push_str(&format!("- **{}:** {value}\n", inline(label)));
    };
    fact(&labels.program, inline(&report.program));
    fact(&labels.folder, code(&report.folder));
    if let Some(branch) = &report.conversation.branch {
        fact(&labels.branch, code(branch));
    }
    let (started, last) = (
        report.conversation.started_at,
        report.conversation.last_activity,
    );
    if let Some(at) = started {
        fact(&labels.started, local_time(at, offset_minutes));
    }
    if let Some(at) = last {
        fact(&labels.last_activity, local_time(at, offset_minutes));
    }
    if let (Some(start), Some(end)) = (started, last) {
        fact(&labels.duration, duration(end - start));
    }

    out.push_str(&format!("\n## {}\n", inline(&labels.conversation)));
    let mut previous: Option<&Item> = None;
    for entry in &report.conversation.items {
        // List items follow each other directly; everything else is its own block.
        let joined = previous.is_some_and(is_list_item) && is_list_item(entry);
        out.push_str(if joined { "\n" } else { "\n\n" });
        out.push_str(&item(entry, labels));
        previous = Some(entry);
    }
    out.push('\n');

    if let Some(git) = &report.git {
        out.push_str(&format!("\n## {}\n\n", inline(&labels.changed_files)));
        if git.changed.is_empty() {
            out.push_str(&format!("_{}_\n", inline(&labels.none)));
        }
        for file in &git.changed {
            out.push_str(&format!(
                "- {} {}\n",
                code(file.status.trim()),
                code(&file.path)
            ));
        }
        if git.changed_cut {
            out.push_str("- \u{2026}\n");
        }
        out.push_str(&format!("\n## {}\n\n", inline(&labels.commits)));
        if git.commits.is_empty() {
            out.push_str(&format!("_{}_\n", inline(&labels.none)));
        }
        for commit in &git.commits {
            out.push_str(&format!(
                "- {} {}\n",
                code(&commit.short_sha),
                inline(&commit.subject)
            ));
        }
        if git.more_commits {
            out.push_str("- \u{2026}\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_durations_read_well() {
        // 2026-11-14 10:05:00 UTC.
        let at = 1_794_650_700_000;
        assert_eq!(local_time(at, 0), "2026-11-14 10:05 UTC+00:00");
        assert_eq!(local_time(at, 60), "2026-11-14 11:05 UTC+01:00");
        assert_eq!(local_time(at, -330), "2026-11-14 04:35 UTC-05:30");
        assert_eq!(local_time(at, 14 * 60), "2026-11-15 00:05 UTC+14:00");
        assert_eq!(duration(-5), "0 s");
        assert_eq!(duration(45_900), "45 s");
        assert_eq!(duration(12 * 60_000 + 59_000), "12 min");
        assert_eq!(duration(2 * 3_600_000 + 5 * 60_000), "2 h 05 min");
    }

    #[test]
    fn text_cannot_become_structure() {
        assert_eq!(
            inline("# a\n*b* [c](d) <e>"),
            "\\# a \\*b\\* \\[c\\](d) \\<e\\>"
        );
        assert_eq!(code("cargo test"), "`cargo test`");
        assert_eq!(code("a `b` c"), "``a `b` c``");
        assert_eq!(code("`x`"), "`` `x` ``");
        assert_eq!(code("a\nb"), "`a b`");
        assert_eq!(prose("```rust\nfn x() {}"), "```rust\nfn x() {}\n```");
        assert_eq!(prose("```\na\n```\nb"), "```\na\n```\nb");
        assert_eq!(prose("~~~~\n```\n~~~"), "~~~~\n```\n~~~\n~~~~");
        // A backtick in the info string: no fence opens, so none is added.
        assert_eq!(prose("```a`b"), "```a`b");
        assert_eq!(prose("plain"), "plain");
        assert_eq!(quote("a\n\nb"), "> a\n>\n> b");
    }

    #[test]
    fn raw_html_shows_as_text_outside_code() {
        // Unclosed blocks that would hide everything after them in a viewer that renders HTML.
        assert_eq!(prose("<!-- note"), "&lt;!-- note");
        assert_eq!(
            prose("Wrap it in <div> or <img src=x>"),
            "Wrap it in &lt;div> or &lt;img src=x>"
        );
        // Code spans, fenced and indented code stay verbatim.
        assert_eq!(
            prose("Use `Vec<u8>` and ``a ` <b>``"),
            "Use `Vec<u8>` and ``a ` <b>``"
        );
        assert_eq!(prose("```html\n<p>x</p>\n```"), "```html\n<p>x</p>\n```");
        assert_eq!(
            prose("Code:\n\n    <p>x</p>\n\n    <q>\nafter <b>"),
            "Code:\n\n    <p>x</p>\n\n    <q>\nafter &lt;b>"
        );
        // An unclosed backtick run is literal; backslash escapes are kept.
        assert_eq!(prose("a ` <b>"), "a ` &lt;b>");
        assert_eq!(prose("\\` <b> \\<i>"), "\\` &lt;b> \\\\&lt;i>");
        // Inside a quoted prompt as well.
        assert_eq!(quote(&prose("<script>")), "> &lt;script>");
    }

    #[test]
    fn images_and_inline_links_show_as_text_outside_code() {
        assert_eq!(
            prose("See ![chart](https://example.invalid/pixel.png) here"),
            "See !\\[chart]\\(https://example.invalid/pixel.png) here"
        );
        assert_eq!(
            prose("![ref image][r]\n\n[r]: https://example.invalid/x.png"),
            "!\\[ref image][r]\n\n[r]: https://example.invalid/x.png"
        );
        assert_eq!(
            prose("[docs](https://example.invalid)"),
            "[docs]\\(https://example.invalid)"
        );
        // Code keeps it verbatim; an escaped `!` stays escaped.
        assert_eq!(prose("`![a](b)`"), "`![a](b)`");
        assert_eq!(prose("```\n![a](b)\n```"), "```\n![a](b)\n```");
        assert_eq!(prose("\\![a]"), "\\![a]");
        assert_eq!(prose("Wow! [a] (b)"), "Wow! [a] (b)");
        assert_eq!(quote(&prose("![x](y)")), "> !\\[x]\\(y)");
    }
}
