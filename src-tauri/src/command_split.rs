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
/// Offsets a launcher's inner command is read at, so an option the guard does not know cannot
/// hide the command behind it.
const MAX_LAUNCH_OFFSETS: usize = 8;

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
    /// Names of variables the command writes (a `for` header, an assignment, `read`, `printf -v`,
    /// `Set-Variable`, ...), as the lexers see the raw words, once per write. A loop variable is
    /// placed in its body only when it is written exactly once (its own header).
    pub writes: Vec<String>,
    /// The command runs an inline script (`sh -c`, `pwsh -Command`, `cmd /c`, `eval`, a
    /// substitution): a variable could be written where the loop reader does not see it, so a loop
    /// variable is not placed in its body.
    pub has_inline_script: bool,
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
    /// A POSIX call with a brace outside quotes: only then does the shell expand braces, so a
    /// quoted JSON text is not taken apart.
    pub braces: bool,
    /// A PowerShell call inside the script block of a piped `ForEach-Object` (`%`): its `$_`
    /// is each item the call before the `ForEach-Object` hands on.
    pub per_item: bool,
    /// That call, when it is one call of [`Analysis::calls`].
    pub items: Option<usize>,
    /// A cmd call in a script run with delayed expansion (`cmd /v:on /c ...`): only there is
    /// `!name!` a variable; plain cmd prints it as written.
    pub delayed: bool,
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
    /// A brace stood outside quotes in one of its words.
    braces: bool,
    /// The piece of the piped `ForEach-Object` whose script block holds it.
    item_of: Option<usize>,
}

/// One part of a script in order: a simple command, a nested script to split, or the inside of
/// a POSIX arithmetic context (`((...))`, `$((...))`), read as commands without here-documents
/// so its substitutions and any subshell bash may make of it are still seen.
enum Piece {
    Command(Simple),
    Script(String, Dialect),
    Arithmetic(String),
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
    /// Open `${` in the current word: `<<` inside is text, not a here-document.
    params: usize,
    /// Open `$[` (old arithmetic) in the current word.
    brackets: usize,
    /// A brace outside quotes went into a word of the current command.
    braces: bool,
    /// Open PowerShell script blocks, each with the piece of the piped `ForEach-Object` whose
    /// items its `$_` stands for (inherited by blocks nested in one).
    blocks: Vec<Option<usize>>,
    /// Open cmd blocks (`(`): a `)` glued to a word closes one there.
    parens: usize,
    /// cmd's `start` of the current command has had its quoted window title.
    titled: bool,
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
                braces: self.braces,
                item_of: self.blocks.last().copied().flatten(),
            }));
            Some(self.pieces.len() - 1)
        } else {
            None
        };
        self.piped = false;
        self.braces = false;
        self.titled = false;
        if self.params > 0 || self.brackets > 0 {
            // A `${` or `$[` left open: the shell reads on where the lexer stopped.
            self.unclosed = true;
        }
        self.params = 0;
        self.brackets = 0;
        for mut doc in std::mem::take(&mut self.open_docs) {
            doc.owner = owner;
            self.ready_docs.push(doc);
        }
    }

    /// A PowerShell script block opens: a piped `ForEach-Object` (`%`, `foreach`) in front of
    /// it, with no more than parameters (`-Process`), makes its `$_` that command's items.
    fn open_block(&mut self) {
        self.end_word();
        let for_each = self.piped
            && self.words.first().is_some_and(|first| {
                ["foreach-object", "%", "foreach"]
                    .iter()
                    .any(|name| first.eq_ignore_ascii_case(name))
            })
            && self.words[1..].iter().all(|word| word.starts_with('-'));
        let inherited = self.blocks.last().copied().flatten();
        self.end_command();
        let owner = if for_each {
            self.pieces.len().checked_sub(1)
        } else {
            inherited
        };
        self.blocks.push(owner);
    }

    /// A PowerShell script block closes.
    fn close_block(&mut self) {
        self.end_command();
        self.blocks.pop();
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
    closing_paren_nested(chars, start, dialect, 0)
}

/// Substitutions inside double quotes inside a substitution followed by [`closing_paren`]; a
/// deeper one is not looked for and counts as unclosed.
const MAX_QUOTED_NESTING: usize = 32;

/// Reserved words after which a POSIX word is still in command position (`if case`, `do ((`).
const KEEP_COMMAND_POSITION: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "for", "!", "{", "time",
];

