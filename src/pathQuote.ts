// Quoting dropped file paths and pasted image paths for the program running in a session. Each
// shell has its own rules; agent CLIs read the input as text and only need quotes around paths
// with spaces.
//
// Known limit: the kind follows the program the session's profile starts, not whatever runs in
// the session right now. Bash started inside a cmd session still gets cmd quoting, an agent
// started from a PowerShell session gets PowerShell quoting. The terminal cannot tell which
// program reads the input, and both quotings stay readable for an agent.

/**
 * - `powershell`: pwsh, powershell
 * - `cmd`: cmd.exe
 * - `posix`: bash, zsh, sh, fish and friends
 * - `wsl`: a POSIX shell inside WSL; Windows paths become `/mnt/<drive>/...`
 * - `agent`: everything else (agent CLIs, unknown programs)
 */
export type ShellKind = "powershell" | "cmd" | "posix" | "wsl" | "agent";

const POSIX_SHELLS = new Set(["bash", "zsh", "sh", "fish", "dash", "ksh", "mksh", "tcsh", "csh"]);

/**
 * `bash.exe` in the Windows system folder is the WSL launcher, not Git Bash: it runs the default
 * distribution, which sees Windows drives under `/mnt`.
 */
const WSL_BASH = /^[a-z]:[\\/]windows[\\/](system32|sysnative)[\\/]bash(\.exe)?$/i;

/** Decides by the program's file name; the command may be a full path with an extension. */
export function shellKind(command: string): ShellKind {
  const trimmed = command.trim();
  const file = trimmed.split(/[\\/]/).pop() ?? "";
  const name = file.toLowerCase().replace(/\.(exe|cmd|bat|com)$/, "");
  if (name === "pwsh" || name === "powershell" || name === "pwsh-preview") return "powershell";
  if (name === "cmd") return "cmd";
  if (name === "wsl" || WSL_BASH.test(trimmed)) return "wsl";
  if (POSIX_SHELLS.has(name)) return "posix";
  return "agent";
}

// Characters that never need quotes. Backslashes only for Windows shells: in POSIX shells they
// escape the next character. Letters and digits of any script are fine everywhere.
const SAFE_WINDOWS = /^[\p{L}\p{N}_\-.:\\/]+$/u;
const SAFE_POSIX = /^[\p{L}\p{N}_\-.:/]+$/u;

