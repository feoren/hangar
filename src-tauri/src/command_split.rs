//! Splitting a shell command line into program calls for the command guard (ADR 0010).
//!
//! Three lexers, one per dialect the agents' shell tools speak: POSIX shells (Claude Code's
//! `Bash`, Git Bash on Windows), PowerShell and cmd.exe. Wrappers in front of a program are
//! skipped and inline scripts (`sh -c`, `pwsh -Command`, `cmd /c`, ...) are split again in their
//! own dialect. Pure: nothing is run or touched.
//!
//! What cannot be followed is reported instead of passed over: a script nested deeper than
//! [`MAX_DEPTH`], more than [`MAX_CALLS`] calls, a quote or substitution left open, a shell that
//! reads its script from stdin.

/// How deep inline scripts and substitutions are followed.
pub const MAX_DEPTH: usize = 6;
/// Program calls looked at in one command.
pub const MAX_CALLS: usize = 512;
/// Longest decoded `-EncodedCommand` script, in bytes of base64 text.
const MAX_ENCODED: usize = 256 * 1024;
/// Start folders of one `find` that its `-exec` command is read for.
const MAX_FIND_STARTS: usize = 8;

/// The shell language a script is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    #[default]
    Posix,
    PowerShell,
    Cmd,
}

/// What a command line runs, as far as the lexers can tell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    /// Each program call's words, wrappers removed, program first, in order.
    pub calls: Vec<Vec<String>>,
    /// Where each call of [`Analysis::calls`] stands and what it reads, at the same index.
    pub info: Vec<CallInfo>,
    /// Something was not followed: nested too deep or too many calls.
    pub truncated: bool,
    /// A quote, substitution or here-document was left open: the shell would read more than the
    /// lexer could place.
    pub unclosed: bool,
    /// A shell reads its script from stdin (a pipe, a here-string, `pwsh -Command -`): what it
    /// runs is not on the command line.
    pub stdin_script: bool,
    /// Redirection targets (`> file`, `2>> log`), each with the number of calls found before
    /// its command, so a `cd` before it is known.
    pub redirects: Vec<(usize, String)>,
}

/// How one program call was written and what it reads.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallInfo {
    /// The dialect of the script the call stands in (an inline script's own).
    pub dialect: Dialect,
    /// Its stdin is the output of the command before it (`a | b`).
    pub piped: bool,
    /// The call whose output it reads, when that is one call of [`Analysis::calls`].
    pub upstream: Option<usize>,
    /// Here-strings and here-document bodies it reads on stdin, as the shell passes them.
    pub fed: Vec<String>,
}

/// A word whose value Hangar cannot know (a substitution); it reads as a variable, so a path
/// built from it counts as unplaceable.
pub const UNKNOWN: &str = "$?";
/// The file a process substitution (`<(cmd)`) names: a pipe of the running shell.
pub const PROCESS_FILE: &str = "/dev/fd/$?";

/// Every program call of `command` read as `dialect`.
pub fn split(command: &str, dialect: Dialect) -> Analysis {
    let mut splitter = Splitter::default();
    splitter.script(command, dialect, 0);
    splitter.analysis
}

/// One simple command as a lexer read it.
struct Simple {
    words: Vec<String>,
    redirects: Vec<String>,
    /// Its stdin is the output of the command before it.
    piped: bool,
    /// Here-strings and here-document bodies it reads.
    fed: Vec<String>,
}

/// One part of a script in order: a simple command, or a nested script to split.
enum Piece {
    Command(Simple),
    Script(String, Dialect),
}

/// A POSIX here-document whose body is still to come.
struct Heredoc {
    /// The delimiter line, quotes removed.
    delimiter: String,
    /// Any part of the delimiter was quoted: the body is plain data.
    quoted: bool,
    /// `<<-`: leading tabs are removed from the body and the delimiter line.
    strip_tabs: bool,
    /// The piece of the command that reads it.
    owner: Option<usize>,
}

/// The words of the simple command being lexed.
#[derive(Default)]
struct Words {
    pieces: Vec<Piece>,
    words: Vec<String>,
    word: String,
    in_word: bool,
    /// The next word is a redirection's target, not an argument.
    target: bool,
    /// The next word is a here-string's text (`<<<`).
    here_string: bool,
    redirects: Vec<String>,
    fed: Vec<String>,
    /// The current command reads the previous one's output.
    piped: bool,
    /// Here-documents of the current command.
    open_docs: Vec<Heredoc>,
    /// Here-documents of finished commands; their bodies start after the next line break.
    ready_docs: Vec<Heredoc>,
    unclosed: bool,
}

impl Words {
    fn start(&mut self) {
        self.in_word = true;
    }

    fn push(&mut self, c: char) {
        self.in_word = true;
        self.word.push(c);
    }

    fn end_word(&mut self) {
        if !self.in_word {
            return;
        }
        let word = std::mem::take(&mut self.word);
        if self.target {
            self.target = false;
            self.redirects.push(word);
        } else if self.here_string {
            self.here_string = false;
            self.fed.push(word);
        } else {
            self.words.push(word);
        }
        self.in_word = false;
    }

    fn end_command(&mut self) {
        self.end_word();
        self.target = false;
        self.here_string = false;
        let owner = if !self.words.is_empty() || !self.redirects.is_empty() || !self.fed.is_empty()
        {
            self.pieces.push(Piece::Command(Simple {
                words: std::mem::take(&mut self.words),
                redirects: std::mem::take(&mut self.redirects),
                piped: self.piped,
                fed: std::mem::take(&mut self.fed),
            }));
            Some(self.pieces.len() - 1)
        } else {
            None
        };
        self.piped = false;
        for mut doc in std::mem::take(&mut self.open_docs) {
            doc.owner = owner;
            self.ready_docs.push(doc);
        }
    }

    /// A pipe: the command after it reads this one's output.
    fn pipe(&mut self) {
        self.end_command();
        self.piped = true;
    }

    /// Text a finished command reads on stdin.
    fn feed(&mut self, owner: Option<usize>, text: String) {
        if let Some(Piece::Command(simple)) = owner.and_then(|at| self.pieces.get_mut(at)) {
            simple.fed.push(text);
        }
    }

    /// A redirection operator: a file descriptor number in front (`2>`, PowerShell's `*>`) is
    /// no argument, and the next word is the target.
    fn redirect(&mut self) {
        if self.in_word && (self.word.chars().all(|d| d.is_ascii_digit()) || self.word == "*") {
            self.word.clear();
            self.in_word = false;
        } else {
            self.end_word();
        }
        self.target = true;
    }

    fn substitution(&mut self, inner: String, dialect: Dialect, value: &str) {
        self.pieces.push(Piece::Script(inner, dialect));
        self.push_str(value);
    }

    fn push_str(&mut self, text: &str) {
        self.in_word = true;
        self.word.push_str(text);
    }
}