/// [`closing_paren`] `nesting` substitutions deep. Modelled as bash reads a `$(...)`: `#`
/// comments at a word start, `case ... esac` (whose pattern `)` closes nothing), arithmetic
/// `((...))`, `$((...))`, `${...}` and `$[...]` (where `<<` shifts), `$'...'`, backticks and
/// substitutions inside double quotes. What it cannot place (a here-document delimiter bash
/// reads differently, nesting past [`MAX_QUOTED_NESTING`]) gives `None`: asked about.
fn closing_paren_nested(
    chars: &[char],
    start: usize,
    dialect: Dialect,
    nesting: usize,
) -> Option<usize> {
    if nesting > MAX_QUOTED_NESTING {
        return None;
    }
    let posix = dialect == Dialect::Posix;
    // `$((...))` is arithmetic: `<<` there shifts.
    let arithmetic = chars.get(start) == Some(&'(');
    // Here-documents opened on the current line: delimiter and whether tabs are stripped.
    let mut docs: Vec<(String, bool)> = Vec::new();
    let mut depth = 1usize;
    // The paren depth an arithmetic `((` started at, open `${` and `$[`.
    let mut arith_at: Option<usize> = None;
    let mut params = 0usize;
    let mut brackets = 0usize;
    // Paren depths of open `case` statements: a `)` at that depth ends a pattern.
    let mut cases: Vec<usize> = Vec::new();
    let mut command_position = true;
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];
        let quiet = arithmetic || arith_at.is_some() || params > 0 || brackets > 0;
        let word_start = i == start
            || chars
                .get(i.wrapping_sub(1))
                .is_some_and(|p| p.is_whitespace() || ";&|()".contains(*p));
        if word_start && !quiet && !c.is_whitespace() && !";&|()<>".contains(c) {
            if c == '#' {
                // A comment up to the line break, which is read next (here-document bodies).
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if posix {
                let word: String = chars[i..]
                    .iter()
                    .take_while(|w| !w.is_whitespace() && !";&|()<>".contains(**w))
                    .take(8)
                    .collect();
                if command_position && word == "case" {
                    cases.push(depth);
                } else if command_position && word == "esac" {
                    cases.pop();
                }
                command_position = KEEP_COMMAND_POSITION.contains(&word.as_str());
            }
        }
        match c {
            ';' | '&' | '|' | '\n' => command_position = true,
            _ => {}
        }
        match c {
            // A here-string (`<<<`) is no here-document.
            '<' if posix && chars.get(i + 1) == Some(&'<') && chars.get(i + 2) == Some(&'<') => {
                i += 2;
            }
            '<' if posix && !quiet && chars.get(i + 1) == Some(&'<') => {
                let mut j = i + 2;
                let strip_tabs = chars.get(j) == Some(&'-');
                if strip_tabs {
                    j += 1;
                }
                while matches!(chars.get(j), Some(' ' | '\t')) {
                    j += 1;
                }
                let (delimiter, _, end) = heredoc_word(chars, j);
                if !plain_delimiter(&delimiter) {
                    return None;
                }
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
            '\\' if posix => i += 1,
            '`' if dialect == Dialect::PowerShell => i += 1,
            '`' if posix => i = closing_backtick(chars, i + 1)?,
            '$' if posix && chars.get(i + 1) == Some(&'\'') => {
                i = ansi_c(chars, i + 2).1?;
            }
            '$' if posix && chars.get(i + 1) == Some(&'{') => {
                params += 1;
                i += 1;
            }
            '$' if posix && chars.get(i + 1) == Some(&'[') => {
                brackets += 1;
                i += 1;
            }
            '}' if params > 0 => params -= 1,
            ']' if brackets > 0 => brackets -= 1,
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    match chars[i] {
                        '\\' if posix => i += 1,
                        '`' if dialect == Dialect::PowerShell => i += 1,
                        '`' if posix => i = closing_backtick(chars, i + 1)?,
                        '$' if chars.get(i + 1) == Some(&'(') => {
                            i = closing_paren_nested(chars, i + 2, dialect, nesting + 1)?;
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            '(' => {
                let dollar = i > 0 && chars[i - 1] == '$';
                if posix && !quiet && chars.get(i + 1) == Some(&'(') && (command_position || dollar)
                {
                    arith_at = Some(depth);
                }
                depth += 1;
                command_position = true;
            }
            ')' if !quiet && cases.last() == Some(&depth) => command_position = true,
            ')' => {
                depth -= 1;
                if arith_at == Some(depth) {
                    arith_at = None;
                }
                if depth == 0 {
                    return Some(i);
                }
                command_position = true;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Whether bash reads a here-document delimiter the way [`heredoc_word`] does: one with a `$`,
/// a backtick or a carriage return is read differently (`<<a$(b)`, `<<'EOF\r'`), so its body
/// cannot be placed.
fn plain_delimiter(delimiter: &str) -> bool {
    !delimiter.contains(['$', '`', '\r'])
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
    lex_posix_in(text, false)
}

/// [`lex_posix`]; in an arithmetic context (`arithmetic`) `<` and `>` only separate words.
fn lex_posix_in(text: &str, arithmetic: bool) -> Words {
    let chars: Vec<char> = text.chars().collect();
    let mut w = Words::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\r' => w.end_word(),
            '<' | '>' if arithmetic => w.end_word(),
            // Inside `${...}` and `$[...]` a `<<` shifts and nothing groups.
            '<' | '>' | '#' | '(' | ')' | '{' | '|' | '&' | ';'
                if w.params > 0 || w.brackets > 0 =>
            {
                w.push(c)
            }
            '}' if w.params > 0 => {
                w.params -= 1;
                w.push(c);
            }
            ']' if w.brackets > 0 => {
                w.brackets -= 1;
                w.push(c);
            }
            '$' if next == Some('{') => {
                w.params += 1;
                w.push_str("${");
                i += 1;
            }
            '$' if next == Some('[') => {
                w.brackets += 1;
                w.push_str("$[");
                i += 1;
            }
            // `((...))` at the start of a command is arithmetic: what it holds is read as
            // commands without here-documents.
            '(' if next == Some('(')
                && !w.in_word
                && w.words
                    .iter()
                    .all(|word| KEEP_COMMAND_POSITION.contains(&word.as_str())) =>
            {
                w.end_command();
                match closing_paren(&chars, i + 1, Dialect::Posix) {
                    Some(end) => {
                        w.pieces
                            .push(Piece::Arithmetic(text_of(&chars, i + 1, end)));
                        i = end;
                    }
                    None => {
                        w.unclosed = true;
                        i = chars.len();
                    }
                }
            }
            '\n' => {
                w.end_command();
                if !w.ready_docs.is_empty() {
                    i = heredoc_bodies(&mut w, &chars, i + 1);
                    continue;
                }
            }
            ';' => w.end_command(),
            // An extended glob (`--@(force|x)`, `+(a)`) is one word; a zsh glob qualifier that
            // runs code (`*(e:'cmd':)`, `*(+fn)`) cannot be followed.
            '(' if w.in_word && w.word.ends_with(['@', '+', '!', '?', '*']) => {
                match extglob_end(&chars, i + 1) {
                    Some(end) => {
                        let inner = text_of(&chars, i + 1, end);
                        if zsh_qualifier_runs_code(&inner) {
                            w.unclosed = true;
                        }
                        w.push('(');
                        w.push_str(&inner);
                        w.push(')');
                        i = end;
                    }
                    None => {
                        w.unclosed = true;
                        i = chars.len();
                    }
                }
            }
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
                if (delimiter.is_empty() && !quoted) || !plain_delimiter(&delimiter) {
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
            // zsh's `=(...)` is a process substitution too (a temporary file); in bash it is a
            // syntax error, so reading it as one costs nothing there.
            '<' | '>' if next == Some('(') => {
                i = posix_substitution_as(&mut w, &chars, i + 2, true, PROCESS_FILE);
            }
            '=' if next == Some('(') && !w.in_word => {
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
            other => {
                if other == '{' {
                    w.braces = true;
                }
                w.push(other)
            }
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
    let text = text_of(chars, start, end);
    if paren && chars.get(start) == Some(&'(') {
        // `$((...))`: arithmetic.
        w.pieces.push(Piece::Arithmetic(text));
        w.push_str(value);
    } else {
        // `$(pwd)` is the working folder, which Hangar places like `$PWD`.
        let value = if value == UNKNOWN && is_pwd(&text) {
            WORKING_FOLDER
        } else {
            value
        };
        w.substitution(text, Dialect::Posix, value);
    }
    end
}

/// The working folder as a word: `$PWD`, also what `$(pwd)` and PowerShell's `$pwd` give.
pub const WORKING_FOLDER: &str = "$PWD";

/// Whether a substitution's script only prints the working folder.
fn is_pwd(script: &str) -> bool {
    matches!(
        script.split_whitespace().collect::<Vec<_>>().as_slice(),
        ["pwd"] | ["pwd", "-L" | "-P"] | ["/bin/pwd"]
    )
}

/// The index of the `)` closing an extended glob's `(` before `start`, nested parens counted and
/// quotes skipped; `None` when the line ends first.
fn extglob_end(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        match c {
            '\n' => return None,
            '\\' => i += 1,
            '\'' | '"' => {
                i += 1;
                while chars.get(i).is_some_and(|q| *q != c) {
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

/// A zsh glob qualifier that runs shell code: `e` with any delimiter (`e:cmd:`, `e'cmd'`) or
/// `+function`.
fn zsh_qualifier_runs_code(inner: &str) -> bool {
    inner.starts_with('+')
        || inner.char_indices().any(|(at, c)| {
            c == 'e' && inner[at + 1..].starts_with([':', '\'', '"', '[', '{', '|', '/'])
        })
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
                let text = text_of(&chars, i + 2, end);
                w.pieces.push(if chars.get(i + 2) == Some(&'(') {
                    Piece::Arithmetic(text)
                } else {
                    Piece::Script(text, Dialect::Posix)
                });
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
            // The call operator at a command's start is a word of its own, so what it calls
            // (`& (Get-Command git) push`) is read as a value; elsewhere `&` ends a command.
            '&' if next != Some('&') && !w.in_word && w.words.is_empty() => {
                w.push('&');
                w.end_word();
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
            // Hangar cannot know, and what it runs is split on its own; plain literals and a
            // `Join-Path` of plain words are read as their value.
            '(' if !w.words.is_empty() || w.in_word => {
                i = match ps_literal_in_parens(&chars, i + 1)
                    .or_else(|| ps_join_path(&chars, i + 1))
                {
                    Some((text, end)) => {
                        w.push_str(&text);
                        end
                    }
                    None => ps_substitution(&mut w, &chars, i + 1),
                }
            }
            // `@(...)`: an array; like `$(...)`, literals are their text.
            '@' if next == Some('(') => i = ps_subexpression(&mut w, &chars, i + 2),
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
            // `${env:x}`: a variable name, up to the first `}` not escaped by a backtick.
            '{' if w.in_word && w.word.ends_with('$') => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '}' {
                    j += if chars[j] == '`' { 2 } else { 1 };
                }
                if j >= chars.len() {
                    w.unclosed = true;
                }
                w.push_str(&text_of(&chars, i, (j + 1).min(chars.len())));
                i = j;
            }
            // `@{a=1}`: a hashtable, one word; what it holds is split as a script of its own,
            // since a value may run a command (`@{a=$(...)}`).
            '{' if w.in_word && w.word.ends_with('@') => match ps_closing_brace(&chars, i + 1) {
                Some(end) => {
                    w.substitution(
                        text_of(&chars, i + 1, end),
                        Dialect::PowerShell,
                        &text_of(&chars, i, end + 1),
                    );
                    i = end;
                }
                None => {
                    w.unclosed = true;
                    i = chars.len();
                }
            },
            // A script block (`& {...}`, `%{...}`, `if (x) {...}`, `-ScriptBlock {...}`): its
            // braces end the word and the command wherever they stand, so what runs in it is
            // a call of its own.
            '{' => w.open_block(),
            '}' => w.close_block(),
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
                            let (text, taken) = ps_escape(&chars, i + 1);
                            w.word.push_str(&text);
                            i += 1 + taken;
                        }
                        Some('$') if chars.get(i + 1) == Some(&'(') => {
                            i = ps_subexpression(&mut w, &chars, i + 2) + 1;
                        }
                        Some(&other) => {
                            w.word.push(other);
                            i += 1;
                        }
                    }
                }
            }
            '$' if next == Some('(') => i = ps_subexpression(&mut w, &chars, i + 2),
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

/// `("..\build")`, `('a','b')`: a parenthesized argument of plain string literals, with nothing
/// PowerShell would expand in them. Their text ([`ps_literal_list`]) and the index of the
/// closing parenthesis.
fn ps_literal_in_parens(chars: &[char], start: usize) -> Option<(String, usize)> {
    let end = closing_paren(chars, start, Dialect::PowerShell)?;
    ps_literal_list(&text_of(chars, start, end)).map(|text| (text, end))
}

/// `$(...)` or `@(...)` from `start` (after the paren); returns the index of its closing paren.
/// Empty, it adds nothing (`--for$()ce`); plain literals are their text (`$('--force')`,
/// `@('a','b')`); anything else runs and is an unknown value.
fn ps_subexpression(w: &mut Words, chars: &[char], start: usize) -> usize {
    if let Some(end) = closing_paren(chars, start, Dialect::PowerShell) {
        let inner = text_of(chars, start, end);
        if inner.trim().is_empty() {
            w.start();
            return end;
        }
        // `$(pwd)`, `$(Get-Location)`: the working folder.
        let lower = inner.trim().to_ascii_lowercase();
        if matches!(lower.as_str(), "pwd" | "get-location" | "gl" | "$pwd") {
            w.push_str(WORKING_FOLDER);
            return end;
        }
        if let Some(text) = ps_literal_list(&inner) {
            w.push_str(&text);
            return end;
        }
    }
    ps_substitution(w, chars, start)
}

/// Plain PowerShell string literals separated by commas (`'a'`, `"b"`, `'a','b'`, `,'a'`),
/// joined with commas, as PowerShell hands an array to a native program as one word each (the
/// guard splits PowerShell words at commas). `None` for anything else, such as a double quoted
/// string with a variable in it.
fn ps_literal_list(inner: &str) -> Option<String> {
    let chars: Vec<char> = inner.chars().collect();
    let blank = |i: &mut usize| {
        while chars.get(*i).is_some_and(|c| c.is_whitespace()) {
            *i += 1;
        }
    };
    let mut i = 0;
    blank(&mut i);
    // The unary comma: `,'a'` is an array of one.
    if chars.get(i) == Some(&',') {
        i += 1;
    }
    let mut items: Vec<String> = Vec::new();
    loop {
        blank(&mut i);
        // A type cast in front (`[string]'x'`) keeps a string a string.
        while chars.get(i) == Some(&'[') {
            let close = chars[i..].iter().position(|c| *c == ']')? + i;
            let name_ok = chars[i + 1..close]
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_');
            if !name_ok || close == i + 1 {
                return None;
            }
            i = close + 1;
            blank(&mut i);
        }
        let (text, next) = ps_plain_literal(&chars, i)?;
        items.push(text);
        i = next;
        blank(&mut i);
        match chars.get(i) {
            None => break,
            Some(',') => i += 1,
            Some(_) => return None,
        }
    }
    Some(items.join(","))
}

/// One plain string literal at `start`: its text and the index after its closing quote.
fn ps_plain_literal(chars: &[char], start: usize) -> Option<(String, usize)> {
    let open = *chars.get(start)?;
    let single = ps_single(open);
    if !single && !ps_double(open) {
        return None;
    }
    let closes = |c: char| if single { ps_single(c) } else { ps_double(c) };
    let mut text = String::new();
    let mut i = start + 1;
    loop {
        let c = *chars.get(i)?;
        if closes(c) {
            if chars.get(i + 1).is_some_and(|n| closes(*n)) {
                text.push(if single { '\'' } else { '"' });
                i += 2;
                continue;
            }
            return Some((text, i + 1));
        }
        match c {
            '`' if !single => {
                let (escaped, taken) = ps_escape(chars, i + 1);
                text.push_str(&escaped);
                i += 1 + taken;
            }
            // A variable or subexpression: not plain.
            '$' if !single => return None,
            other => {
                text.push(other);
                i += 1;
            }
        }
    }
}

/// What a backtick escape in a PowerShell double quoted string stands for (`` `n ``, `` `t ``,
/// `` `u{2d} ``, ...; any other character as itself), from `at` (after the backtick), and how
/// many characters it takes.
fn ps_escape(chars: &[char], at: usize) -> (String, usize) {
    let Some(&c) = chars.get(at) else {
        return (String::new(), 0);
    };
    let plain = |c: char| (c.to_string(), 1);
    match c {
        '0' => plain('\0'),
        'a' => plain('\u{7}'),
        'b' => plain('\u{8}'),
        'e' => plain('\u{1b}'),
        'f' => plain('\u{c}'),
        'n' => plain('\n'),
        'r' => plain('\r'),
        't' => plain('\t'),
        'v' => plain('\u{b}'),
        'u' if chars.get(at + 1) == Some(&'{') => {
            let digits: String = chars[at + 2..]
                .iter()
                .take(7)
                .take_while(|d| d.is_ascii_hexdigit())
                .collect();
            let closed = chars.get(at + 2 + digits.len()) == Some(&'}');
            match u32::from_str_radix(&digits, 16)
                .ok()
                .and_then(char::from_u32)
            {
                Some(decoded) if closed => (decoded.to_string(), digits.len() + 3),
                _ => plain('u'),
            }
        }
        other => plain(other),
    }
}

/// `(Join-Path a b)` (also `-Path a -ChildPath b`) of plain words: the joined path and the index
/// of the closing paren; `(Get-Command git)` and `(gcm git)` give the command's name, which
/// `&` runs. Anything else (a nested command, other parameters) is `None`.
fn ps_join_path(chars: &[char], start: usize) -> Option<(String, usize)> {
    let end = closing_paren(chars, start, Dialect::PowerShell)?;
    let inner = text_of(chars, start, end);
    let lower = inner.trim_start().to_ascii_lowercase();
    // Only a flat call is lexed here, so no nesting can recurse.
    let known = ["join-path", "get-command", "gcm"]
        .iter()
        .any(|name| lower.starts_with(name));
    if !known || inner.contains(['(', ')', '{', '}', '|', ';', '&', '@']) {
        return None;
    }
    let words = lex_powershell(&inner);
    let [Piece::Command(simple)] = words.pieces.as_slice() else {
        return None;
    };
    if words.unclosed || !simple.redirects.is_empty() {
        return None;
    }
    let name = simple.words.first()?;
    if name.eq_ignore_ascii_case("get-command") || name.eq_ignore_ascii_case("gcm") {
        return match simple.words.get(1..)? {
            [command] if ps_param_rest(command).is_none() && !command.contains(UNKNOWN) => {
                Some((command.clone(), end))
            }
            _ => None,
        };
    }
    if !name.eq_ignore_ascii_case("join-path") {
        return None;
    }
    let (mut path, mut child) = (None, None);
    let mut args = simple.words.iter().skip(1);
    while let Some(arg) = args.next() {
        if is_ps_param(arg, "path", 2) {
            path = Some(args.next()?);
        } else if is_ps_param(arg, "childpath", 2) {
            child = Some(args.next()?);
        } else if ps_param_rest(arg).is_some() {
            return None;
        } else if path.is_none() {
            path = Some(arg);
        } else if child.is_none() {
            child = Some(arg);
        } else {
            return None;
        }
    }
    let (path, child) = (path?, child?);
    if path.contains(UNKNOWN) || child.contains(UNKNOWN) {
        return None;
    }
    Some((format!("{path}\\{child}"), end))
}

/// PowerShell's `$pwd` (variables ignore case) at the start of a word, as [`WORKING_FOLDER`].
fn ps_named_pwd(word: &str) -> String {
    for name in ["$pwd", "${pwd}"] {
        let Some(head) = word.get(..name.len()) else {
            continue;
        };
        let tail = &word[name.len()..];
        if head.eq_ignore_ascii_case(name) && (tail.is_empty() || tail.starts_with(['\\', '/'])) {
            return format!("{WORKING_FOLDER}{tail}");
        }
    }
    word.to_owned()
}

fn ps_substitution(w: &mut Words, chars: &[char], start: usize) -> usize {
    let end = closing_paren(chars, start, Dialect::PowerShell).unwrap_or_else(|| {
        w.unclosed = true;
        chars.len()
    });
    w.substitution(text_of(chars, start, end), Dialect::PowerShell, UNKNOWN);
    end
}

/// The index of the `}` closing a PowerShell `{` before `start`, nested braces counted and
/// quotes skipped; `None` when it never closes.
fn ps_closing_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '`' => i += 1,
            q if ps_single(q) => {
                i += 1;
                while i < chars.len() && !ps_single(chars[i]) {
                    i += 1;
                }
            }
            q if ps_double(q) => {
                i += 1;
                while i < chars.len() && !ps_double(chars[i]) {
                    if chars[i] == '`' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '{' => depth += 1,
            '}' => {
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

/// Whether a PowerShell word ends in a method that turns its string argument into a script.
fn creates_script(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    [
        "scriptblock]::create",
        ".invokescript",
        ".newscriptblock",
        ".expandstring",
    ]
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
                let (text, taken) = ps_escape(chars, i + 1);
                line_start = chars.get(i + 1) == Some(&'\n');
                w.word.push_str(&text);
                i += 1 + taken.max(1);
            }
            '$' if double && chars.get(i + 1) == Some(&'(') => {
                i = ps_subexpression(w, chars, i + 2) + 1;
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
            // The set of `for ... in (set) do`: one word; `for /f` runs a set in single quotes
            // (or backticks with `usebackq`) as a command.
            '(' if w
                .words
                .first()
                .is_some_and(|f| f.eq_ignore_ascii_case("for"))
                && (if w.in_word {
                    w.word.eq_ignore_ascii_case("in")
                } else {
                    w.words.last().is_some_and(|l| l.eq_ignore_ascii_case("in"))
                }) =>
            {
                w.end_word();
                match cmd_for_set(&chars, i + 1) {
                    Some((set, command, end)) => {
                        if let Some(command) = command {
                            w.pieces.push(Piece::Script(command, Dialect::Cmd));
                        }
                        w.push_str(&set);
                        w.end_word();
                        i = end;
                    }
                    None => {
                        w.unclosed = true;
                        i = chars.len();
                    }
                }
            }
            '(' | ')' if !w.in_word => {
                if c == '(' {
                    w.parens += 1;
                } else {
                    w.parens = w.parens.saturating_sub(1);
                }
                w.end_command();
            }
            // Inside a block, a `)` ends the word it is glued to and the block
            // (`if exist x (git push --force) else (echo)`).
            ')' if w.parens > 0 => {
                w.parens -= 1;
                w.end_command();
            }
            '<' | '>' => {
                w.redirect();
                while matches!(chars.get(i + 1), Some('>' | '&')) {
                    i += 1;
                }
            }
            '^' => {
                match next {
                    Some('\n') | None => {}
                    // The program parses its command line again, where the blank still parts
                    // two words (measured: `git^ version` runs `git version`).
                    Some(' ' | '\t') => w.end_word(),
                    Some(escaped) => w.push(escaped),
                }
                i += 1;
            }
            // `start "title" ...`: cmd's `start` takes its first quoted argument as the window
            // title, also one that looks like a program.
            '"' if !w.in_word
                && !w.titled
                && w.words.len() == 1
                && w.words[0].eq_ignore_ascii_case("start") =>
            {
                w.titled = true;
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += 1;
                }
                if i >= chars.len() {
                    w.unclosed = true;
                }
            }
            '"' => {
                // A quoted program name ends at its closing quote (`"git"push` runs git with
                // `push`), as cmd looks the program up.
                let program = !w.in_word && w.words.is_empty();
                w.start();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    w.word.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    w.unclosed = true;
                } else if program {
                    w.end_word();
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

/// The set of a cmd `for` from `start` (after its `(`): the text, the command it runs when it is
/// one in single quotes or backticks (`for /f`), and the index of the closing `)`. `None` when
/// it never closes.
fn cmd_for_set(chars: &[char], start: usize) -> Option<(String, Option<String>, usize)> {
    let mut i = start;
    while matches!(chars.get(i), Some(' ' | '\t')) {
        i += 1;
    }
    if let Some(&quote) = chars.get(i).filter(|q| matches!(q, '\'' | '`')) {
        let close = chars[i + 1..].iter().position(|c| *c == quote)? + i + 1;
        let mut end = close + 1;
        while matches!(chars.get(end), Some(' ' | '\t')) {
            end += 1;
        }
        if chars.get(end) != Some(&')') {
            return None;
        }
        return Some((
            text_of(chars, start, end),
            Some(text_of(chars, i + 1, close)),
            end,
        ));
    }
    let mut in_quotes = false;
    while let Some(&c) = chars.get(i) {
        match c {
            '"' => in_quotes = !in_quotes,
            ')' if !in_quotes => return Some((text_of(chars, start, i), None, i)),
            '\n' => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// Items of a cmd `for` set that are read as paths: the do-command, once per item, with the loop
/// variable placed. `for %d in (build dist) do rd /s /q %d` reads the variable as `build` and as
/// `dist`; `for /d /r <root> %d in (<name>) do ...` reads it as `<root>/<name>`, since a recursive
/// loop finds entries of that name below the root. `None` when the header cannot be read as such:
/// a `/f` set (command output) or `/l` set (numbers), or a missing `in`/`do`; the caller then runs
/// the do-command with the variable left in place (unplaceable, asked about).
fn cmd_for_commands(args: &[String]) -> Option<Vec<Vec<String>>> {
    let do_at = args.iter().position(|a| a.eq_ignore_ascii_case("do"))?;
    let in_at = args[..do_at]
        .iter()
        .position(|a| a.eq_ignore_ascii_case("in"))?;
    let header = &args[..in_at];
    let variable = header.last().filter(|v| v.starts_with('%'))?;
    let options: Vec<String> = header[..header.len() - 1]
        .iter()
        .filter(|w| w.starts_with('/'))
        .map(|w| w.to_ascii_lowercase())
        .collect();
    // `/f` runs a command for its items, `/l` counts a numeric range: neither names paths.
    if options.iter().any(|o| o == "/f" || o == "/l") {
        return None;
    }
    let recursive = options.iter().any(|o| o == "/r");
    // The recursion root is the one non-option word before the variable; without it `/r` starts
    // in the current folder.
    let root = header[..header.len() - 1]
        .iter()
        .find(|w| !w.starts_with('/'))
        .map(String::as_str);
    let set = args.get(in_at + 1).filter(|_| in_at + 1 < do_at)?;
    let do_command = &args[do_at + 1..];
    if do_command.is_empty() {
        return None;
    }
    // A wildcard set (`for %i in (*)`) matches unknown entries of the folder, so the variable
    // cannot be placed: left as written, it is asked about. So is a set with a quote, caret,
    // percent or any other character cmd may read specially (`("..\x")` is the item `..\x`).
    // cmd splits a set at spaces, tabs, commas, semicolons and equals signs.
    let plain = |c: char| {
        c.is_alphanumeric()
            || c.is_whitespace()
            || matches!(
                c,
                '.' | '_' | '-' | '\\' | '/' | ':' | '~' | ',' | ';' | '='
            )
    };
    if !set.chars().all(plain) {
        return None;
    }
    let mut commands = Vec::new();
    for name in set
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '='))
        .filter(|name| !name.is_empty())
        .take(MAX_FIND_STARTS)
    {
        let reading = if recursive {
            format!("{}/{name}", root.unwrap_or("."))
        } else {
            name.to_owned()
        };
        commands.push(
            do_command
                .iter()
                .map(|word| word.replace(variable.as_str(), &reading))
                .collect(),
        );
    }
    (!commands.is_empty()).then_some(commands)
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
    /// The cmd script being split runs with delayed expansion.
    delayed: bool,
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
        self.pieces(words, dialect, depth);
    }

    /// The inside of a POSIX arithmetic context, read as commands without here-documents.
    fn arithmetic(&mut self, text: &str, depth: usize) {
        if depth > MAX_DEPTH {
            self.analysis.truncated = true;
            return;
        }
        self.pieces(lex_posix_in(text, true), Dialect::Posix, depth);
    }

    /// Marks that the command runs an inline script in the current shell (`sh -c`, `pwsh
    /// -Command`, `cmd /c`, `eval`, a sourced file), where a variable could be written where the
    /// loop reader does not see it, so no loop variable is placed in its body.
    fn inline_script(&mut self, text: &str, dialect: Dialect, depth: usize) {
        self.analysis.has_inline_script = true;
        self.script(text, dialect, depth);
    }

    fn inline_scripts(&mut self, texts: &[String], dialect: Dialect, depth: usize) {
        self.analysis.has_inline_script = true;
        self.scripts(texts, dialect, depth);
    }

    fn pieces(&mut self, words: Words, dialect: Dialect, depth: usize) {
        if words.unclosed {
            self.analysis.unclosed = true;
        }
        // The call of the last command at this level, which a pipe hands its output on from.
        let mut last: Option<usize> = None;
        // The last call each piece made, for the script blocks of a `ForEach-Object`.
        let mut call_of: Vec<Option<usize>> = Vec::with_capacity(words.pieces.len());
        for piece in words.pieces {
            match piece {
                Piece::Command(simple) => {
                    let at = self.analysis.calls.len();
                    collect_writes(&simple.words, dialect, &mut self.analysis.writes);
                    self.analysis
                        .redirects
                        .extend(simple.redirects.into_iter().map(|target| (at, target)));
                    let for_each = simple
                        .item_of
                        .and_then(|piece| call_of.get(piece).copied().flatten());
                    let meta = CallInfo {
                        dialect,
                        piped: simple.piped,
                        upstream: if simple.piped { last } else { None },
                        fed: simple.fed,
                        braces: simple.braces,
                        per_item: simple.item_of.is_some(),
                        items: for_each
                            .and_then(|call| self.analysis.info.get(call))
                            .and_then(|info| info.upstream),
                        delayed: dialect == Dialect::Cmd && self.delayed,
                    };
                    let words = if dialect == Dialect::PowerShell {
                        simple.words.iter().map(|word| ps_named_pwd(word)).collect()
                    } else {
                        simple.words
                    };
                    self.command(&words, dialect, depth, &meta);
                    let now = self.analysis.calls.len();
                    last = (now > at).then(|| now - 1);
                    call_of.push(last);
                }
                Piece::Script(inner, dialect) => {
                    self.script(&inner, dialect, depth + 1);
                    call_of.push(None);
                }
                Piece::Arithmetic(inner) => {
                    self.arithmetic(&inner, depth + 1);
                    call_of.push(None);
                }
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

    /// A launcher's inner command, read at every plausible start: option parsing for the many
    /// launcher flags cannot be known in full, so the command is read starting at each word that is
    /// no option, bounded by [`MAX_LAUNCH_OFFSETS`]. A lone interactive shell (`docker run -it img
    /// sh`) reads the container's terminal, not a hidden script, unless the launcher is fed by a
    /// pipe (`echo ... | docker exec -i c sh`), which it then reads as a script from stdin.
    fn launch(&mut self, tail: &[String], dialect: Dialect, depth: usize, meta: &CallInfo) {
        let mut read = 0;
        for (index, word) in tail.iter().enumerate() {
            if read >= MAX_LAUNCH_OFFSETS {
                break;
            }
            // A command starts with a program name, never with an option or a `--` separator.
            if word.starts_with('-') {
                continue;
            }
            let candidate = &tail[index..];
            if bare_interactive_shell(candidate) {
                if meta.piped {
                    self.analysis.stdin_script = true;
                }
                read += 1;
                continue;
            }
            self.command(candidate, dialect, depth + 1, meta);
            read += 1;
        }
    }

    /// Several readings of one script (see [`scripts_of`]), each split on its own.
    fn scripts(&mut self, texts: &[String], dialect: Dialect, depth: usize) {
        for text in texts {
            self.script(text, dialect, depth);
        }
    }

    /// `NAME=value` in front of a POSIX program: a variable a later program runs as a command
    /// (`GIT_SSH_COMMAND`, `PROMPT_COMMAND`, ...) has its value read as a script; a startup file
    /// a shell reads from a process (`BASH_ENV=<(...)`) is a script from stdin.
    fn assignment(&mut self, word: &str, depth: usize) {
        let Some((name, value)) = word.split_once('=') else {
            return;
        };
        if STARTUP_FILE_VARIABLES.contains(&name) {
            if value == PROCESS_FILE || is_stdin_path(value) {
                self.analysis.stdin_script = true;
            }
        } else if COMMAND_VARIABLES.contains(&name) && !value.trim().is_empty() {
            self.script(value, Dialect::Posix, depth + 1);
        }
    }

    /// The substitutions in a word a bash builtin evaluates (an array subscript in `let`,
    /// `declare`, `test -v`, `read`, ...), quoted or not, split as scripts of their own.
    fn evaluated(&mut self, text: &str, depth: usize) {
        if !text.contains("$(") && !text.contains('`') {
            return;
        }
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\\' => i += 1,
                '$' if chars.get(i + 1) == Some(&'(') => {
                    let Some(end) = closing_paren(&chars, i + 2, Dialect::Posix) else {
                        self.analysis.unclosed = true;
                        return;
                    };
                    let inner = text_of(&chars, i + 2, end);
                    if inner.starts_with('(') {
                        self.arithmetic(&inner, depth + 1);
                    } else {
                        self.script(&inner, Dialect::Posix, depth + 1);
                    }
                    i = end;
                }
                '`' => {
                    let Some(end) = closing_backtick(&chars, i + 1) else {
                        self.analysis.unclosed = true;
                        return;
                    };
                    self.script(&text_of(&chars, i + 1, end), Dialect::Posix, depth + 1);
                    i = end;
                }
                _ => {}
            }
            i += 1;
        }
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
            // Only POSIX shells take `NAME=value` in front of a program; cmd reads `cd=x` as
            // `cd x`.
            while let Some(word) = rest
                .first()
                .filter(|word| dialect != Dialect::Cmd && is_assignment(word))
            {
                if dialect == Dialect::Posix {
                    self.assignment(word, depth);
                }
                rest = &rest[1..];
            }
            // `$x = git push`, `return git push`: PowerShell runs what follows.
            if dialect == Dialect::PowerShell {
                if let Some(after) = ps_after_assignments(rest) {
                    self.command(&after, dialect, depth, meta);
                    return;
                }
            }
            let Some(first) = rest.first() else {
                return;
            };
            // cmd's `@` in front of a command turns off its echo and is no part of the program
            // name. It can lead any command, also one after `do`, `if <condition>` or `else`,
            // where the lexer leaves it glued to the first word (`do @rd`, `else @git`).
            if dialect == Dialect::Cmd {
                if let Some(stripped) = first.strip_prefix('@') {
                    let mut without = Vec::with_capacity(rest.len());
                    if !stripped.is_empty() {
                        without.push(stripped.to_owned());
                    }
                    without.extend(rest[1..].iter().cloned());
                    if without.is_empty() {
                        return;
                    }
                    self.command(&without, dialect, depth, meta);
                    return;
                }
            }
            // A program word the shell reads as more than one word (`rd/s`, `cd..`, `=git`).
            if let Some(mut respelled) = respelled(first, dialect) {
                respelled.extend(rest[1..].iter().cloned());
                self.command(&respelled, dialect, depth, meta);
                return;
            }
            let program = crate::command_guard::program_name(first);
            let args = &rest[1..];
            match program.as_str() {
                // A sourced file is a script, not a wrapper; from stdin or a process its text is
                // not on the command line. PowerShell's `.` runs what follows.
                "." | "source" if dialect == Dialect::Posix => {
                    if args.first().is_some_and(|file| is_stdin_path(file)) {
                        self.analysis.stdin_script = true;
                    }
                    // A sourced file runs in the current shell and may write a loop variable where
                    // the reader does not see it.
                    self.analysis.has_inline_script = true;
                    self.push(rest, meta);
                    return;
                }
                // cmd's `if [/i] [not] <condition> command`: the condition is skipped.
                "if" if dialect == Dialect::Cmd => rest = after_cmd_condition(args),
                // cmd's `for ... in (set) do command`.
                "for" if dialect == Dialect::Cmd => {
                    // When the set is a list of literal paths, the loop variable reads as each of
                    // them (`for %d in (.git) do rd /s /q "%d"` deletes `.git`), so the do-command
                    // is checked once per item with the variable placed. `/f` and `/l` sets are
                    // not paths and keep the variable (unplaceable, asked about).
                    if let Some(commands) = cmd_for_commands(args) {
                        for command in commands {
                            self.command(&command, dialect, depth + 1, meta);
                        }
                        return;
                    }
                    match args.iter().position(|a| a.eq_ignore_ascii_case("do")) {
                        Some(at) => rest = &args[at + 1..],
                        None => {
                            self.push(rest, meta);
                            return;
                        }
                    }
                }
                // bash builtins that evaluate an array subscript or arithmetic in their
                // arguments, also from a quoted word (`let 'a[$(cmd)]'`, `test -v 'a[$(cmd)]'`);
                // the declaring ones only in the name before `=`.
                "let" | "test" | "[" | "[[" | "unset" | "read" | "mapfile" | "readarray"
                | "printf" | "declare" | "typeset" | "local" | "export" | "readonly"
                    if dialect == Dialect::Posix =>
                {
                    // A nameref (`declare -n r='a[$(cmd)]'`) evaluates its value when used.
                    let nameref = args.iter().any(|arg| {
                        arg.strip_prefix('-')
                            .is_some_and(|group| !group.starts_with('-') && group.contains('n'))
                    });
                    let declaring = !nameref
                        && matches!(
                            program.as_str(),
                            "declare" | "typeset" | "local" | "export" | "readonly"
                        );
                    if program != "printf" || args.iter().any(|arg| arg.starts_with("-v")) {
                        for arg in args {
                            let evaluated = if declaring {
                                arg.split_once('=').map_or(arg.as_str(), |(name, _)| name)
                            } else {
                                arg
                            };
                            self.evaluated(evaluated, depth);
                        }
                    }
                    self.push(rest, meta);
                    return;
                }
                "{" | "}" | "!" | "then" | "do" | "else" | "elif" | "if" | "while" | "until"
                | "builtin" | "&" | "." | "call" | "noglob" | "busybox" => rest = args,
                // zsh's prefixes `coproc`, `nocorrect` and `-`, and programs that only run the
                // rest (`unbuffer`, `chronic`).
                "coproc" | "nocorrect" | "-" if dialect == Dialect::Posix => rest = args,
                "unbuffer" | "chronic" => rest = args,
                "repeat" if dialect == Dialect::Posix => rest = args.get(1..).unwrap_or_default(),
                "command" | "nohup" => rest = skip_options_with(args, "", &[]),
                "exec" => rest = skip_options_with(args, "a", &[]),
                "time" => rest = skip_options_with(args, "fo", &["--format", "--output"]),
                "winpty" => rest = skip_options_with(args, "", &["--title"]),
                "setsid" => rest = skip_options_with(args, "", &[]),
                "stdbuf" => {
                    rest = skip_options_with(args, "ioe", &["--input", "--output", "--error"])
                }
                "ionice" => rest = skip_options_with(args, "cnpPu", &["--class", "--classdata"]),
                "caffeinate" => rest = skip_options_with(args, "tw", &[]),
                "arch" => rest = skip_options(args, &["-arch", "-e", "-d"]),
                "xcrun" => {
                    rest = skip_options(args, &["--sdk", "-sdk", "--toolchain", "-toolchain"])
                }
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
                    let (calls, all_starts) = find_commands(args);
                    if !all_starts {
                        self.analysis.truncated = true;
                    }
                    for call in calls {
                        self.command(&call, dialect, depth + 1, &inline);
                    }
                    return;
                }
                "sudo" | "doas" => {
                    rest = skip_options_with(
                        args,
                        "ugCDhprtUTac",
                        &[
                            "--user",
                            "--group",
                            "--close-from",
                            "--chdir",
                            "--host",
                            "--prompt",
                            "--role",
                            "--type",
                            "--other-user",
                            "--command-timeout",
                            "--auth-type",
                            "--login-class",
                        ],
                    )
                }
                // Privilege and sandbox wrappers that run a command after their own options
                // (`gsudo -u x git ...`, `strace -o log git ...`, `xvfb-run git ...`, `npx git ...`,
                // `bunx git ...`). The option set is open, so the command is read at every offset.
                "gsudo" | "strace" | "ltrace" | "xvfb-run" | "proot" | "npx" | "bunx" | "uvx"
                | "pipx" | "systemd-run" => {
                    self.launch(args, dialect, depth, meta);
                    return;
                }
                // `dotenv [-e file] -- <command>` and `dotenvx run -- <command>`.
                "dotenv" | "dotenvx" => {
                    rest = match args.iter().position(|w| w == "--") {
                        Some(at) => &args[at + 1..],
                        None => skip_options_with(args, "ef", &["--env", "--file"]),
                    };
                }
                // `nix-shell --run <script>`, `nix develop -c <command>`, `nix shell ... -c <cmd>`.
                "nix" | "nix-shell" | "nix-build" => {
                    if let Some(script) = nix_script(args) {
                        self.script(&script, Dialect::Posix, depth + 1);
                    } else {
                        self.push(rest, meta);
                    }
                    return;
                }
                // `cross-env NAME=value ... <command>`: the assignments are skipped, then the
                // command is read at every offset.
                "cross-env" | "env-cmd" => {
                    let mut tail = args;
                    while tail.first().is_some_and(|word| is_assignment(word)) {
                        tail = &tail[1..];
                    }
                    self.launch(tail, dialect, depth, meta);
                    return;
                }
                // Wrappers whose command follows a `run`/`exec` subcommand (`uv run`, `poetry run`,
                // `pnpm exec`, `conda run`, `direnv exec`, `mise exec`, `op run`, `aws-vault exec`),
                // possibly after global options (`uv --directory x run`, `pnpm -C x exec`). The
                // command after the subcommand is read at every offset.
                "uv" | "poetry" | "pipenv" | "pdm" | "rye" | "hatch" | "pnpm" | "yarn" | "npm"
                | "bun" | "conda" | "micromamba" | "mamba" | "direnv" | "mise" | "rtx" | "op"
                | "doppler" | "infisical" | "aws-vault" | "devbox" => {
                    match runner_tail(&program, args) {
                        Some(tail) => self.launch(tail, dialect, depth, meta),
                        None => self.push(rest, meta),
                    }
                    return;
                }
                // `su [-] [user] (-c <script> | --command=<script>) ` and `runuser ... -- <cmd>`:
                // the script runs in a login shell (an inline script), the words after `--` run
                // directly.
                "su" | "runuser" => {
                    match su_command(args) {
                        SuCommand::Script(script) => {
                            self.inline_script(&script, Dialect::Posix, depth + 1)
                        }
                        SuCommand::Direct(after) => self.launch(after, dialect, depth, meta),
                        SuCommand::None => self.push(rest, meta),
                    }
                    return;
                }
                // `docker|podman|nerdctl [globals] exec|run|compose ... <command...>`: the command
                // after the container, service or image is read at every offset.
                "docker" | "podman" | "nerdctl" | "docker-compose" | "podman-compose" => {
                    match container_tail(&program, args) {
                        Some(tail) => self.launch(tail, dialect, depth, meta),
                        None => self.push(rest, meta),
                    }
                    return;
                }
                // `kubectl|oc [globals] exec [options] POD [-- ] <command...>`.
                "kubectl" | "oc" | "k3s" => {
                    match kubectl_tail(args) {
                        Some(tail) => self.launch(tail, dialect, depth, meta),
                        None => self.push(rest, meta),
                    }
                    return;
                }
                "env" => {
                    if let Some(script) = env_split_string(args) {
                        match script {
                            Some(script) => self.script(&script, Dialect::Posix, depth + 1),
                            None => self.analysis.stdin_script = true,
                        }
                        return;
                    }
                    rest = skip_options_with(args, "uC", &["--unset", "--chdir"]);
                }
                "nice" => rest = skip_options_with(args, "n", &["--adjustment"]),
                "timeout" | "gtimeout" => {
                    let after = skip_options_with(args, "sk", &["--signal", "--kill-after"]);
                    rest = after.get(1..).unwrap_or_default();
                }
                // `watch` hands its words to `sh -c` (with `-x` it runs them as they are, which
                // the same reading covers).
                "watch" => {
                    let after = skip_options_with(args, "nq", &["--interval", "--equexit"]);
                    if after.is_empty() {
                        self.push(rest, meta);
                    } else {
                        let scripts = scripts_of(after, Dialect::Posix);
                        self.scripts(&scripts, Dialect::Posix, depth + 1);
                    }
                    return;
                }
                // `flock [options] <file> <command...>` or `flock <file> -c <command line>`.
                "flock" => {
                    let after = skip_options_with(
                        args,
                        "wE",
                        &["--timeout", "--wait", "--conflict-exit-code"],
                    );
                    match after.get(1).map(String::as_str) {
                        Some("-c" | "--command") => {
                            if let Some(script) = after.get(2) {
                                self.script(script, Dialect::Posix, depth + 1);
                            }
                            return;
                        }
                        Some(_) => rest = &after[1..],
                        None => {
                            self.push(rest, meta);
                            return;
                        }
                    }
                }
                // `parallel <command> ::: <values>`: each value becomes an argument, so the
                // values are read as the command's arguments too.
                "parallel" => {
                    let after = skip_options(args, &["-j", "--jobs", "-S", "--sshlogin"]);
                    let call: Vec<String> = after
                        .iter()
                        .filter(|word| !word.starts_with(":::"))
                        .cloned()
                        .collect();
                    self.command(&call, dialect, depth + 1, meta);
                    return;
                }
                // `script -c <command line>` runs it in a new shell.
                "script" => {
                    match script_command(args) {
                        Some(script) => self.script(script, Dialect::Posix, depth + 1),
                        None => self.push(rest, meta),
                    }
                    return;
                }
                // `ssh host <command...>`: the remote shell runs the words joined.
                "ssh" => {
                    let after = skip_options_with(args, "BbcDEeFIiJLlmOoPpQRSWw", &[]);
                    if after.len() >= 2 {
                        let scripts = scripts_of(&after[1..], Dialect::Posix);
                        self.scripts(&scripts, Dialect::Posix, depth + 1);
                    } else {
                        self.push(rest, meta);
                    }
                    return;
                }
                "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" | "mksh" | "fish" => {
                    match shell_script(args) {
                        ShellScript::Inline(script, after) => {
                            // From cmd a quote is no quote: `sh -c 'a && b'` gives sh the
                            // unclosed `'a`, which sh rejects whole, so nothing of it runs.
                            let rejected = dialect == Dialect::Cmd
                                && !script.contains('\n')
                                && !script.contains("<<")
                                && lex_posix(script).unclosed;
                            if !rejected {
                                self.inline_script(script, Dialect::Posix, depth + 1);
                                match posix_positional(script, after) {
                                    Positional::Same => {}
                                    Positional::Read(text) => {
                                        self.script(&text, Dialect::Posix, depth + 1)
                                    }
                                    Positional::Unreadable => self.analysis.truncated = true,
                                }
                            }
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
                        ShellScriptOwned::Inline(scripts) => {
                            self.inline_scripts(&scripts, Dialect::PowerShell, depth + 1)
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
                    match cmd_script(args) {
                        Some((scripts, delayed)) => {
                            let outer = std::mem::replace(&mut self.delayed, delayed);
                            self.inline_scripts(&scripts, Dialect::Cmd, depth + 1);
                            self.delayed = outer;
                        }
                        None => {
                            self.analysis.stdin_script = true;
                            self.push(rest, meta);
                        }
                    }
                    return;
                }
                "eval" => {
                    self.inline_scripts(&scripts_of(args, dialect), dialect, depth + 1);
                    return;
                }
                "iex" | "invoke-expression" => {
                    let scripts = match args.first() {
                        Some(name) if is_ps_param(name, "command", 1) => {
                            match ps_colon_value(name) {
                                Some(value) => {
                                    let mut words = vec![value.to_owned()];
                                    words.extend(args[1..].iter().cloned());
                                    scripts_of(&words, Dialect::PowerShell)
                                }
                                None => scripts_of(&args[1..], Dialect::PowerShell),
                            }
                        }
                        _ => scripts_of(args, Dialect::PowerShell),
                    };
                    // `... | iex` runs what the pipe brings: not on the command line.
                    if scripts.iter().all(|script| script.trim().is_empty()) && meta.piped {
                        self.analysis.stdin_script = true;
                        self.push(rest, meta);
                        return;
                    }
                    self.inline_scripts(&scripts, Dialect::PowerShell, depth + 1);
                    return;
                }
                "wsl" => {
                    // `wsl ~` starts in the home folder; what follows is read as before.
                    let args = match args.first() {
                        Some(home) if home == "~" => &args[1..],
                        _ => args,
                    };
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
                            let scripts = scripts_of(&after[1..], Dialect::Posix);
                            self.scripts(&scripts, Dialect::Posix, depth + 1);
                            return;
                        }
                        Some(_) => {
                            let scripts = scripts_of(after, Dialect::Posix);
                            self.scripts(&scripts, Dialect::Posix, depth + 1);
                            return;
                        }
                        None => {
                            self.analysis.stdin_script = true;
                            self.push(rest, meta);
                            return;
                        }
                    }
                }
                // cmd knows only its own `start`; `Start-Process` there is an unknown program.
                "start-process" | "saps" if dialect == Dialect::Cmd => {
                    self.push(rest, meta);
                    return;
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

/// Variables whose value a program runs as a shell command line (git's, the pager's, the
/// editor's, bash's prompt hook).
const COMMAND_VARIABLES: &[&str] = &[
    "GIT_SSH_COMMAND",
    "GIT_SSH",
    "GIT_PAGER",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_EXTERNAL_DIFF",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "PAGER",
    "EDITOR",
    "VISUAL",
    "PROMPT_COMMAND",
];

/// Variables naming a file a starting shell runs as a script.
const STARTUP_FILE_VARIABLES: &[&str] = &["BASH_ENV", "ENV"];

/// Assignment operators of PowerShell.
const PS_ASSIGNMENTS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "??="];

/// The words after PowerShell assignments in front of a pipeline (`$x = git ...`,
/// `[string]$a, $b += ...`, `$x = $y = ...`, `$x=git`) and after `return` or `throw`, whose
/// value is a pipeline too; `None` when there is none.
fn ps_after_assignments(words: &[String]) -> Option<Vec<String>> {
    let keyword =
        |word: &str| word.eq_ignore_ascii_case("return") || word.eq_ignore_ascii_case("throw");
    let first = words.first()?;
    if !first.starts_with(['$', '[']) && !keyword(first) {
        return None;
    }
    let mut words = words.to_vec();
    let mut at = 0;
    let mut changed = false;
    loop {
        if words.get(at).is_some_and(|word| keyword(word)) {
            at += 1;
            changed = true;
            continue;
        }
        // Targets up to an operator, separated by commas.
        let mut j = at;
        let mut found: Option<(usize, Option<String>)> = None;
        while let Some(word) = words.get(j) {
            if word == "," {
                j += 1;
                continue;
            }
            if PS_ASSIGNMENTS.contains(&word.as_str()) {
                if j > at {
                    found = Some((j + 1, None));
                }
                break;
            }
            if let Some((target, value)) = ps_glued_assignment(word) {
                if ps_lvalue(target.trim_end_matches(',')) {
                    found = Some((j + 1, (!value.is_empty()).then(|| value.to_owned())));
                }
                break;
            }
            if !ps_lvalue(word.trim_end_matches(',')) {
                break;
            }
            j += 1;
        }
        let Some((next, value)) = found else {
            break;
        };
        changed = true;
        at = next;
        if let Some(value) = value {
            // `$x=git`: the value starts the next part.
            at -= 1;
            words[at] = value;
        }
    }
    changed.then(|| words.split_off(at.min(words.len())))
}

/// `$x=value`, `$x+=value`: the target and what follows the operator.
fn ps_glued_assignment(word: &str) -> Option<(&str, &str)> {
    let at = word.find('=')?;
    let before = &word[..at];
    let target = ["??", "+", "-", "*", "/", "%"]
        .iter()
        .find_map(|op| before.strip_suffix(op))
        .unwrap_or(before);
    (!target.is_empty()).then(|| (target, &word[at + 1..]))
}

/// A PowerShell assignment target: a variable with optional type casts in front and member or
/// index access after it (`[string]$x`, `$x.y`, `$x[0]`, `${x}`, `$env:X`).
fn ps_lvalue(word: &str) -> bool {
    let mut rest = word;
    while let Some(cast) = rest.strip_prefix('[') {
        let mut depth = 1usize;
        let mut end = None;
        for (at, c) in cast.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(at);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            return false;
        };
        rest = &cast[end + 1..];
    }
    rest.len() >= 2 && rest.starts_with('$') && !rest.contains(['=', '(', ')']) && rest != UNKNOWN
}

/// cmd built-ins a switch or folder may be glued to (`rd/s`, `cmd/c`, `cd..`); longer names
/// first, so `cmd.exe` is not read as `cmd`.
const CMD_GLUED: &[&str] = &[
    "cmd.exe",
    "cmd",
    "chdir",
    "cd",
    "rmdir",
    "rd",
    "del",
    "erase",
    "start",
    "pushd",
    "mkdir",
    "md",
    "format.com",
    "format",
];

/// A program word the shell reads as more than one word: zsh's `=git` (the program's path),
/// cmd's switch or folder glued to a built-in (`rd/s/q`, `cmd/c`, `cd..`, `cd\`, `cd=x`).
fn respelled(first: &str, dialect: Dialect) -> Option<Vec<String>> {
    match dialect {
        Dialect::Posix => {
            let name = first.trim_start_matches('=');
            (name.len() != first.len() && !name.is_empty()).then(|| vec![name.to_owned()])
        }
        Dialect::Cmd => {
            let lower = first.to_ascii_lowercase();
            for name in CMD_GLUED {
                let Some(tail) = lower.strip_prefix(name) else {
                    continue;
                };
                let folder = matches!(*name, "cd" | "chdir");
                if tail.starts_with(['/', '\\']) || (folder && tail.starts_with(['.', '='])) {
                    let rest = first[name.len()..].trim_start_matches('=');
                    let mut out = vec![first[..name.len()].to_owned()];
                    if !rest.is_empty() {
                        out.push(rest.to_owned());
                    }
                    return Some(out);
                }
            }
            None
        }
        Dialect::PowerShell => None,
    }
}

/// What `cmd` runs: the words after `/c`, `/k` or `/r` (also `//c` as Git Bash passes it, in a
/// group `/q/c`, or glued `/cdir`), as [`scripts_of`] reads them, and whether `/v` or `/v:on`
/// turns delayed expansion on. `None` without one: cmd reads its commands from stdin.
fn cmd_script(args: &[String]) -> Option<(Vec<String>, bool)> {
    let mut delayed = false;
    for (at, arg) in args.iter().enumerate() {
        if !arg.starts_with('/') {
            continue;
        }
        let mut offset = 0;
        for part in arg.split('/') {
            let start = offset;
            offset += part.len() + 1;
            let lower = part.to_ascii_lowercase();
            if lower == "v" || lower.starts_with("v:on") {
                delayed = true;
            }
            if lower.starts_with(['c', 'k', 'r']) {
                let glued = arg.get(start + 1..).unwrap_or_default();
                let mut words = Vec::new();
                if !glued.is_empty() {
                    words.push(glued.to_owned());
                }
                words.extend(args[at + 1..].iter().cloned());
                return Some((scripts_of(&words, Dialect::Cmd), delayed));
            }
        }
    }
    None
}

/// The command line of `script -c <line>` (also `-qc <line>`, `--command <line>`,
/// `--command=<line>`); `None` without one.
fn script_command(args: &[String]) -> Option<&str> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" || !arg.starts_with('-') {
            return None;
        }
        if arg == "--command" {
            return args.get(i + 1).map(String::as_str);
        }
        if let Some(value) = arg.strip_prefix("--command=") {
            return Some(value);
        }
        if !arg.starts_with("--") {
            let group = &arg[1..];
            if let Some(at) = group.find('c') {
                let tail = &group[at + 1..];
                return if tail.is_empty() {
                    args.get(i + 1).map(String::as_str)
                } else {
                    Some(tail)
                };
            }
            // Short options that take a value.
            if group.ends_with(['E', 'o', 'm', 'I', 'O', 'B', 'T']) {
                i += 1;
            }
        } else if matches!(
            arg.as_str(),
            "--echo"
                | "--output-limit"
                | "--logging-format"
                | "--log-in"
                | "--log-out"
                | "--log-io"
                | "--log-timing"
        ) {
            i += 1;
        }
        i += 1;
    }
    None
}

/// A lone interactive shell (`sh`, `bash -l`, `pwsh`) with no inline script: run in a container it
/// reads that container's own terminal, not a script Hangar cannot see, so unwrapping it must not
/// report a stdin script.
fn bare_interactive_shell(call: &[String]) -> bool {
    let Some(first) = call.first() else {
        return true;
    };
    let program = crate::command_guard::program_name(first);
    if !matches!(
        program.as_str(),
        "sh" | "bash"
            | "zsh"
            | "dash"
            | "ksh"
            | "ash"
            | "mksh"
            | "fish"
            | "pwsh"
            | "powershell"
            | "cmd"
    ) {
        return false;
    }
    // Only a shell with nothing but these start-up switches is bare; any other argument may
    // carry a script (`bash -lc`, `pwsh -c`, `sh script.sh`), so the call is unwrapped and read.
    call[1..].iter().all(|arg| {
        matches!(
            arg.to_ascii_lowercase().as_str(),
            "-l" | "-i"
                | "-il"
                | "-li"
                | "--login"
                | "--norc"
                | "--noprofile"
                | "-nologo"
                | "-noprofile"
                | "-noexit"
                | "/q"
        )
    })
}

/// Global options of a program runner before its subcommand that take the next word as a value
/// (`uv --directory x run`, `pnpm -C x exec`, `poetry -C x run`, `conda --name x run`).
fn skip_runner_globals(args: &[String]) -> &[String] {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-C" | "--directory" | "--project" | "--cwd" | "--config" | "-c" | "--name" | "-n"
            | "--prefix" | "-p" => i += 2,
            _ if arg.starts_with('-') => i += 1,
            _ => break,
        }
    }
    args.get(i..).unwrap_or_default()
}

/// The words after a program runner's `run`/`exec` subcommand, where its inner command starts
/// (read at every offset by [`Splitter::launch`]); global options before the subcommand are
/// skipped. `None` when the subcommand runs no arbitrary command (`npm install`, `uv sync`).
fn runner_tail<'a>(program: &str, args: &'a [String]) -> Option<&'a [String]> {
    let args = skip_runner_globals(args);
    let sub = args.first().map(String::as_str);
    let runs = matches!((program, sub), (_, Some("run" | "exec" | "dlx" | "x")))
        || matches!(
            (
                program,
                args.first().map(String::as_str),
                args.get(1).map(String::as_str)
            ),
            ("uv", Some("tool"), Some("run"))
        );
    if !runs {
        return None;
    }
    let after = if sub == Some("tool") {
        args.get(2..)
    } else {
        args.get(1..)
    };
    after.filter(|rest| !rest.is_empty())
}

/// The command of `su`/`runuser`.
enum SuCommand<'a> {
    /// `-c <script>`, `-c<script>`, `--command=<script>`: a login shell runs it.
    Script(String),
    /// `-- <command...>`: run directly.
    Direct(&'a [String]),
    /// No command found: run nothing further.
    None,
}

/// `su [-] [user] (-c <script> | -c<script> | --command=<script>)` or `runuser [-u user] [-]
/// [user] (-c <script> | -- <command>)`: the script a login shell runs, or the words after `--`.
fn su_command(args: &[String]) -> SuCommand<'_> {
    if let Some(at) = args.iter().position(|w| w == "--") {
        let after = &args[at + 1..];
        if !after.is_empty() {
            return SuCommand::Direct(after);
        }
    }
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "-c" || arg == "--command" {
            return match args.get(i + 1) {
                Some(script) => SuCommand::Script(script.clone()),
                None => SuCommand::None,
            };
        }
        if let Some(script) = arg.strip_prefix("--command=") {
            return SuCommand::Script(script.to_owned());
        }
        if let Some(script) = arg.strip_prefix("-c").filter(|s| !s.is_empty()) {
            return SuCommand::Script(script.to_owned());
        }
        // Options that take the next word as their value and so do not start the command.
        if matches!(
            arg.as_str(),
            "-u" | "--user" | "-g" | "--group" | "-s" | "--shell" | "-w"
        ) {
            i += 2;
            continue;
        }
        i += 1;
    }
    SuCommand::None
}

/// Global options of docker/podman before the subcommand that take a value (`docker -H host
/// exec`, `docker --context x exec`), skipped so the subcommand is found.
fn skip_docker_globals(args: &[String]) -> &[String] {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-H" | "--host" | "--context" | "-c" | "--config" | "-l" | "--log-level"
            | "--tlscacert" | "--tlscert" | "--tlskey" => i += 2,
            _ if arg.starts_with('-') => i += 1,
            _ => break,
        }
    }
    args.get(i..).unwrap_or_default()
}

/// The words after a container command's `exec`/`run`/`create` subcommand (where the inner
/// command starts, read at every offset): `docker|podman [globals] [container] exec|run ...`,
/// `... compose [globals] exec|run ...`, and `docker-compose`/`podman-compose`. `None` for any
/// other subcommand (`docker build`, `docker ps`).
fn container_tail<'a>(program: &str, args: &'a [String]) -> Option<&'a [String]> {
    if program.ends_with("compose") {
        return compose_tail(args);
    }
    let args = skip_docker_globals(args);
    // `docker container exec ...`, `docker image ...`: the object word before the verb.
    let args = match args.first().map(String::as_str) {
        Some("container" | "image" | "service") => args.get(1..).unwrap_or_default(),
        _ => args,
    };
    match args.first().map(String::as_str) {
        Some("compose") => compose_tail(args.get(1..).unwrap_or_default()),
        Some("exec" | "run" | "create") => args.get(1..).filter(|rest| !rest.is_empty()),
        _ => None,
    }
}

/// `compose [globals] exec|run ... <command...>`: the words after the subcommand.
fn compose_tail(args: &[String]) -> Option<&[String]> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-f"
            | "--file"
            | "-p"
            | "--project-name"
            | "--project-directory"
            | "--profile"
            | "--env-file" => i += 2,
            _ if arg.starts_with('-') => i += 1,
            _ => break,
        }
    }
    let args = args.get(i..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("exec" | "run") => args.get(1..).filter(|rest| !rest.is_empty()),
        _ => None,
    }
}