/** PowerShell also treats typographic single quotes as quote characters. */
const POWERSHELL_QUOTES = /['\u2018\u2019\u201a\u201b]/g;

/** `C:\x\y` as WSL sees it (`/mnt/c/x/y`); `\\wsl$\Distro\home` as `/home`. Others unchanged. */
export function wslPath(path: string): string {
  const drive = /^([a-z]):[\\/]?(.*)$/i.exec(path);
  if (drive?.[1] !== undefined) {
    const rest = (drive[2] ?? "").replace(/\\/g, "/").replace(/\/+$/, "");
    const base = `/mnt/${drive[1].toLowerCase()}`;
    return rest === "" ? base : `${base}/${rest}`;
  }
  const share = /^[\\/]{2}(?:wsl\$|wsl\.localhost)[\\/][^\\/]+(.*)$/i.exec(path);
  if (share) {
    const rest = (share[1] ?? "").replace(/\\/g, "/");
    return rest === "" ? "/" : rest;
  }
  return path;
}

export function quotePath(path: string, kind: ShellKind): string {
  switch (kind) {
    case "powershell":
      return SAFE_WINDOWS.test(path) ? path : `'${path.replace(POWERSHELL_QUOTES, "$&$&")}'`;
    case "cmd":
      // Windows file names cannot contain `"`, so plain double quotes are enough. Known limit:
      // cmd expands `%NAME%` even inside double quotes when NAME is a defined variable, and an
      // interactive command line has no escape for `%`. Undefined names stay literal; such file
      // names are rare enough to accept this.
      return SAFE_WINDOWS.test(path) ? path : `"${path}"`;
    case "posix":
      return posixQuote(path);
    case "wsl":
      return posixQuote(wslPath(path));
    case "agent":
      return agentQuote(path);
  }
}

/**
 * A path in an agent's prompt is read, not parsed by a shell: quote it with a quote kind it does
 * not contain. Only a path with both kinds gets backslash escapes, and then backslashes too, so
 * the text stays unambiguous.
 */
function agentQuote(path: string): string {
  if (/^[^\s"']+$/u.test(path)) return path;
  if (!path.includes('"')) return `"${path}"`;
  if (!path.includes("'")) return `'${path}'`;
  return `"${path.replace(/[\\"]/g, "\\$&")}"`;
}

function posixQuote(path: string): string {
  return SAFE_POSIX.test(path) ? path : `'${path.replace(/'/g, "'\\''")}'`;
}

/** All kinds, for checking a kind read from a message. */
export const SHELL_KINDS: readonly ShellKind[] = ["powershell", "cmd", "posix", "wsl", "agent"];

export function isShellKind(value: unknown): value is ShellKind {
  return SHELL_KINDS.some((kind) => kind === value);
}

/** An entry of the Files tab: a path below the tree's folder, with `/` between its names. */
export interface TreePath {
  /** The tree's folder: the working folder of the session the tree shows. */
  root: string;
  /** Relative to `root`, `/` between names, never empty. */
  path: string;
  folder: boolean;
}

// eslint-disable-next-line no-control-regex -- control characters are exactly the target here.
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** A Windows path: a drive (`C:\`, `C:/`) or a share (`\\server\share`). */
function isWindowsPath(path: string): boolean {
  return /^[a-z]:([\\/]|$)/i.test(path) || /^[\\/]{2}[^\\/]/.test(path);
}

/** `/` between names and no trailing `/`, except for a bare root (`/`, `C:/`). */
function normalized(path: string, windows: boolean): string {
  const slashed = windows ? path.replace(/\\/g, "/") : path;
  const trimmed = slashed.replace(/\/+$/, "");
  if (trimmed === "") return "/";
  return /^[a-z]:$/i.test(trimmed) ? `${trimmed}/` : trimmed;
}

/** `path` relative to `folder` (both normalized), or `null` when it lies outside. */
function relativeTo(path: string, folder: string, windows: boolean): string | null {
  const prefix = folder.endsWith("/") ? folder : `${folder}/`;
  const head = path.slice(0, prefix.length);
  const same = windows ? head.toLowerCase() === prefix.toLowerCase() : head === prefix;
  return same && path.length > prefix.length ? path.slice(prefix.length) : null;
}

/** `C:/x/y` as Git Bash sees it (`/c/x/y`); a share keeps its `//server/share` form. */
function gitBashPath(path: string): string {
  const drive = /^([a-z]):\/?(.*)$/i.exec(path);
  if (drive?.[1] === undefined) return path;
  const rest = drive[2] ?? "";
  const base = `/${drive[1].toLowerCase()}`;
  return rest === "" ? base : `${base}/${rest}`;
}

/**
 * A shell would read a relative path starting with `-` as an option: it gets a `./` (`.\`) in
 * front, which names the same file.
 */
function notAnOption(relative: string, separator: string): string {
  return relative.startsWith("-") ? `.${separator}${relative}` : relative;
}

/**
 * What inserting a Files tab entry types into a session, for the program reading it:
 *
 * - `agent`: an @ mention (`@src/app.ts`), `/` between names, a folder with a trailing `/`, as
 *   agent CLIs mention files and folders; a path with spaces quoted (`@"my notes.md"`).
 * - `powershell`, `cmd`: backslashes, quoted as that shell needs it.
 * - `posix`: `/` between names; a Windows path as Git Bash names it (`/c/...`).
 * - `wsl`: `/` between names; a Windows path as WSL sees it (`/mnt/c/...`).
 *
 * Inside the session's working folder the path is relative to it, otherwise absolute, so a row
 * dragged onto a session in another folder still names the right file. A trailing space keeps the
 * next word apart.
 *
 * Empty for a path with control characters (a line break, ESC): no quoting makes them safe in
 * every shell, and a program without bracketed paste would run what follows a line break.
 */
export function insertPathText(item: TreePath, sessionCwd: string, kind: ShellKind): string {
  if (CONTROL.test(item.root) || CONTROL.test(item.path)) return "";
  const windows = isWindowsPath(item.root);
  const root = normalized(item.root, windows);
  const inner = item.path.replace(/^\/+|\/+$/g, "");
  const absolute = root.endsWith("/") ? `${root}${inner}` : `${root}/${inner}`;
  const relative = relativeTo(absolute, normalized(sessionCwd, windows), windows);
  switch (kind) {
    case "agent": {
      const path = relative ?? absolute;
      return `@${agentQuote(item.folder ? `${path}/` : path)} `;
    }
    case "powershell":
    case "cmd": {
      const path = relative === null ? absolute : notAnOption(relative, "/");
      const native = windows ? path.replace(/\//g, "\\") : path;
      return `${quotePath(native, kind)} `;
    }
    case "posix": {
      if (relative !== null) return `${posixQuote(notAnOption(relative, "/"))} `;
      return `${posixQuote(windows ? gitBashPath(absolute) : absolute)} `;
    }
    case "wsl": {
      if (relative !== null) return `${posixQuote(notAnOption(relative, "/"))} `;
      return `${posixQuote(wslPath(absolute))} `;
    }
  }
}

/**
 * Text inserted for dropped paths: quoted, separated by spaces, with a trailing space so the
 * next drop or word does not stick to it. Empty for no paths.
 */
export function dropText(paths: readonly string[], kind: ShellKind): string {
  const quoted = paths.filter((path) => path !== "").map((path) => quotePath(path, kind));
  return quoted.length === 0 ? "" : `${quoted.join(" ")} `;
}