/// Where the `)` closing a `(` before `start` is, aware of quotes and, in POSIX shells, of
/// here-document bodies (a quote or `)` in one is text, as in `$(cat <<'EOF' ... EOF)`); `None`
/// when it never closes.
fn closing_paren(chars: &[char], start: usize, dialect: Dialect) -> Option<usize> {
    let posix = dialect == Dialect::Posix;
    // `$((...))` is arithmetic: `<<` there shifts.
    let arithmetic = chars.get(start) == Some(&'(');
    // Here-documents opened on the current line: delimiter and whether tabs are stripped.
    let mut docs: Vec<(String, bool)> = Vec::new();
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            // A here-string (`<<<`) is no here-document.
            '<' if posix && chars.get(i + 1) == Some(&'<') && chars.get(i + 2) == Some(&'<') => {
                i += 2;
            }
            '<' if posix && !arithmetic && chars.get(i + 1) == Some(&'<') => {
                let mut j = i + 2;
                let strip_tabs = chars.get(j) == Some(&'-');
                if strip_tabs {
                    j += 1;
                }
                while matches!(chars.get(j), Some(' ' | '\t')) {
                    j += 1;
                }
                let (delimiter, _, end) = heredoc_word(chars, j);
                if !delimiter.is_empty() {
                    docs.push((delimiter, strip_tabs));
                }
                i = end;
                continue;
            }
            '\n' if !docs.is_empty() => {
                i = after_heredoc_bodies(chars, i + 1, &std::mem::take(&mut docs));
                continue;
            }
            '\\' if dialect == Dialect::Posix => i += 1,
            '`' if dialect == Dialect::PowerShell => i += 1,
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if (dialect == Dialect::Posix && chars[i] == '\\')
                        || (dialect == Dialect::PowerShell && chars[i] == '`')
                    {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Where the line after the bodies of `docs` begins, the first body starting at `start`, read as
/// [`heredoc_bodies`] reads them. `start` when a delimiter line never comes: the text is then
/// scanned as commands, and the lexer reports the open document as unclosed.
fn after_heredoc_bodies(chars: &[char], start: usize, docs: &[(String, bool)]) -> usize {
    let mut i = start;
    for (delimiter, strip_tabs) in docs {
        let mut closed = false;
        while i < chars.len() {
            let end = chars[i..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(chars.len(), |n| i + n);
            let mut line = &chars[i..end];
            i = (end + 1).min(chars.len());
            if *strip_tabs {
                while let Some(rest) = line.strip_prefix(&['\t']) {
                    line = rest;
                }
            }
            let bare = line.strip_suffix(&['\r']).unwrap_or(line);
            if bare.iter().copied().eq(delimiter.chars()) {
                closed = true;
                break;
            }
        }
        if !closed {
            return start;
        }
    }
    i
}

/// Where the backtick closing one before `start` is; `None` when it never closes.
fn closing_backtick(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '`' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn text_of(chars: &[char], from: usize, to: usize) -> String {
    chars.get(from..to).unwrap_or_default().iter().collect()
}

/// POSIX shell: quotes, every backslash outside quotes escapes, `$'...'`, `$(...)`, backticks,
/// here-documents and here-strings, process substitutions.
fn lex_posix(text: &str) -> Words {
    let chars: Vec<char> = text.chars().collect();
    let mut w = Words::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\r' => w.end_word(),
            '\n' => {
                w.end_command();
                if !w.ready_docs.is_empty() {
                    i = heredoc_bodies(&mut w, &chars, i + 1);
                    continue;
                }
            }
            ';' => w.end_command(),
            '&' if next == Some('>') => {
                w.end_word();
                i += 1;
                if chars.get(i + 1) == Some(&'>') {
                    i += 1;
                }
                w.target = true;
            }
            '&' => {
                w.end_command();
                if next == Some('&') {
                    i += 1;
                }
            }
            '|' if next == Some('|') => {
                w.end_command();
                i += 1;
            }
            '|' => {
                w.pipe();
                if next == Some('&') {
                    i += 1;
                }
            }
            '(' | ')' => w.end_command(),
            // `{` and `}` group commands only as words of their own; inside a word they are
            // text or a brace expansion (`{git,push}`, `{}`).
            '{' if !w.in_word && matches!(next, None | Some(' ' | '\t' | '\n' | '\r')) => {
                w.end_command()
            }
            '}' if !w.in_word
                && matches!(
                    next,
                    None | Some(' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | ')')
                ) =>
            {
                w.end_command()
            }
            '<' if next == Some('<') && chars.get(i + 2) != Some(&'<') => {
                // A here-document: its body follows the next line break.
                w.redirect();
                w.target = false;
                i += 2;
                let strip_tabs = chars.get(i) == Some(&'-');
                if strip_tabs {
                    i += 1;
                }
                while matches!(chars.get(i), Some(' ' | '\t')) {
                    i += 1;
                }
                let (delimiter, quoted, end) = heredoc_word(&chars, i);
                if delimiter.is_empty() && !quoted {
                    w.unclosed = true;
                }
                w.open_docs.push(Heredoc {
                    delimiter,
                    quoted,
                    strip_tabs,
                    owner: None,
                });
                i = end;
                continue;
            }
            '<' if next == Some('<') => {
                // A here-string: the next word is what the command reads.
                w.redirect();
                w.target = false;
                w.here_string = true;
                i += 2;
            }
            '<' | '>' if next == Some('(') => {
                i = posix_substitution_as(&mut w, &chars, i + 2, true, PROCESS_FILE);
            }
            '<' | '>' => {
                w.redirect();
                while matches!(chars.get(i + 1), Some('<' | '>' | '&' | '|')) {
                    i += 1;
                }
            }
            '#' if !w.in_word => {
                while i + 1 < chars.len() && chars[i + 1] != '\n' {
                    i += 1;
                }
            }
            '\'' => {
                w.start();
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    w.word.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    w.unclosed = true;
                }
            }
            '$' if next == Some('\'') => {
                w.start();
                i += 2;
                let (decoded, end) = ansi_c(&chars, i);
                w.word.push_str(&decoded);
                match end {
                    Some(end) => i = end,
                    None => {
                        w.unclosed = true;
                        i = chars.len();
                    }
                }
            }
            '"' => {
                w.start();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    match chars[i] {
                        '\\' if matches!(chars.get(i + 1), Some('"' | '\\' | '$' | '`')) => {
                            i += 1;
                            w.word.push(chars[i]);
                        }
                        '\\' if chars.get(i + 1) == Some(&'\n') => i += 1,
                        '$' if chars.get(i + 1) == Some(&'(') => {
                            i = posix_substitution(&mut w, &chars, i + 2, true);
                        }
                        '`' => i = posix_substitution(&mut w, &chars, i + 1, false),
                        other => w.word.push(other),
                    }
                    i += 1;
                }
                if i >= chars.len() {
                    w.unclosed = true;
                }
            }
            '$' if next == Some('(') => i = posix_substitution(&mut w, &chars, i + 2, true),
            // `$"..."` is a translated string: the `$` belongs to no word.
            '$' if next == Some('"') => w.start(),
            '`' => i = posix_substitution(&mut w, &chars, i + 1, false),
            '\\' => match next {
                Some('\n') => i += 1,
                Some(escaped) => {
                    w.push(escaped);
                    i += 1;
                }
                None => {}
            },
            other => w.push(other),
        }
        i += 1;
    }
    w.end_command();
    w
}

/// A `$(...)` (`paren`) or backtick substitution from `start`; returns the index of its closing
/// character, or the end of the text when it never closes.
fn posix_substitution(w: &mut Words, chars: &[char], start: usize, paren: bool) -> usize {
    posix_substitution_as(w, chars, start, paren, UNKNOWN)
}

/// [`posix_substitution`] whose value in the word is `value`.
fn posix_substitution_as(
    w: &mut Words,
    chars: &[char],
    start: usize,
    paren: bool,
    value: &str,
) -> usize {
    let end = if paren {
        closing_paren(chars, start, Dialect::Posix)
    } else {
        closing_backtick(chars, start)
    };
    let end = end.unwrap_or_else(|| {
        w.unclosed = true;
        chars.len()
    });
    w.substitution(text_of(chars, start, end), Dialect::Posix, value);
    end
}

/// The delimiter word of a here-document from `start`: its text without quotes, whether any part
/// was quoted, and the index after it.
fn heredoc_word(chars: &[char], start: usize) -> (String, bool, usize) {
    let mut out = String::new();
    let mut quoted = false;
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        match c {
            ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '<' | '>' | '(' | ')' => break,
            '\'' => {
                quoted = true;
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '"' => {
                quoted = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && matches!(chars.get(i + 1), Some('"' | '\\' | '$' | '`'))
                    {
                        i += 1;
                    }
                    out.push(chars[i]);
                    i += 1;
                }
            }
            '\\' => {
                quoted = true;
                i += 1;
                if let Some(&escaped) = chars.get(i) {
                    out.push(escaped);
                }
            }
            '$' if chars.get(i + 1) == Some(&'\'') => {
                quoted = true;
                let (decoded, end) = ansi_c(chars, i + 2);
                out.push_str(&decoded);
                i = end.unwrap_or(chars.len());
            }
            other => out.push(other),
        }
        i += 1;
    }
    (out, quoted, i.min(chars.len()))
}

/// Reads the bodies of the here-documents waiting for the line break before `start`, one after
/// the other; returns where the next line of commands starts. A quoted document is data; in an
/// unquoted one only `$(...)` and backticks run. A body whose delimiter line never comes leaves
/// the rest of the text unread, which counts as unclosed.
fn heredoc_bodies(w: &mut Words, chars: &[char], start: usize) -> usize {
    let mut i = start;
    for doc in std::mem::take(&mut w.ready_docs) {
        let mut body = String::new();
        let mut closed = false;
        while i < chars.len() {
            let end = chars[i..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(chars.len(), |n| i + n);
            let mut line = &chars[i..end];
            i = (end + 1).min(chars.len());
            if doc.strip_tabs {
                while let Some(rest) = line.strip_prefix(&['\t']) {
                    line = rest;
                }
            }
            let bare = line.strip_suffix(&['\r']).unwrap_or(line);
            if bare.iter().copied().eq(doc.delimiter.chars()) {
                closed = true;
                break;
            }
            body.extend(line);
            body.push('\n');
        }
        if !closed {
            w.unclosed = true;
        }
        let text = if doc.quoted {
            body
        } else {
            heredoc_substitutions(w, &body)
        };
        w.feed(doc.owner, text);
    }
    i
}

/// An unquoted here-document's body with its substitutions split as scripts of their own and
/// read as [`UNKNOWN`]; `\$`, `` \` `` and `\\` stay plain characters.
fn heredoc_substitutions(w: &mut Words, body: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if matches!(chars.get(i + 1), Some('$' | '`' | '\\')) => {
                out.push(chars[i + 1]);
                i += 1;
            }
            '\\' if chars.get(i + 1) == Some(&'\n') => i += 1,
            '$' if chars.get(i + 1) == Some(&'(') => {
                let end = closing_paren(&chars, i + 2, Dialect::Posix).unwrap_or_else(|| {
                    w.unclosed = true;
                    chars.len()
                });
                w.pieces
                    .push(Piece::Script(text_of(&chars, i + 2, end), Dialect::Posix));
                out.push_str(UNKNOWN);
                i = end;
            }
            '`' => {
                let end = closing_backtick(&chars, i + 1).unwrap_or_else(|| {
                    w.unclosed = true;
                    chars.len()
                });
                w.pieces
                    .push(Piece::Script(text_of(&chars, i + 1, end), Dialect::Posix));
                out.push_str(UNKNOWN);
                i = end;
            }
            other => out.push(other),
        }
        i += 1;
    }
    out
}