/// `kubectl|oc [globals] exec [opts] POD [-- ] <command...>`: the words where the inner command
/// starts (after `--` when present), read at every offset.
fn kubectl_tail(args: &[String]) -> Option<&[String]> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "-n" | "--namespace" | "--context" | "-s" | "--server" | "--kubeconfig" | "--as"
            | "--cluster" => i += 2,
            _ if arg.starts_with('-') => i += 1,
            _ => break,
        }
    }
    let args = args.get(i..).unwrap_or_default();
    if args.first().map(String::as_str) != Some("exec") {
        return None;
    }
    let args = args.get(1..).unwrap_or_default();
    if let Some(at) = args.iter().position(|w| w == "--") {
        return args.get(at + 1..).filter(|rest| !rest.is_empty());
    }
    Some(args).filter(|rest| !rest.is_empty())
}

/// The script of `nix-shell --run <script>` or `nix develop|shell ... -c <command...>`; `None`
/// without one.
fn nix_script(args: &[String]) -> Option<String> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--run" => return args.get(i + 1).cloned(),
            "-c" | "--command" => {
                let rest = args.get(i + 1..).unwrap_or_default();
                // A single quoted word is the whole script; several words are a command line.
                return (!rest.is_empty()).then(|| rest.join(" "));
            }
            _ => i += 1,
        }
    }
    None
}

