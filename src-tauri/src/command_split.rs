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

/// The shell language a script is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Posix,
    PowerShell,
    Cmd,
}

/// What a command line runs, as far as the lexers can tell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    /// Each program call's words, wrappers removed, program first, in order.
    pub calls: Vec<Vec<String>>,
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

/// A word whose value Hangar cannot know (a substitution); it reads as a variable, so a path
/// built from it counts as unplaceable.
pub const UNKNOWN: &str = "$?";

/// Every program call of `command` read as `dialect`.
pub fn split(command: &str, dialect: Dialect) -> Analysis {
    let mut splitter = Splitter::default();
    splitter.script(command, dialect, 0);
    splitter.analysis
}

/// One part of a script in order: a simple command's words, or a nested script to split.
enum Piece {
    /// The words and the redirection targets of one simple command.
    Command(Vec<String>, Vec<String>),
    Script(String, Dialect),
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
    redirects: Vec<String>,
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
        } else {
            self.words.push(word);
        }
        self.in_word = false;
    }

    fn end_command(&mut self) {
        self.end_word();
        self.target = false;
        if !self.words.is_empty() || !self.redirects.is_empty() {
            self.pieces.push(Piece::Command(
                std::mem::take(&mut self.words),
                std::mem::take(&mut self.redirects),
            ));
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

    fn substitution(&mut self, inner: String, dialect: Dialect) {
        self.pieces.push(Piece::Script(inner, dialect));
        self.push_str(UNKNOWN);
    }

    fn push_str(&mut self, text: &str) {
        self.in_word = true;
        self.word.push_str(text);
    }
}

/// Where the `)` closing a `(` before `start` is, aware of quotes; `None` when it never closes.
fn closing_paren(chars: &[char], start: usize, dialect: Dialect) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
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

/// POSIX shell: quotes, every backslash outside quotes escapes, `$'...'`, `$(...)`, backticks.
fn lex_posix(text: &str) -> Words {
    let chars: Vec<char> = text.chars().collect();
    let mut w = Words::default();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            ' ' | '\t' | '\r' => w.end_word(),
            '\n' | ';' => w.end_command(),
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
            '|' => {
                w.end_command();
                if matches!(next, Some('|' | '&')) {
                    i += 1;
                }
            }
            '(' | ')' => w.end_command(),
            '{' | '}' if !w.in_word => w.end_command(),
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
    let end = if paren {
        closing_paren(chars, start, Dialect::Posix)
    } else {
        closing_backtick(chars, start)
    };
    let end = end.unwrap_or_else(|| {
        w.unclosed = true;
        chars.len()
    });
    w.substitution(text_of(chars, start, end), Dialect::Posix);
    end
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
            '|' => {
                w.end_command();
                if next == Some('|') {
                    i += 1;
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
    w.substitution(text_of(chars, start, end), Dialect::PowerShell);
    end
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
            '|' => {
                w.end_command();
                if next == Some('|') {
                    i += 1;
                }
            }
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
            other => w.push(other),
        }
        i += 1;
    }
    w.end_command();
    w
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
        for piece in words.pieces {
            match piece {
                Piece::Command(words, redirects) => {
                    let at = self.analysis.calls.len();
                    self.analysis
                        .redirects
                        .extend(redirects.into_iter().map(|target| (at, target)));
                    self.command(&words, dialect, depth);
                }
                Piece::Script(inner, dialect) => self.script(&inner, dialect, depth + 1),
            }
        }
    }

    fn push(&mut self, words: &[String]) {
        if self.analysis.calls.len() >= MAX_CALLS {
            self.analysis.truncated = true;
            return;
        }
        self.analysis.calls.push(words.to_vec());
    }

    /// One simple command: wrappers in front of the program are skipped, inline scripts are
    /// split in turn, and what is left is one program call.
    fn command(&mut self, words: &[String], dialect: Dialect, depth: usize) {
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
                "{" | "}" | "!" | "then" | "do" | "else" | "elif" | "if" | "while" | "until"
                | "time" | "command" | "builtin" | "exec" | "nohup" | "&" | "." | "call"
                | "noglob" => rest = args,
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
                            self.push(rest);
                        }
                        ShellScript::File => self.push(rest),
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
                            self.push(rest);
                        }
                        ShellScriptOwned::Stdin => {
                            self.analysis.stdin_script = true;
                            self.push(rest);
                        }
                        ShellScriptOwned::File => self.push(rest),
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
                            self.push(rest);
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
                            self.push(rest);
                            return;
                        }
                    }
                }
                "start" | "start-process" | "saps" => {
                    let call = started_program(args);
                    if call.is_empty() {
                        self.push(rest);
                    } else {
                        // One level deeper, so `start start start ...` ends at MAX_DEPTH.
                        self.command(&call, dialect, depth + 1);
                    }
                    return;
                }
                _ => {
                    self.push(rest);
                    return;
                }
            }
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
            return ShellScriptOwned::File;
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