/// The text of a `$'...'` string from `start` (after the quote) and the index of its closing
/// quote.
fn ansi_c(chars: &[char], start: usize) -> (String, Option<usize>) {
    let mut out = String::new();
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\'' => return (out, Some(i)),
            '\\' if i + 1 < chars.len() => {
                i += 1;
                let code = |digits: &[char], radix: u32| {
                    let text: String = digits.iter().collect();
                    u32::from_str_radix(&text, radix)
                        .ok()
                        .and_then(char::from_u32)
                };
                match chars[i] {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'a' => out.push('\u{7}'),
                    'b' => out.push('\u{8}'),
                    'e' | 'E' => out.push('\u{1b}'),
                    'f' => out.push('\u{c}'),
                    'v' => out.push('\u{b}'),
                    'x' | 'u' | 'U' => {
                        let max = match chars[i] {
                            'x' => 2,
                            'u' => 4,
                            _ => 8,
                        };
                        let digits: Vec<char> = chars[i + 1..]
                            .iter()
                            .take(max)
                            .take_while(|c| c.is_ascii_hexdigit())
                            .copied()
                            .collect();
                        if let Some(c) = code(&digits, 16) {
                            out.push(c);
                        }
                        i += digits.len();
                    }
                    '0'..='7' => {
                        let digits: Vec<char> = chars[i..]
                            .iter()
                            .take(3)
                            .take_while(|c| ('0'..='7').contains(*c))
                            .copied()
                            .collect();
                        if let Some(c) = code(&digits, 8) {
                            out.push(c);
                        }
                        i += digits.len() - 1;
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    (out, None)
}

/// PowerShell's quotes: the typographic ones count like the ASCII ones.
fn ps_single(c: char) -> bool {
    matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}')
}

fn ps_double(c: char) -> bool {
    matches!(c, '"' | '\u{201c}' | '\u{201d}' | '\u{201e}')
}

/// PowerShell reads an en dash, em dash or horizontal bar in front of a parameter as `-`.
fn ps_dash(c: char) -> bool {
    matches!(c, '\u{2013}' | '\u{2014}' | '\u{2015}')
}

/// PowerShell: backtick escapes, a backslash is a plain character, doubled quotes, `$(...)`.
fn lex_powershell(text: &str) -> Words {
    let chars: Vec<char> = text.chars().collect();
    let mut w = Words::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\r' => w.end_word(),
            '\n' | ';' => w.end_command(),
            '&' => {
                w.end_command();
                if next == Some('&') {
                    i += 1;
                }
            }
            '|' if next == Some('|') => {
                w.end_command();
                i += 1;
            }
            '|' => w.pipe(),
            // `[scriptblock]::Create('...')`, `.InvokeScript('...')`: a literal argument is a
            // script, read like `Invoke-Expression`'s.
            '(' if w.in_word && creates_script(&w.word) => {
                i = match ps_literal_in_parens(&chars, i + 1) {
                    Some((text, end)) => {
                        w.substitution(text, Dialect::PowerShell, UNKNOWN);
                        end
                    }
                    None => ps_substitution(&mut w, &chars, i + 1),
                }
            }
            // `Remove-Item -Recurse (Join-Path $HOME x)`: an argument in parentheses is a value
            // Hangar cannot know, and what it runs is split on its own.
            '(' if !w.words.is_empty() || w.in_word => {
                i = match ps_literal_in_parens(&chars, i + 1) {
                    Some((text, end)) => {
                        w.push_str(&text);
                        end
                    }
                    None => ps_substitution(&mut w, &chars, i + 1),
                }
            }
            // A here-string: `@'` or `@"` at the end of a line, up to a line starting with the
            // same quote and `@`.
            '@' if next.is_some_and(|q| ps_single(q) || ps_double(q)) => {
                match here_string_body(&chars, i + 2) {
                    Some(body) => {
                        i = ps_here_string(&mut w, &chars, body, next.is_some_and(ps_double));
                        continue;
                    }
                    None => w.push('@'),
                }
            }
            '(' | ')' => w.end_command(),
            '{' | '}' if !w.in_word => w.end_command(),
            '<' if next == Some('#') => {
                // Block comment `<# ... #>`.
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '#' && chars[i + 1] == '>') {
                    i += 1;
                }
                if i + 1 >= chars.len() {
                    w.unclosed = true;
                }
                i += 1;
            }
            '<' | '>' => {
                w.redirect();
                while matches!(chars.get(i + 1), Some('>' | '&')) {
                    i += 1;
                }
                // `2>&1`: the target is a stream number.
            }
            '#' if !w.in_word => {
                while i + 1 < chars.len() && chars[i + 1] != '\n' {
                    i += 1;
                }
            }
            '-' if !w.in_word && next == Some('-') && chars.get(i + 2) == Some(&'%') => {
                // `--%`: the rest of the line goes to the program as it is.
                w.end_word();
                i += 3;
                let start = i;
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                for word in text_of(&chars, start, i).split_whitespace() {
                    w.words.push(word.to_owned());
                }
                continue;
            }
            c if ps_dash(c) && !w.in_word => w.push('-'),
            c if ps_single(c) => {
                w.start();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => {
                            w.unclosed = true;
                            break;
                        }
                        Some(&q) if ps_single(q) => {
                            if chars.get(i + 1).is_some_and(|n| ps_single(*n)) {
                                w.word.push('\'');
                                i += 2;
                            } else {
                                break;
                            }
                        }
                        Some(&other) => {
                            w.word.push(other);
                            i += 1;
                        }
                    }
                }
            }
            c if ps_double(c) => {
                w.start();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => {
                            w.unclosed = true;
                            break;
                        }
                        Some(&q) if ps_double(q) => {
                            if chars.get(i + 1).is_some_and(|n| ps_double(*n)) {
                                w.word.push('"');
                                i += 2;
                            } else {
                                break;
                            }
                        }
                        Some('`') => {
                            if let Some(&escaped) = chars.get(i + 1) {
                                w.word.push(escaped);
                            }
                            i += 2;
                        }
                        Some('$') if chars.get(i + 1) == Some(&'(') => {
                            i = ps_substitution(&mut w, &chars, i + 2) + 1;
                        }
                        Some(&other) => {
                            w.word.push(other);
                            i += 1;
                        }
                    }
                }
            }
            '$' if next == Some('(') => i = ps_substitution(&mut w, &chars, i + 2),
            '`' => {
                match next {
                    // A line continuation.
                    Some('\n') => {}
                    Some(escaped) => w.push(escaped),
                    None => {}
                }
                i += 1;
            }
            other => w.push(other),
        }
        i += 1;
    }
    w.end_command();
    w
}

/// `("..\build")`: a parenthesized argument that is one plain string literal, with nothing
/// PowerShell would expand in it. Its text and the index of the closing parenthesis.
fn ps_literal_in_parens(chars: &[char], start: usize) -> Option<(String, usize)> {
    let end = closing_paren(chars, start, Dialect::PowerShell)?;
    let inner: String = text_of(chars, start, end);
    let inner = inner.trim();
    let mut quotes = inner.chars();
    let open = quotes.next()?;
    let body = inner.get(open.len_utf8()..inner.len().checked_sub(1)?)?;
    let closes = inner.ends_with(open) && inner.chars().count() >= 2;
    let plain = match open {
        '\'' => !body.contains('\''),
        '"' => !body.contains(['"', '$', '`']),
        _ => false,
    };
    (closes && plain).then(|| (body.to_owned(), end))
}