/// How an inline script reads the words after it.
enum Positional {
    /// It names none of them, or none follow.
    Same,
    /// The script with `$1`, `"$@"`, `$args[0]`, ... replaced by those words.
    Read(String),
    /// It reads them in a way Hangar does not follow (`shift`, `${@:2}`).
    Unreadable,
}

/// `bash -c '<script>' <$0> <$1> ...`: the script with its positional parameters replaced by
/// the words after it, an extra reading of the call.
fn posix_positional(script: &str, args: &[String]) -> Positional {
    if args.is_empty() || !script.contains('$') {
        return Positional::Same;
    }
    let names_shift = script
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == "shift");
    if script.contains("${@:")
        || script.contains("${*:")
        || script.contains("BASH_ARGV")
        || names_shift
    {
        return Positional::Unreadable;
    }
    // Inside double quotes a value keeps its characters with these escaped; outside them the
    // same escapes give the characters too.
    let escaped = |text: &str| -> String {
        let mut out = String::new();
        for c in text.chars() {
            if matches!(c, '\\' | '"' | '$' | '`') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    let after: &[String] = args.get(1..).unwrap_or_default();
    let all = escaped(&after.join(" "));
    let chars: Vec<char> = script.chars().collect();
    let mut out = String::new();
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        // `"$@"` and `"${@}"`: one word per argument.
        if let Some(form) = ["\"$@\"", "\"${@}\""].iter().find(|form| {
            let len = form.chars().count();
            chars
                .get(i..i + len)
                .is_some_and(|here| here.iter().copied().eq(form.chars()))
        }) {
            out.push_str(&join(after));
            changed = true;
            i += form.chars().count();
            continue;
        }
        if chars[i] == '$' {
            let braced = chars.get(i + 1) == Some(&'{');
            let at = if braced { i + 2 } else { i + 1 };
            let closed = !braced || chars.get(at + 1) == Some(&'}');
            let end = if braced { at + 2 } else { at + 1 };
            match chars.get(at).copied() {
                Some('@' | '*') if closed => {
                    out.push_str(&all);
                    changed = true;
                    i = end;
                    continue;
                }
                Some(digit @ '0'..='9') if closed => {
                    let index = digit as usize - '0' as usize;
                    out.push_str(&escaped(args.get(index).map_or("", String::as_str)));
                    changed = true;
                    i = end;
                    continue;
                }
                Some('@' | '*' | '0'..='9') => return Positional::Unreadable,
                _ => {}
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if changed {
        Positional::Read(out)
    } else {
        Positional::Same
    }
}

/// `pwsh -CommandWithArgs '<script>' <args...>`: the script with `$args[n]`, `@args` and
/// `$args` replaced by those words, an extra reading.
fn powershell_positional(script: &str, args: &[String]) -> Option<String> {
    if args.is_empty() || !script.to_ascii_lowercase().contains("args") {
        return None;
    }
    let quoted = |word: &String| join_for(std::slice::from_ref(word), Dialect::PowerShell);
    let all = join_for(args, Dialect::PowerShell);
    let chars: Vec<char> = script.chars().collect();
    let mut out = String::new();
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        let lead = chars[i];
        let name: String = chars[i + 1..].iter().take(4).collect();
        if matches!(lead, '$' | '@') && name.eq_ignore_ascii_case("args") {
            let after = i + 5;
            if lead == '$' && chars.get(after) == Some(&'[') {
                let digits: String = chars[after + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .take(4)
                    .collect();
                if !digits.is_empty() && chars.get(after + 1 + digits.len()) == Some(&']') {
                    let value = digits
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| args.get(index))
                        .map(quoted)
                        .unwrap_or_default();
                    out.push_str(&value);
                    changed = true;
                    i = after + 2 + digits.len();
                    continue;
                }
            }
            out.push_str(&all);
            changed = true;
            i = after;
            continue;
        }
        out.push(lead);
        i += 1;
    }
    changed.then_some(out)
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
fn find_commands(args: &[String]) -> (Vec<Vec<String>>, bool) {
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
    // `{}` reads as `<start>/*`, which a leading-dot name (`.git`) escapes. When nothing limits
    // the names found, or a name test can match `.git`, the entries may be the project's `.git`,
    // so the command is also read with `{}` as `<start>/.git`.
    let reaches_git = find_matches_git(args);
    let mut out = Vec::new();
    let all_starts = starts.len() <= MAX_FIND_STARTS;
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
        let substitutions = if reaches_git {
            ["*", ".git"].as_slice()
        } else {
            ["*"].as_slice()
        };
        for start in starts.iter().take(MAX_FIND_STARTS) {
            for found in substitutions {
                out.push(
                    args[begin..end]
                        .iter()
                        .map(|word| word.replace("{}", &format!("{start}/{found}")))
                        .collect(),
                );
            }
        }
        i = end + 1;
    }
    let commands = !out.is_empty();
    (out, all_starts || !commands)
}

/// Whether a `find`'s entries may be the project's `.git`: no name test limits them, a regular
/// expression test is used (not read as a glob), or a name, path or whole-name test can match
/// `.git`.
fn find_matches_git(args: &[String]) -> bool {
    // Lowercase: `-iname .GIT` matches `.git`, and so does `-name .GIT` where names ignore case.
    let hits_git = |value: &str| {
        let value = value.to_lowercase();
        let base = value.rsplit(['/', '\\']).next().unwrap_or(&value);
        crate::command_guard::glob_match(&value, ".git")
            || crate::command_guard::glob_match(base, ".git")
    };
    let mut has_name_test = false;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            // A regular expression is not read as a glob; a negation or an alternative
            // (`-not -name x`, `-name x -o -type d`) lets through entries no name test limits.
            "-regex" | "-iregex" | "!" | "-not" | "-o" | "-or" | "," => return true,
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename" | "-lname"
            | "-ilname" => {
                has_name_test = true;
                if args.get(i + 1).is_some_and(|value| hits_git(value)) {
                    return true;
                }
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    !has_name_test
}

/// Whether a name is a plain shell identifier.
fn plain_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The names a simple command writes, appended to `out`, as the lexer's raw words show them
/// (before wrappers are stripped), so a loop variable can be bound only when written exactly once.
/// Conservative: when unsure it records a name, which only makes the guard ask more.
fn collect_writes(words: &[String], dialect: Dialect, out: &mut Vec<String>) {
    match dialect {
        Dialect::Posix => collect_posix_writes(words, out),
        Dialect::PowerShell => collect_powershell_writes(words, out),
        Dialect::Cmd => {}
    }
}

fn collect_posix_writes(words: &[String], out: &mut Vec<String>) {
    // Reserved words in front of a command (`do d=x`, `then d=x`, `! d=x`, `{ d=x`) leave the
    // assignment after them.
    let mut rest = words;
    while rest.first().is_some_and(|word| {
        matches!(
            word.as_str(),
            "do" | "then" | "else" | "elif" | "if" | "while" | "until" | "!" | "{" | "(" | "time"
        )
    }) {
        rest = &rest[1..];
    }
    // `NAME=` / `NAME+=` assignments at the front of the command.
    while let Some(word) = rest.first() {
        if let Some(name) = posix_assignment_name(word) {
            out.push(name);
            rest = &rest[1..];
        } else {
            break;
        }
    }
    let Some(program) = rest.first().map(String::as_str) else {
        return;
    };
    let args = &rest[1..];
    match program {
        "for" if rest.get(2).map(String::as_str) == Some("in") => {
            if let Some(name) = rest.get(1).filter(|n| plain_identifier(n)) {
                out.push(name.clone());
            }
        }
        "export" | "declare" | "local" | "typeset" | "readonly" => {
            // A name reference (`declare -n r=d`) writes `d` through every later write of `r`.
            if args
                .iter()
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('n'))
            {
                out.push(ANY_WRITE.to_owned());
            }
            for arg in args {
                let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
                if plain_identifier(name) {
                    out.push(name.to_owned());
                }
            }
        }
        // `read [opts] NAME...`, `mapfile/readarray [opts] NAME`, `getopts opts NAME`: the plain
        // identifiers among the arguments are written.
        "read" | "mapfile" | "readarray" | "getopts" => {
            for arg in args {
                if plain_identifier(arg) {
                    out.push(arg.clone());
                }
            }
        }
        // `printf -v NAME ...`.
        "printf" => {
            let mut iter = args.iter();
            while let Some(arg) = iter.next() {
                if arg == "-v" {
                    if let Some(name) = iter.next().filter(|n| plain_identifier(n)) {
                        out.push(name.clone());
                    }
                } else if let Some(name) = arg.strip_prefix("-v").filter(|n| plain_identifier(n)) {
                    out.push(name.to_owned());
                }
            }
        }
        _ => {}
    }
}

/// `NAME=` or `NAME+=` at the start of a word: the variable name.
fn posix_assignment_name(word: &str) -> Option<String> {
    let at = word.find('=')?;
    let name = word[..at].strip_suffix('+').unwrap_or(&word[..at]);
    // An array element (`a[0]=`) writes the array.
    let name = name.split_once('[').map_or(name, |(base, _)| base);
    plain_identifier(name).then(|| name.to_owned())
}

/// The variable name of a PowerShell variable word (`$d`, `$d++`, `${d}`, `$global:d`,
/// `$d,`), lowercase since PowerShell ignores case in names: without `$`, braces, a scope
/// prefix, a trailing `++`/`--` and a list comma.
fn ps_lvalue_name(word: &str) -> Option<String> {
    let rest = word.strip_prefix('$')?;
    let rest = rest.trim_end_matches(',');
    let rest = rest
        .strip_suffix("++")
        .or_else(|| rest.strip_suffix("--"))
        .unwrap_or(rest);
    let rest = rest.split('=').next().unwrap_or(rest);
    let rest = rest
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .unwrap_or(rest);
    let rest = rest.rsplit(':').next().unwrap_or(rest);
    Some(rest.to_ascii_lowercase())
}

/// Recorded when a command may write variables Hangar cannot name (a dot-sourced script, a bash
/// name reference): no loop variable is bound then.
pub const ANY_WRITE: &str = "\u{0}any";

fn collect_powershell_writes(words: &[String], out: &mut Vec<String>) {
    // A dot-sourced script (`. .\x.ps1`) runs in this scope and may set any variable.
    if words.first().is_some_and(|w| w == ".") {
        out.push(ANY_WRITE.to_owned());
    }
    // `foreach ($d in ...)`: the lexer leaves `$d in ...`.
    if words.get(1).is_some_and(|w| w.eq_ignore_ascii_case("in")) {
        if let Some(name) = words
            .first()
            .and_then(|w| ps_lvalue_name(w))
            .filter(|n| plain_identifier(n))
        {
            out.push(name);
        }
    }
    // Every variable before an assignment operator (`$d = ...`, `$d, $e = ...`) and every
    // glued write (`$d=1`, `$d++`, `$d--`).
    let operator = words
        .iter()
        .position(|w| PS_ASSIGNMENTS.contains(&w.as_str()) || w.starts_with('='));
    for (index, word) in words.iter().enumerate() {
        let Some(name) = ps_lvalue_name(word) else {
            continue;
        };
        if !plain_identifier(&name) {
            continue;
        }
        let glued = word.ends_with("++") || word.ends_with("--") || word.contains('=');
        if glued || operator.is_some_and(|at| index < at) {
            out.push(name);
        }
    }
    // A bare word may name a variable to a cmdlet (`Set-Variable d`, `-OutVariable d`,
    // `-ErrorVariable d`, `New-Variable -Name D`): every plain word counts as a possible write,
    // which only makes the guard ask more.
    for word in words {
        let bare = word.trim_matches(['"', '\'']).to_ascii_lowercase();
        if plain_identifier(&bare) {
            out.push(bare);
        }
    }
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

/// The words after the options of a wrapper, read as getopt reads them: `short_values` are the
/// short options that take a value (in a group `-Eu root` the last one takes the next word,
/// `-uroot` holds it), `long_values` the long ones (`--user root`; `--user=root` holds it).
/// `--` ends them.
fn skip_options_with<'a>(
    args: &'a [String],
    short_values: &str,
    long_values: &[&str],
) -> &'a [String] {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" {
            return &args[i + 1..];
        }
        if arg == "-" || !arg.starts_with('-') {
            break;
        }
        if arg.starts_with("--") {
            i += if long_values.contains(&arg.as_str()) {
                2
            } else {
                1
            };
            continue;
        }
        let group = &arg[1..];
        let mut skip = 1;
        for (at, c) in group.char_indices() {
            if short_values.contains(c) {
                if at + c.len_utf8() == group.len() {
                    skip = 2;
                }
                break;
            }
        }
        i += skip;
    }
    args.get(i..).unwrap_or_default()
}