fn ps_substitution(w: &mut Words, chars: &[char], start: usize) -> usize {
    let end = closing_paren(chars, start, Dialect::PowerShell).unwrap_or_else(|| {
        w.unclosed = true;
        chars.len()
    });
    w.substitution(text_of(chars, start, end), Dialect::PowerShell, UNKNOWN);
    end
}

/// Whether a PowerShell word ends in a method that turns its string argument into a script.
fn creates_script(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    ["scriptblock]::create", ".invokescript", ".newscriptblock"]
        .iter()
        .any(|method| lower.ends_with(method))
}

/// Where a here-string's text starts: only blanks may follow `@'` or `@"` (from `start`) on its
/// line. `None` when it is no here-string.
fn here_string_body(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while matches!(chars.get(i), Some(' ' | '\t')) {
        i += 1;
    }
    if chars.get(i) == Some(&'\r') {
        i += 1;
    }
    (chars.get(i) == Some(&'\n')).then_some(i + 1)
}

/// The text of a here-string from `start` up to the line that begins with its closing quote and
/// `@`; returns the index after that `@`. A double quoted one keeps its backtick escapes and
/// runs its `$(...)`; a single quoted one is literal.
fn ps_here_string(w: &mut Words, chars: &[char], start: usize, double: bool) -> usize {
    w.start();
    let closes = |c: char| if double { ps_double(c) } else { ps_single(c) };
    let mut i = start;
    let mut line_start = true;
    while let Some(&c) = chars.get(i) {
        if line_start && closes(c) && chars.get(i + 1) == Some(&'@') {
            // The line break before the closing line is not part of the text.
            if w.word.ends_with('\n') {
                w.word.pop();
                if w.word.ends_with('\r') {
                    w.word.pop();
                }
            }
            return i + 2;
        }
        line_start = c == '\n';
        match c {
            '`' if double => {
                if let Some(&escaped) = chars.get(i + 1) {
                    w.word.push(escaped);
                    line_start = escaped == '\n';
                }
                i += 2;
            }
            '$' if double && chars.get(i + 1) == Some(&'(') => {
                i = ps_substitution(w, chars, i + 2) + 1;
            }
            other => {
                w.word.push(other);
                i += 1;
            }
        }
    }
    w.unclosed = true;
    chars.len()
}

/// cmd.exe: `^` escapes the next character, double quotes group, `,` and `;` separate words, a
/// backslash is a plain character.
fn lex_cmd(text: &str) -> Words {
    let chars: Vec<char> = text.chars().collect();
    let mut w = Words::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\r' | ',' | ';' | '=' if !w.in_word || c != '=' => w.end_word(),
            '\n' => w.end_command(),
            '&' => {
                w.end_command();
                if next == Some('&') {
                    i += 1;
                }
            }
            '|' if next == Some('|') => {
                w.end_command();
                i += 1;
            }
            '|' => w.pipe(),
            '(' | ')' if !w.in_word => w.end_command(),
            '<' | '>' => {
                w.redirect();
                while matches!(chars.get(i + 1), Some('>' | '&')) {
                    i += 1;
                }
            }
            '^' => {
                match next {
                    Some('\n') | None => {}
                    Some(escaped) => w.push(escaped),
                }
                i += 1;
            }
            '"' => {
                w.start();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    w.word.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    w.unclosed = true;
                }
            }
            '@' if !w.in_word && w.words.is_empty() => {}
            // cmd expands `%name:~0,1%` and `%name:a=b%` before it splits the line, so the
            // separators inside belong to the expansion.
            '%' => match cmd_edit_end(&chars, i) {
                Some(end) => {
                    w.push_str(&text_of(&chars, i, end + 1));
                    i = end;
                }
                None => w.push('%'),
            },
            other => w.push(other),
        }
        i += 1;
    }
    w.end_command();
    w
}

/// The index of the `%` closing a cmd substring or replace expansion (`%name:...%`) that starts
/// at `start`; `None` when none starts there.
fn cmd_edit_end(chars: &[char], start: usize) -> Option<usize> {
    let close = chars[start + 1..]
        .iter()
        .take_while(|c| **c != '\n')
        .position(|c| *c == '%')
        .map(|at| at + start + 1)?;
    let inner: String = text_of(chars, start + 1, close);
    let (name, _) = inner.split_once(':')?;
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.()".contains(c));
    plain.then_some(close)
}

#[derive(Default)]
struct Splitter {
    analysis: Analysis,
}

impl Splitter {
    fn script(&mut self, text: &str, dialect: Dialect, depth: usize) {
        if depth > MAX_DEPTH {
            self.analysis.truncated = true;
            return;
        }
        let words = match dialect {
            Dialect::Posix => lex_posix(text),
            Dialect::PowerShell => lex_powershell(text),
            Dialect::Cmd => lex_cmd(text),
        };
        if words.unclosed {
            self.analysis.unclosed = true;
        }
        // The call of the last command at this level, which a pipe hands its output on from.
        let mut last: Option<usize> = None;
        for piece in words.pieces {
            match piece {
                Piece::Command(simple) => {
                    let at = self.analysis.calls.len();
                    self.analysis
                        .redirects
                        .extend(simple.redirects.into_iter().map(|target| (at, target)));
                    let meta = CallInfo {
                        dialect,
                        piped: simple.piped,
                        upstream: if simple.piped { last } else { None },
                        fed: simple.fed,
                    };
                    self.command(&simple.words, dialect, depth, &meta);
                    let now = self.analysis.calls.len();
                    last = (now > at).then(|| now - 1);
                }
                Piece::Script(inner, dialect) => self.script(&inner, dialect, depth + 1),
            }
        }
    }

    fn push(&mut self, words: &[String], meta: &CallInfo) {
        if self.analysis.calls.len() >= MAX_CALLS {
            self.analysis.truncated = true;
            return;
        }
        self.analysis.calls.push(words.to_vec());
        self.analysis.info.push(meta.clone());
    }

    /// One simple command: wrappers in front of the program are skipped, inline scripts are
    /// split in turn, and what is left is one program call.
    fn command(&mut self, words: &[String], dialect: Dialect, depth: usize, meta: &CallInfo) {
        if depth > MAX_DEPTH {
            self.analysis.truncated = true;
            return;
        }
        let mut rest: &[String] = words;
        loop {
            while rest.first().is_some_and(|word| is_assignment(word)) {
                rest = &rest[1..];
            }
            let Some(first) = rest.first() else {
                return;
            };
            let program = crate::command_guard::program_name(first);
            let args = &rest[1..];
            match program.as_str() {
                // A sourced file is a script, not a wrapper; from stdin or a process its text is
                // not on the command line. PowerShell's `.` runs what follows.
                "." | "source" if dialect == Dialect::Posix => {
                    if args.first().is_some_and(|file| is_stdin_path(file)) {
                        self.analysis.stdin_script = true;
                    }
                    self.push(rest, meta);
                    return;
                }
                "{" | "}" | "!" | "then" | "do" | "else" | "elif" | "if" | "while" | "until"
                | "time" | "command" | "builtin" | "exec" | "nohup" | "&" | "." | "call"
                | "noglob" | "busybox" => rest = args,
                "winpty" => rest = skip_options(args, &["--title"]),
                "setsid" => rest = skip_options(args, &[]),
                "stdbuf" => rest = skip_options(args, &["-i", "-o", "-e"]),
                "ionice" => rest = skip_options(args, &["-c", "-n", "-p", "-P", "-u"]),
                "trap" if dialect == Dialect::Posix => {
                    // `trap 'cmd' EXIT` runs its first argument as a script later.
                    if let Some(script) = skip_options(args, &[]).first().filter(|s| *s != "-") {
                        self.script(script, Dialect::Posix, depth + 1);
                    }
                    self.push(rest, meta);
                    return;
                }
                "find" => {
                    self.push(rest, meta);
                    let inline = CallInfo {
                        dialect,
                        ..CallInfo::default()
                    };
                    for call in find_commands(args) {
                        self.command(&call, dialect, depth + 1, &inline);
                    }
                    return;
                }
                "sudo" | "doas" => {
                    rest = skip_options(
                        args,
                        &["-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-U", "-T"],
                    )
                }
                "env" => {
                    if let Some(position) =
                        args.iter().position(|a| a == "-S" || a == "--split-string")
                    {
                        match args.get(position + 1) {
                            Some(script) => self.script(script, Dialect::Posix, depth + 1),
                            None => self.analysis.stdin_script = true,
                        }
                        return;
                    }
                    rest = skip_options(args, &["-u", "-C", "--unset", "--chdir"]);
                }
                "nice" => rest = skip_options(args, &["-n", "--adjustment"]),
                "timeout" => {
                    let after = skip_options(args, &["-s", "-k", "--signal", "--kill-after"]);
                    rest = after.get(1..).unwrap_or_default();
                }
                "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" | "mksh" | "fish" => {
                    match shell_script(args) {
                        ShellScript::Inline(script) => {
                            self.script(script, Dialect::Posix, depth + 1)
                        }
                        ShellScript::Stdin => {
                            self.analysis.stdin_script = true;
                            self.push(rest, meta);
                        }
                        ShellScript::File => self.push(rest, meta),
                    }
                    return;
                }
                "pwsh" | "powershell" => {
                    match powershell_script(args, program == "powershell") {
                        ShellScriptOwned::Inline(script) => {
                            self.script(&script, Dialect::PowerShell, depth + 1)
                        }
                        ShellScriptOwned::Undecodable => {
                            self.analysis.unclosed = true;
                            self.push(rest, meta);
                        }
                        ShellScriptOwned::Stdin => {
                            self.analysis.stdin_script = true;
                            self.push(rest, meta);
                        }
                        ShellScriptOwned::File => self.push(rest, meta),
                    }
                    return;
                }
                "cmd" => {
                    match args
                        .iter()
                        .position(|a| ["/c", "/k", "/r"].iter().any(|s| a.eq_ignore_ascii_case(s)))
                    {
                        Some(position) => self.script(
                            &script_of(&args[position + 1..], Dialect::Cmd),
                            Dialect::Cmd,
                            depth + 1,
                        ),
                        None => {
                            self.analysis.stdin_script = true;
                            self.push(rest, meta);
                        }
                    }
                    return;
                }
                "eval" => {
                    self.script(&script_of(args, dialect), dialect, depth + 1);
                    return;
                }
                "iex" | "invoke-expression" => {
                    let args = match args.first() {
                        Some(name) if is_ps_param(name, "command", 1) => &args[1..],
                        _ => args,
                    };
                    self.script(
                        &script_of(args, Dialect::PowerShell),
                        Dialect::PowerShell,
                        depth + 1,
                    );
                    return;
                }
                "wsl" => {
                    let after = skip_options(
                        args,
                        &[
                            "-d",
                            "--distribution",
                            "-u",
                            "--user",
                            "--cd",
                            "--shell-type",
                        ],
                    );
                    match after.first().map(String::as_str) {
                        // `-e` runs the program without a shell: its words are the call.
                        Some("-e" | "--exec") => rest = &after[1..],
                        // Otherwise the words go to the default shell as one command line.
                        Some("--") => {
                            self.script(
                                &script_of(&after[1..], Dialect::Posix),
                                Dialect::Posix,
                                depth + 1,
                            );
                            return;
                        }
                        Some(_) => {
                            self.script(
                                &script_of(after, Dialect::Posix),
                                Dialect::Posix,
                                depth + 1,
                            );
                            return;
                        }
                        None => {
                            self.analysis.stdin_script = true;
                            self.push(rest, meta);
                            return;
                        }
                    }
                }
                "start" | "start-process" | "saps" => {
                    let call = started_program(args);
                    if call.is_empty() {
                        self.push(rest, meta);
                    } else {
                        // One level deeper, so `start start start ...` ends at MAX_DEPTH.
                        self.command(&call, dialect, depth + 1, meta);
                    }
                    return;
                }
                _ => {
                    self.push(rest, meta);
                    return;
                }
            }
        }
    }
}

/// A file that is the shell's own stdin or a pipe of the running shell (`/dev/stdin`,
/// `/dev/fd/0`, `/proc/self/fd/0`, a process substitution).
fn is_stdin_path(path: &str) -> bool {
    path == "/dev/stdin"
        || path.starts_with("/dev/fd/")
        || (path.starts_with("/proc/") && path.contains("/fd/"))
}

/// The commands `find` runs for what it finds (`-exec`, `-execdir`, `-ok`, `-okdir` up to `;`
/// or `+`), with `{}` read as `<start>/*` for each of its start folders: what it finds lies
/// below them, and a wildcard right in the project counts as inside it.
fn find_commands(args: &[String]) -> Vec<Vec<String>> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-H" | "-L" | "-P" => i += 1,
            "-D" => i += 2,
            _ if arg.starts_with("-O") => i += 1,
            _ => break,
        }
    }
    let mut starts: Vec<&str> = Vec::new();
    while let Some(arg) = args.get(i) {
        if arg.starts_with('-') || arg == "(" || arg == "!" || arg == "," {
            break;
        }
        starts.push(arg);
        i += 1;
    }
    if starts.is_empty() {
        starts.push(".");
    }
    let mut out = Vec::new();
    while i < args.len() {
        if !matches!(args[i].as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            i += 1;
            continue;
        }
        let begin = i + 1;
        let mut end = begin;
        while end < args.len() && args[end] != ";" && args[end] != "+" {
            end += 1;
        }
        for start in starts.iter().take(MAX_FIND_STARTS) {
            out.push(
                args[begin..end]
                    .iter()
                    .map(|word| word.replace("{}", &format!("{start}/*")))
                    .collect(),
            );
        }
        i = end + 1;
    }
    out
}

/// `NAME=value` in front of a program.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The words after the options of a wrapper; `with_value` take the next word along. `--` ends
/// them.
fn skip_options<'a>(args: &'a [String], with_value: &[&str]) -> &'a [String] {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" {
            return &args[i + 1..];
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        i += if with_value.contains(&arg.as_str()) {
            2
        } else {
            1
        };
    }
    args.get(i..).unwrap_or_default()
}

enum ShellScript<'a> {
    Inline(&'a str),
    /// A script file: what it runs is not on the command line, but it is not stdin either.
    File,
    Stdin,
}

/// Options of `sh`/`bash` that take the next word as their value.
const SHELL_VALUES: &[&str] = &["-o", "+o", "-O", "+O", "--rcfile", "--init-file"];

/// What `sh` runs: `-c <script>` (also `-lc`, `--command`, and `-c` anywhere among the options),
/// a file, stdin (no script argument, `-s`, `-`), or nothing (`--version`, `--help`).
fn shell_script(args: &[String]) -> ShellScript<'_> {
    let mut wants = false;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if SHELL_VALUES.contains(&arg.as_str()) {
            i += 2;
            continue;
        }
        if arg == "--version" || arg == "--help" {
            return ShellScript::File;
        }
        if arg == "-" || arg == "-s" {
            return ShellScript::Stdin;
        }
        if arg == "--command" {
            wants = true;
        } else if let Some(group) = arg.strip_prefix(['-', '+']) {
            if !group.starts_with('-') && group.contains('c') && arg.starts_with('-') {
                wants = true;
            }
        } else {
            return if wants {
                ShellScript::Inline(arg)
            } else if is_stdin_path(arg) {
                ShellScript::Stdin
            } else {
                ShellScript::File
            };
        }
        i += 1;
    }
    if wants {
        // `bash -c` without a script: nothing runs from the command line.
        ShellScript::File
    } else {
        ShellScript::Stdin
    }
}

/// PowerShell parameters of `Start-Process` that are switches; every other one takes a value.
const START_PROCESS_SWITCHES: &[(&str, usize)] = &[
    ("wait", 2),
    ("passthru", 1),
    ("nonewwindow", 1),
    ("usenewenvironment", 1),
    ("loaduserprofile", 1),
];

/// Parameters of `Start-Process` that take a value, besides `-FilePath` and `-ArgumentList`.
const START_PROCESS_VALUES: &[(&str, usize)] = &[
    ("workingdirectory", 2),
    ("windowstyle", 2),
    ("verb", 2),
    ("credential", 2),
    ("redirectstandardinput", 22),
    ("redirectstandardoutput", 22),
    ("redirectstandarderror", 22),
    ("environment", 2),
];