/// `env -S <text>` (also `-S<text>`, `--split-string[=]<text>`, in a group `-iS<text>`): the
/// command line it splits, with the arguments after it; `Some(None)` when the text is missing,
/// `None` without `-S`.
fn env_split_string(args: &[String]) -> Option<Option<String>> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if arg == "--" || arg == "-" || !arg.starts_with('-') {
            return None;
        }
        let (value, after) = if arg == "--split-string" {
            (args.get(i + 1).cloned(), i + 2)
        } else if let Some(value) = arg.strip_prefix("--split-string=") {
            (Some(value.to_owned()), i + 1)
        } else if arg.starts_with("--") {
            i += if arg == "--unset" || arg == "--chdir" {
                2
            } else {
                1
            };
            continue;
        } else {
            let group = &arg[1..];
            let mut found = None;
            let mut skip = 1;
            for (at, c) in group.char_indices() {
                let tail = &group[at + c.len_utf8()..];
                match c {
                    'S' => {
                        found = Some(tail);
                        break;
                    }
                    'u' | 'C' => {
                        if tail.is_empty() {
                            skip = 2;
                        }
                        break;
                    }
                    _ => {}
                }
            }
            match found {
                Some("") => (args.get(i + 1).cloned(), i + 2),
                Some(tail) => (Some(tail.to_owned()), i + 1),
                None => {
                    i += skip;
                    continue;
                }
            }
        };
        return Some(value.map(|value| {
            let mut words = vec![value];
            words.extend(args.get(after..).unwrap_or_default().iter().cloned());
            words.join(" ")
        }));
    }
    None
}

/// What follows the condition of cmd's `if [/i] [not] <condition>`: `exist x`, `defined x`,
/// `errorlevel n`, `cmdextversion n`, `a==b` (also `a== b`, which cmd's lexer leaves as two
/// words, or `a == b`, left as `a b`), `a equ b`.
fn after_cmd_condition(args: &[String]) -> &[String] {
    let mut i = 0;
    let is = |at: usize, word: &str| args.get(at).is_some_and(|a| a.eq_ignore_ascii_case(word));
    if is(i, "/i") {
        i += 1;
    }
    if is(i, "not") {
        i += 1;
    }
    let Some(first) = args.get(i) else {
        return &[];
    };
    let lower = first.to_ascii_lowercase();
    i += if matches!(
        lower.as_str(),
        "exist" | "defined" | "errorlevel" | "cmdextversion"
    ) {
        2
    } else if lower.contains("==") {
        if lower.ends_with("==") {
            2
        } else {
            1
        }
    } else if ["equ", "neq", "lss", "leq", "gtr", "geq"]
        .iter()
        .any(|op| is(i + 1, op))
    {
        3
    } else {
        2
    };
    args.get(i..).unwrap_or_default()
}

/// The value of a PowerShell `-Name:value` parameter word, when it holds one.
fn ps_colon_value(arg: &str) -> Option<&str> {
    ps_param_rest(arg)?
        .split_once(':')
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

/// A PowerShell parameter word without its lead: `-`, `--`, `/` or the dashes PowerShell reads
/// as `-` (U+2013, U+2014, U+2015). `None` for a word that is no parameter.
fn ps_param_rest(arg: &str) -> Option<&str> {
    arg.strip_prefix("--")
        .or_else(|| arg.strip_prefix(['-', '/']))
        .or_else(|| arg.strip_prefix(ps_dash))
}

enum ShellScript<'a> {
    /// The script and the words after it (`$0`, `$1`, ...).
    Inline(&'a str, &'a [String]),
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
                ShellScript::Inline(arg, &args[i + 1..])
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
    let mut program: Option<String> = None;
    let mut list: Option<String> = None;
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
        } else if is_ps_param(arg, "filepath", 2) && !arg.starts_with('/') {
            match ps_colon_value(arg) {
                Some(value) => {
                    program = Some(value.to_owned());
                    i += 1;
                }
                None => {
                    program = args.get(i + 1).cloned();
                    i += 2;
                }
            }
        } else if is_ps_param(arg, "argumentlist", 2) && !arg.starts_with('/') {
            match ps_colon_value(arg) {
                Some(value) => {
                    list = Some(value.to_owned());
                    i += 1;
                }
                None => {
                    list = args.get(i + 1).cloned();
                    i += 2;
                }
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            i += if own_switch { 1 } else { 2 };
        } else if !found && arg.starts_with('/') && arg.len() <= 12 && !arg[1..].contains('/') {
            // cmd's `start /d <folder>`, `/affinity <mask>` and `/node <n>` take a value; its
            // other switches do not.
            let valued = ["/d", "/affinity", "/node"]
                .iter()
                .any(|switch| arg.eq_ignore_ascii_case(switch));
            i += if valued { 2 } else { 1 };
        } else if arg.is_empty() {
            i += 1;
        } else {
            positional.push(arg);
            i += 1;
        }
    }
    let mut positional = positional.into_iter();
    let Some(program) = program.or_else(|| positional.next().cloned()) else {
        return Vec::new();
    };
    let mut call = vec![program];
    let extra: Vec<String> = match list {
        Some(list) => vec![list],
        None => positional.cloned().collect(),
    };
    for value in extra {
        for part in value.split(',') {
            call.extend(part.split_whitespace().map(str::to_owned));
        }
    }
    call
}