/// The program and arguments `Start-Process` (`-FilePath`, `-ArgumentList` with commas, or the
/// first two positional values) or cmd's `start` (`/b` and other switches, an empty title)
/// starts; empty when there is none. After the program, a word that is none of
/// `Start-Process`'s own parameters is the program's argument (cmd's `start git push --force`).
fn started_program(args: &[String]) -> Vec<String> {
    let mut program = None;
    let mut list: Option<&String> = None;
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        let found = program.is_some() || !positional.is_empty();
        let own_value = START_PROCESS_VALUES
            .iter()
            .any(|(full, min)| is_ps_param(arg, full, *min));
        let own_switch = START_PROCESS_SWITCHES
            .iter()
            .any(|(full, min)| is_ps_param(arg, full, *min));
        if found
            && !arg.starts_with('/')
            && !own_value
            && !own_switch
            && !is_ps_param(arg, "argumentlist", 2)
            && !is_ps_param(arg, "filepath", 2)
        {
            positional.push(arg);
            i += 1;
        } else if is_ps_param(arg, "filepath", 2) && arg.starts_with('-') {
            program = args.get(i + 1);
            i += 2;
        } else if is_ps_param(arg, "argumentlist", 2) && arg.starts_with('-') {
            list = args.get(i + 1);
            i += 2;
        } else if arg.starts_with('-') && arg.len() > 1 {
            i += if own_switch { 1 } else { 2 };
        } else if !found && arg.starts_with('/') && arg.len() <= 12 && !arg[1..].contains('/') {
            // cmd's `start /d <folder>` takes a value; its other switches do not.
            i += if arg.eq_ignore_ascii_case("/d") { 2 } else { 1 };
        } else if arg.is_empty() {
            i += 1;
        } else {
            positional.push(arg);
            i += 1;
        }
    }
    let mut positional = positional.into_iter();
    let Some(program) = program.or_else(|| positional.next()) else {
        return Vec::new();
    };
    let mut call = vec![program.clone()];
    let extra: Vec<&String> = match list {
        Some(list) => vec![list],
        None => positional.collect(),
    };
    for value in extra {
        for part in value.split(',') {
            call.extend(part.split_whitespace().map(str::to_owned));
        }
    }
    call
}

enum ShellScriptOwned {
    Inline(String),
    Undecodable,
    File,
    Stdin,
}

/// Whether `arg` names the PowerShell parameter `full` (lowercase), abbreviated to at least
/// `min` characters, with `-` or `/` and an optional `:value`.
fn is_ps_param(arg: &str, full: &str, min: usize) -> bool {
    let lower = arg.to_ascii_lowercase();
    let Some(name) = lower.strip_prefix('-').or_else(|| lower.strip_prefix('/')) else {
        return false;
    };
    let name = name.split_once(':').map_or(name, |(name, _)| name);
    name.len() >= min && full.starts_with(name)
}

/// PowerShell parameters that take a value, with the shortest abbreviation PowerShell accepts.
const POWERSHELL_VALUES: &[(&str, usize)] = &[
    ("executionpolicy", 2),
    ("ex", 2),
    ("ep", 2),
    ("windowstyle", 1),
    ("configurationname", 3),
    ("workingdirectory", 2),
    ("wd", 2),
    ("outputformat", 1),
    ("inputformat", 2),
    ("settingsfile", 2),
    ("custompipename", 2),
];

/// What `pwsh` runs: `-Command ...`, `-CommandWithArgs <script> args`, `-EncodedCommand
/// <base64>`, Windows PowerShell's first positional words, a file, or stdin.
fn powershell_script(args: &[String], windows_powershell: bool) -> ShellScriptOwned {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if !arg.starts_with(['-', '/']) {
            // Windows PowerShell reads positional words as a command; pwsh as a script file.
            return if windows_powershell {
                ShellScriptOwned::Inline(join_for(&args[i..], Dialect::PowerShell))
            } else {
                ShellScriptOwned::File
            };
        }
        if is_ps_param(arg, "commandwithargs", 8) || arg.eq_ignore_ascii_case("-cwa") {
            return match args.get(i + 1) {
                Some(script) if script == "-" => ShellScriptOwned::Stdin,
                Some(script) => ShellScriptOwned::Inline(script.clone()),
                None => ShellScriptOwned::Stdin,
            };
        }
        if is_ps_param(arg, "command", 1) && !is_ps_param(arg, "configurationname", 3) {
            let rest = &args[i + 1..];
            return match rest {
                [] => ShellScriptOwned::Stdin,
                [one] if one == "-" => ShellScriptOwned::Stdin,
                _ => ShellScriptOwned::Inline(script_of(rest, Dialect::PowerShell)),
            };
        }
        if is_ps_param(arg, "encodedcommand", 1) || arg.eq_ignore_ascii_case("-ec") {
            return match args.get(i + 1).and_then(|text| decode_powershell(text)) {
                Some(script) => ShellScriptOwned::Inline(script),
                None => ShellScriptOwned::Undecodable,
            };
        }
        if is_ps_param(arg, "file", 1) {
            return match args.get(i + 1) {
                Some(file) if file == "-" || is_stdin_path(file) => ShellScriptOwned::Stdin,
                _ => ShellScriptOwned::File,
            };
        }
        // `-Version` names the engine for Windows PowerShell and prints the version in pwsh;
        // `-Help` and `-?` print help: no script either way for pwsh.
        if is_ps_param(arg, "version", 1) {
            if windows_powershell {
                i += 2;
                continue;
            }
            return ShellScriptOwned::File;
        }
        if is_ps_param(arg, "help", 1) || arg == "-?" || arg == "/?" {
            return ShellScriptOwned::File;
        }
        let takes_value = POWERSHELL_VALUES
            .iter()
            .any(|(full, min)| is_ps_param(arg, full, *min));
        i += if takes_value { 2 } else { 1 };
    }
    ShellScriptOwned::Stdin
}