enum ShellScriptOwned {
    /// Every reading of the script (see [`scripts_of`]).
    Inline(Vec<String>),
    Undecodable,
    File,
    Stdin,
}

/// Whether `arg` names the PowerShell parameter `full` (lowercase), abbreviated to at least
/// `min` characters, with any lead [`ps_param_rest`] takes and an optional `:value`.
fn is_ps_param(arg: &str, full: &str, min: usize) -> bool {
    let Some(rest) = ps_param_rest(arg) else {
        return false;
    };
    let name = rest
        .split_once(':')
        .map_or(rest, |(name, _)| name)
        .to_ascii_lowercase();
    name.len() >= min && full.starts_with(&name)
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
    ("configurationfile", 14),
    ("encodedarguments", 8),
];

/// What `pwsh` runs: `-Command ...`, `-CommandWithArgs <script> args`, `-EncodedCommand
/// <base64>`, Windows PowerShell's first positional words, a file, or stdin.
fn powershell_script(args: &[String], windows_powershell: bool) -> ShellScriptOwned {
    let found = powershell_script_from(args, windows_powershell);
    if !matches!(found, ShellScriptOwned::File) {
        return found;
    }
    // A parameter Hangar does not know may have taken the script for a file: any `-Command`,
    // `-CommandWithArgs` or `-EncodedCommand` later in the line is read too.
    let later = args.iter().position(|arg| {
        is_ps_param(arg, "command", 1)
            || is_ps_param(arg, "commandwithargs", 8)
            || is_ps_param(arg, "encodedcommand", 3)
            || ps_param_rest(arg).is_some_and(|rest| rest.eq_ignore_ascii_case("cwa"))
            || ps_param_rest(arg).is_some_and(|rest| rest.eq_ignore_ascii_case("ec"))
    });
    match later {
        Some(at) if at > 0 => match powershell_script_from(&args[at..], windows_powershell) {
            ShellScriptOwned::File => found,
            other => other,
        },
        _ => found,
    }
}

fn powershell_script_from(args: &[String], windows_powershell: bool) -> ShellScriptOwned {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if ps_param_rest(arg).is_none() {
            // Windows PowerShell reads positional words as a command; pwsh as a script file.
            return if windows_powershell {
                ShellScriptOwned::Inline(scripts_of(&args[i..], Dialect::PowerShell))
            } else {
                ShellScriptOwned::File
            };
        }
        let rest = ps_param_rest(arg).unwrap_or_default();
        if is_ps_param(arg, "commandwithargs", 8) || rest.eq_ignore_ascii_case("cwa") {
            let (script, after) = match ps_colon_value(arg) {
                Some(value) => (value, args.get(i + 1..).unwrap_or_default()),
                None => match args.get(i + 1) {
                    Some(script) if script != "-" => {
                        (script.as_str(), args.get(i + 2..).unwrap_or_default())
                    }
                    _ => return ShellScriptOwned::Stdin,
                },
            };
            let mut scripts = vec![script.to_owned()];
            scripts.extend(powershell_positional(script, after));
            return ShellScriptOwned::Inline(scripts);
        }
        if is_ps_param(arg, "command", 1) && !is_ps_param(arg, "configurationname", 3) {
            let rest = &args[i + 1..];
            if let Some(value) = ps_colon_value(arg) {
                let mut words = vec![value.to_owned()];
                words.extend(rest.iter().cloned());
                return ShellScriptOwned::Inline(scripts_of(&words, Dialect::PowerShell));
            }
            return match rest {
                [] => ShellScriptOwned::Stdin,
                [one] if one == "-" => ShellScriptOwned::Stdin,
                _ => ShellScriptOwned::Inline(scripts_of(rest, Dialect::PowerShell)),
            };
        }
        if is_ps_param(arg, "encodedcommand", 1) || rest.eq_ignore_ascii_case("ec") {
            let text = ps_colon_value(arg).or_else(|| args.get(i + 1).map(String::as_str));
            return match text.and_then(decode_powershell) {
                Some(script) => ShellScriptOwned::Inline(vec![script]),
                None => ShellScriptOwned::Undecodable,
            };
        }
        if is_ps_param(arg, "file", 1) {
            return match ps_colon_value(arg).or_else(|| args.get(i + 1).map(String::as_str)) {
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
            .any(|(full, min)| is_ps_param(arg, full, *min))
            || rest.eq_ignore_ascii_case("ea");
        i += if takes_value && ps_colon_value(arg).is_none() {
            2
        } else {
            1
        };
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

/// The command lines a shell may get as its remaining arguments: one argument is the line
/// itself (`cmd /c "a && b"`); several are read twice, re-quoted so each stays one word, and
/// joined with blanks as they are, which is what cmd, PowerShell, `eval`, `wsl` and `ssh` do
/// (`pwsh -c 'git push' '--force'` runs `git push --force`).
fn scripts_of(words: &[String], dialect: Dialect) -> Vec<String> {
    match words {
        [] => Vec::new(),
        [one] => vec![one.clone()],
        _ => {
            let quoted = join_for(words, dialect);
            let raw = words.join(" ");
            if raw == quoted {
                vec![quoted]
            } else {
                vec![quoted, raw]
            }
        }
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
        // A `Join-Path` of plain words is read as the path it makes.
        assert_eq!(
            analysis.calls,
            vec![vec![
                "Remove-Item".to_owned(),
                "-Recurse".into(),
                r"$HOME\x".into()
            ]]
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

    #[test]
    fn powershell_braces_end_words_except_variables_and_hashtables() {
        assert_eq!(
            calls("1|%{git push --force}", Dialect::PowerShell),
            vec![vec!["1".to_owned()], vec!["%".into()], push_force()]
        );
        assert_eq!(
            calls("echo ${env:x}", Dialect::PowerShell),
            vec![vec!["echo", "${env:x}"]]
        );
        let analysis = split("$h = @{a=1}", Dialect::PowerShell);
        assert!(!analysis.unclosed);
        // The assignment is skipped; the value is the call.
        assert!(analysis.calls.contains(&vec!["@{a=1}".to_owned()]));
        // A value of a hashtable may run a command.
        assert!(calls("$h = @{a=$(git push --force)}", Dialect::PowerShell).contains(&push_force()));
        assert!(split("echo ${env:x", Dialect::PowerShell).unclosed);
    }

    #[test]
    fn arithmetic_holds_no_here_documents() {
        for command in [
            "(( x = 1 << EOF ))\ngit push --force\nEOF",
            "echo $((1 << EOF))\ngit push --force\nEOF",
            "echo $[1<<EOF]\ngit push --force\nEOF",
            "echo ${x:0:1<<EOF}\ngit push --force\nEOF",
        ] {
            let analysis = split(command, Dialect::Posix);
            assert!(analysis.calls.contains(&push_force()), "{command:?}");
            assert!(!analysis.unclosed, "{command:?}");
        }
        // Two subshells bash may read where `((` starts are still seen.
        assert!(calls("((echo a); (git push --force))", Dialect::Posix).contains(&push_force()));
        assert!(split("echo ${x", Dialect::Posix).unclosed);
    }

    #[test]
    fn a_substitution_ends_where_bash_ends_it() {
        for (command, end) in [
            ("$(echo x # )\n)", 13),
            ("$(case a in a) :;; esac)", 23),
            ("$(case a in (a) :;; esac)", 24),
            ("$(echo \"$(echo \")\")\")", 20),
            (r"$(echo $'\'' )", 13),
            ("$(echo `echo )`)", 15),
        ] {
            let chars: Vec<char> = command.chars().collect();
            assert_eq!(
                closing_paren(&chars, 2, Dialect::Posix),
                Some(end),
                "{command:?}"
            );
        }
        // Past the nesting bound nothing is guessed.
        let deep = format!("$({}", "\"$(".repeat(40));
        let chars: Vec<char> = deep.chars().collect();
        assert_eq!(closing_paren(&chars, 2, Dialect::Posix), None);
    }

    #[test]
    fn find_with_more_start_folders_than_read_is_reported() {
        let starts = (0..9)
            .map(|i| format!("d{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!("find {starts} -exec rm -r {{}} +");
        assert!(split(&command, Dialect::Posix).truncated);
        assert!(!split("find a b -name x -print", Dialect::Posix).truncated);
    }
}