/// `-EncodedCommand` text: base64 of UTF-16LE.
fn decode_powershell(text: &str) -> Option<String> {
    let bytes = base64_decode(text)?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    };
    let symbols: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .take_while(|b| *b != b'=')
        .collect();
    if symbols.len() > MAX_ENCODED {
        return None;
    }
    let mut out = Vec::with_capacity(symbols.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for symbol in symbols {
        buffer = (buffer << 6) | value(symbol)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// The command line a shell gets as its remaining arguments: one argument is the line itself
/// (`cmd /c "a && b"`), several are joined.
fn script_of(words: &[String], dialect: Dialect) -> String {
    match words {
        [one] => one.clone(),
        _ => join_for(words, dialect),
    }
}

/// [`join`] for the dialect that reads the result. cmd keeps a caret unquoted, so its escape
/// still applies (`--for^ce` from a POSIX line is `--force` to cmd).
fn join_for(words: &[String], dialect: Dialect) -> String {
    let quote = |word: &String, special: &str, quoted: &dyn Fn(&str) -> String| {
        if !word.is_empty()
            && !word
                .chars()
                .any(|c| c.is_whitespace() || special.contains(c))
        {
            word.clone()
        } else {
            quoted(word)
        }
    };
    match dialect {
        Dialect::Posix => join(words),
        Dialect::PowerShell => words
            .iter()
            .map(|word| {
                quote(word, "'\"`$;&|<>(){}#,@", &|w| {
                    format!("'{}'", w.replace('\'', "''"))
                })
            })
            .collect::<Vec<_>>()
            .join(" "),
        Dialect::Cmd => words
            .iter()
            .map(|word| quote(word, "\"&|<>()", &|w| format!("\"{}\"", w.replace('"', ""))))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Words joined back into a command line for another round of splitting; a word with a space
/// or a character a lexer reads keeps its meaning in quotes.
pub fn join(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            if !word.is_empty()
                && !word
                    .chars()
                    .any(|c| c.is_whitespace() || "'\"$`;&|<>()#^\\".contains(c))
            {
                word.clone()
            } else if !word.contains('\'') {
                format!("'{word}'")
            } else {
                let escaped = word
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('$', "\\$")
                    .replace('`', "\\`");
                format!("\"{escaped}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calls(command: &str, dialect: Dialect) -> Vec<Vec<String>> {
        split(command, dialect).calls
    }

    #[test]
    fn posix_quotes_paths_and_redirections_stay_apart() {
        assert_eq!(
            calls(
                r#"git commit -m "a; b" && echo 'x | y' > out.txt 2>&1"#,
                Dialect::Posix
            ),
            vec![vec!["git", "commit", "-m", "a; b"], vec!["echo", "x | y"]]
        );
        assert_eq!(
            calls(r"rm my\ file", Dialect::Posix),
            vec![vec!["rm", "my file"]]
        );
        // Every backslash outside quotes escapes, as Git Bash reads it.
        assert_eq!(
            calls(r"g\it push \--force", Dialect::Posix),
            vec![vec!["git", "push", "--force"]]
        );
        assert_eq!(
            calls(r"git push $'--for\x63e'", Dialect::Posix),
            vec![vec!["git", "push", "--force"]]
        );
        assert!(calls("", Dialect::Posix).is_empty());
        assert!(calls("   ;; && ", Dialect::Posix).is_empty());
    }

    #[test]
    fn powershell_reads_backticks_doubled_quotes_and_plain_backslashes() {
        assert_eq!(
            calls(
                r#"Get-ChildItem "C:\code\app\"; Remove-Item -Recurse C:\Users\me"#,
                Dialect::PowerShell
            ),
            vec![
                vec!["Get-ChildItem", r"C:\code\app\"],
                vec!["Remove-Item", "-Recurse", r"C:\Users\me"],
            ]
        );
        assert_eq!(
            calls("git push --for`ce 'it''s'", Dialect::PowerShell),
            vec![vec!["git", "push", "--force", "it's"]]
        );
        assert_eq!(
            calls("Remove-Item \u{2013}Recurse x", Dialect::PowerShell),
            vec![vec!["Remove-Item", "-Recurse", "x"]]
        );
        assert_eq!(
            calls("git push \u{201c}--force\u{201d}", Dialect::PowerShell),
            vec![vec!["git", "push", "--force"]]
        );
    }

    #[test]
    fn cmd_removes_carets_and_keeps_backslashes() {
        assert_eq!(
            calls(r#"git push --for^ce & del /s "C:\x\""#, Dialect::Cmd),
            vec![vec!["git", "push", "--force"], vec!["del", "/s", r"C:\x\"]]
        );
    }

    #[test]
    fn inline_scripts_are_split_in_their_own_dialect() {
        for (command, dialect) in [
            ("cmd /r git push --for^ce", Dialect::Posix),
            ("pwsh -cwa 'git push --for`ce'", Dialect::Posix),
            ("pwsh -CommandWithArgs 'git push --force' x", Dialect::Posix),
            ("bash -c 'git push \\--force'", Dialect::PowerShell),
        ] {
            let found = calls(command, dialect);
            assert!(
                found.contains(&vec!["git".to_owned(), "push".into(), "--force".into()]),
                "{command}: {found:?}"
            );
        }
    }

    #[test]
    fn open_quotes_deep_nesting_and_many_calls_are_reported() {
        for (command, dialect) in [
            (r#"echo "C:\temp\"; rm -rf /"#, Dialect::Posix),
            ("echo 'open", Dialect::Posix),
            ("echo $(git push", Dialect::Posix),
            ("echo `x", Dialect::Posix),
            ("echo \"open", Dialect::PowerShell),
            ("echo \"open", Dialect::Cmd),
            ("pwsh -EncodedCommand !!!", Dialect::Posix),
        ] {
            assert!(split(command, dialect).unclosed, "{command}");
        }
        let mut deep = "git push --force".to_owned();
        for _ in 0..8 {
            deep = format!("sh -c {}", join(&[deep]));
        }
        assert!(split(&deep, Dialect::Posix).truncated);
        let many = "true;".repeat(600);
        let analysis = split(&many, Dialect::Posix);
        assert!(analysis.truncated);
        assert_eq!(analysis.calls.len(), MAX_CALLS);
        assert!(!split("git status", Dialect::Posix).truncated);
    }

    #[test]
    fn shells_reading_stdin_are_reported() {
        for command in [
            "bash <<< 'git push --force'",
            "echo 'git push --force' | sh",
            "bash -s",
            "curl x | bash -",
            "pwsh -Command -",
            "cat x | pwsh",
            "cmd < script.cmd",
        ] {
            assert!(split(command, Dialect::Posix).stdin_script, "{command}");
        }
        for command in [
            "bash script.sh",
            "sh -c 'ls'",
            "pwsh -File x.ps1",
            "cmd /c dir",
        ] {
            assert!(!split(command, Dialect::Posix).stdin_script, "{command}");
        }
    }

    #[test]
    fn shell_options_with_values_do_not_hide_the_script() {
        for command in [
            "bash -o pipefail -c 'git push --force'",
            "bash --rcfile /dev/null -c 'git push --force'",
            "bash +O extglob -c 'git push --force'",
            "bash -c -o pipefail 'git push --force'",
            "wsl 'git push --force'",
            "wsl -d Ubuntu git push --force",
            "wsl -- git push --force",
            "cmd /c start /b git push --force",
            "cmd /c start \"\" git push --force",
            "Start-Process git -ArgumentList 'push','--force'",
            "Start-Process -FilePath git -ArgumentList 'push --force' -Wait",
            "saps git 'push --force'",
        ] {
            let found = calls(command, Dialect::PowerShell);
            let found_posix = calls(command, Dialect::Posix);
            let wanted = vec!["git".to_owned(), "push".into(), "--force".into()];
            assert!(
                found.contains(&wanted) || found_posix.contains(&wanted),
                "{command}: {found:?} {found_posix:?}"
            );
        }
        for command in [
            "bash --version",
            "bash --help",
            "pwsh -v",
            "pwsh -Version",
            "pwsh -?",
        ] {
            assert!(!split(command, Dialect::Posix).stdin_script, "{command}");
        }
        // Windows PowerShell's -Version names an engine; the command after it still counts.
        assert!(
            calls("powershell -Version 2 -Command git status", Dialect::Posix)
                .contains(&vec!["git".to_owned(), "status".into()])
        );
    }

    #[test]
    fn redirection_targets_and_parenthesized_values_are_kept_apart() {
        let analysis = split(
            "cd /x; echo a > one.txt 2>> two.log; > three",
            Dialect::Posix,
        );
        assert_eq!(
            analysis.redirects,
            vec![
                (1, "one.txt".to_owned()),
                (1, "two.log".into()),
                (2, "three".into())
            ]
        );
        let analysis = split(
            r#"Remove-Item -Recurse (Join-Path $HOME "x")"#,
            Dialect::PowerShell,
        );
        assert_eq!(
            analysis.calls,
            vec![
                vec!["Join-Path".to_owned(), "$HOME".into(), "x".into()],
                vec!["Remove-Item".into(), "-Recurse".into(), UNKNOWN.into()],
            ]
        );
        // `$"..."` is a translated string, `$@` stays for the matcher.
        assert_eq!(
            calls(r#"git push $"--force""#, Dialect::Posix),
            vec![vec!["git", "push", "--force"]]
        );
    }

    #[test]
    fn nested_start_ends_at_the_depth_limit() {
        let command = format!("{}git push --force", "start ".repeat(10_900));
        let analysis = std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || split(&command, Dialect::Cmd))
            .unwrap()
            .join()
            .unwrap();
        assert!(analysis.truncated);
        // A few levels still unwrap.
        assert!(
            calls("start start git push --force", Dialect::Cmd).contains(&vec![
                "git".to_owned(),
                "push".into(),
                "--force".into()
            ])
        );
    }

    fn push_force() -> Vec<String> {
        vec!["git".to_owned(), "push".into(), "--force".into()]
    }

    #[test]
    fn heredoc_body_is_data_and_keeps_quotes_in_sync() {
        // The apostrophe in the body is data: the command after the delimiter line is seen and
        // the quote at the end of the last line belongs to a comment.
        let analysis = split(
            "cat <<EOF\nit's\nEOF\ngit push --force\necho marker #'",
            Dialect::Posix,
        );
        assert!(!analysis.unclosed, "{analysis:?}");
        assert_eq!(
            analysis.calls,
            vec![
                vec!["cat".to_owned()],
                push_force(),
                vec!["echo".into(), "marker".into()]
            ]
        );
        assert_eq!(analysis.info[0].fed, ["it's\n"]);
        // `<<-` strips leading tabs from the body and the delimiter line.
        let analysis = split(
            "cat <<-END\n\tdon't\n\tEND\ngit push --force",
            Dialect::Posix,
        );
        assert!(!analysis.unclosed);
        assert!(analysis.calls.contains(&push_force()), "{analysis:?}");
        // A quoted delimiter makes the body plain data; an unquoted one runs its substitutions.
        for quoted in ["'EOF'", "\"EOF\"", "\\EOF", "E\"O\"F"] {
            let command = format!("cat <<{quoted}\n$(git push --force)\nEOF\necho marker");
            let analysis = split(&command, Dialect::Posix);
            assert!(!analysis.calls.contains(&push_force()), "{command}");
            assert!(!analysis.unclosed, "{command}");
        }
        for body in [
            "$(git push --force)",
            "`git push --force`",
            "x $(git push --force) y",
        ] {
            let command = format!("cat <<EOF\n{body}\nEOF");
            assert!(
                calls(&command, Dialect::Posix).contains(&push_force()),
                "{command}"
            );
        }
        // Escaped in an unquoted body: plain text.
        assert!(
            !calls("cat <<EOF\n\\$(git push --force)\nEOF", Dialect::Posix).contains(&push_force())
        );
        // Two documents on one line are read one after the other.
        let analysis = split(
            "cat <<A; cat <<B\na'\nA\nb'\nB\ngit push --force",
            Dialect::Posix,
        );
        assert!(!analysis.unclosed);
        assert!(analysis.calls.contains(&push_force()));
        // A delimiter that never comes leaves the rest unread: asked about.
        assert!(split("cat <<EOF\ngit push --force", Dialect::Posix).unclosed);
        // A here-string is no here-document, and a document without a body hides nothing.
        assert!(!split("cat <<< 'x'", Dialect::Posix).unclosed);
        assert!(!split("cat <<EOF", Dialect::Posix).unclosed);
        assert!(!split("echo $((1 << 2))", Dialect::Posix).unclosed);
    }

    #[test]
    fn heredoc_inside_a_substitution_keeps_its_quotes_and_parens_to_itself() {
        // The way agents write a commit message: the apostrophes and the `)` in the body are
        // text, not the end of the substitution.
        let message =
            "git commit -m \"$(cat <<'EOF'\nfix: don't drop it (1)\n\nIt's kept.\nEOF\n)\"";
        let analysis = split(message, Dialect::Posix);
        assert!(!analysis.unclosed, "{analysis:?}");
        assert!(analysis.calls.iter().any(|call| call[0] == "git"));
        // A `)` in the body does not end the substitution early, so what runs after the
        // delimiter line inside it is seen, quoted outside or not.
        for command in [
            "echo \"$(cat <<'EOF'\n)\nEOF\ngit push --force\n)\"",
            "echo $(cat <<EOF\n)\nEOF\ngit push --force\n)",
            "echo \"$(cat <<-EOF\n\t)\n\tEOF\ngit push --force\n)\"",
        ] {
            let analysis = split(command, Dialect::Posix);
            assert!(analysis.calls.contains(&push_force()), "{command:?}");
            assert!(!analysis.unclosed, "{command:?}");
        }
        // A here-string and arithmetic shifts inside a substitution are no documents.
        assert!(!split("echo \"$(cat <<< 'x')\"", Dialect::Posix).unclosed);
        assert!(!split("echo \"$((1 << 2))\"\necho marker", Dialect::Posix).unclosed);
        // A document in a substitution whose delimiter never comes is still asked about.
        assert!(split("echo \"$(cat <<EOF\nit's\n)\"", Dialect::Posix).unclosed);
    }

    #[test]
    fn powershell_here_strings_are_one_word() {
        let analysis = split("echo @'\nx'\n'@\ngit push --force #'", Dialect::PowerShell);
        assert!(!analysis.unclosed, "{analysis:?}");
        assert_eq!(analysis.calls[0], ["echo", "x'"]);
        assert!(analysis.calls.contains(&push_force()));
        let analysis = split(
            "echo @\"\nx\"\n\"@\ngit push --force #\"",
            Dialect::PowerShell,
        );
        assert!(!analysis.unclosed, "{analysis:?}");
        assert!(analysis.calls.contains(&push_force()));
        // A double quoted here-string runs its substitutions; a single quoted one does not.
        assert!(
            calls("$x = @\"\n$(git push --force)\n\"@", Dialect::PowerShell)
                .contains(&push_force())
        );
        assert!(
            !calls("$x = @'\n$(git push --force)\n'@", Dialect::PowerShell).contains(&push_force())
        );
        // The end is only a line starting with the quote and `@`.
        assert!(split("echo @'\nx '@\n", Dialect::PowerShell).unclosed);
        assert!(split("echo @\"\nx\n", Dialect::PowerShell).unclosed);
    }

    #[test]
    fn braces_group_only_as_words_of_their_own() {
        assert_eq!(
            calls("{git,push,--force}", Dialect::Posix),
            vec![vec!["{git,push,--force}"]]
        );
        assert_eq!(
            calls("git push {-f,-q}", Dialect::Posix),
            vec![vec!["git", "push", "{-f,-q}"]]
        );
        assert_eq!(
            calls("{ git push --force; }", Dialect::Posix),
            vec![push_force()]
        );
        assert_eq!(
            calls("find . -exec rm {} +", Dialect::Posix)[0],
            ["find", ".", "-exec", "rm", "{}", "+"]
        );
    }

    #[test]
    fn scripts_read_from_stdin_or_a_process_are_reported() {
        for command in [
            "echo 'git push --force' | bash /dev/stdin",
            "echo 'git push --force' | bash /dev/fd/0",
            "echo 'git push --force' | sh /proc/self/fd/0",
            "echo 'git push --force' | pwsh -File -",
            "echo 'git push --force' | source /dev/stdin",
            ". <(echo git push --force)",
            "bash <(echo git push --force)",
            "source <(curl x)",
        ] {
            assert!(split(command, Dialect::Posix).stdin_script, "{command}");
        }
        // A sourced file is a script file, not a wrapper around a program call.
        let analysis = split(". ./env.sh", Dialect::Posix);
        assert!(!analysis.stdin_script);
        assert_eq!(analysis.calls, vec![vec![".", "./env.sh"]]);
        // PowerShell's dot operator runs what follows.
        assert!(calls(". git push --force", Dialect::PowerShell).contains(&push_force()));
    }

    #[test]
    fn pipelines_tell_each_call_where_its_input_comes_from() {
        let analysis = split(
            r"Get-ChildItem C:\x -Recurse | Remove-Item -Force; echo y",
            Dialect::PowerShell,
        );
        assert_eq!(analysis.info.len(), analysis.calls.len());
        assert_eq!(analysis.info[0].upstream, None);
        assert_eq!(analysis.info[1].upstream, Some(0));
        assert!(analysis.info[1].piped);
        assert!(!analysis.info[2].piped);
        let analysis = split(
            "echo 'DROP TABLE x' | psql && psql <<< 'SELECT 1'",
            Dialect::Posix,
        );
        assert!(analysis.info[1].piped);
        assert!(!analysis.info[2].piped);
        assert_eq!(analysis.info[2].fed, ["SELECT 1"]);
        assert_eq!(
            split("cmd /c git status", Dialect::Posix).info[0].dialect,
            Dialect::Cmd
        );
    }

    #[test]
    fn more_wrappers_and_inline_commands_are_unwrapped() {
        for (command, dialect) in [
            ("winpty git push --force", Dialect::Posix),
            ("setsid -f git push --force", Dialect::Posix),
            ("stdbuf -o L git push --force", Dialect::Posix),
            ("ionice -c 3 git push --force", Dialect::Posix),
            ("busybox sh -c 'git push --force'", Dialect::Posix),
            ("trap 'git push --force' EXIT", Dialect::Posix),
            ("find . -name x -exec git push --force \\;", Dialect::Posix),
            ("find . -execdir git push --force {} +", Dialect::Posix),
            (
                "& ([scriptblock]::Create('git push --force'))",
                Dialect::PowerShell,
            ),
            (
                "$ExecutionContext.InvokeCommand.InvokeScript('git push --force')",
                Dialect::PowerShell,
            ),
        ] {
            let found = calls(command, dialect);
            assert!(
                found
                    .iter()
                    .any(|call| call.len() >= 3 && call[..3] == push_force()[..]),
                "{command}: {found:?}"
            );
        }
        // find's `{}` stands for what it found below its start folder.
        assert!(
            calls("find /tmp/marker -exec rm -r {} +", Dialect::Posix).contains(&vec![
                "rm".to_owned(),
                "-r".into(),
                "/tmp/marker/*".into()
            ])
        );
    }

    #[test]
    fn huge_here_documents_and_here_strings_stay_bounded() {
        let body = "line with 'quote' and $(x) and `y`\n".repeat(1800);
        for (text, dialect) in [
            (
                format!("cat <<EOF\n{body}EOF\ngit push --force"),
                Dialect::Posix,
            ),
            (format!("cat <<EOF\n{body}"), Dialect::Posix),
            ("cat <<A\n".repeat(8000), Dialect::Posix),
            (
                format!("echo @\"\n{body}\"@\ngit push --force"),
                Dialect::PowerShell,
            ),
            (format!("echo @'\n{body}"), Dialect::PowerShell),
            ("@'\n".repeat(20_000), Dialect::PowerShell),
        ] {
            assert!(text.len() <= 64 * 1024 + 64, "{}", text.len());
            let started = std::time::Instant::now();
            let analysis = split(&text, dialect);
            assert!(analysis.calls.len() <= MAX_CALLS);
            assert!(started.elapsed() < std::time::Duration::from_secs(5));
        }
    }

    #[test]
    fn huge_inputs_stay_bounded() {
        for text in [
            "$(".repeat(30_000),
            "sh -c ".repeat(10_000) + "x",
            format!("powershell -EncodedCommand {}", "QQBBAA".repeat(10_000)),
            "\"".to_owned() + &"\\\"$(".repeat(20_000),
            "`".repeat(60_000),
        ] {
            let started = std::time::Instant::now();
            let analysis = split(&text, Dialect::Posix);
            let _ = split(&text, Dialect::PowerShell);
            let _ = split(&text, Dialect::Cmd);
            assert!(analysis.calls.len() <= MAX_CALLS);
            assert!(started.elapsed() < std::time::Duration::from_secs(5));
        }
    }
}
