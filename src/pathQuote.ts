// Quoting dropped file paths for the program running in a session. Each shell has its own rules;
// agent CLIs read the input as text and only need quotes around paths with spaces.

/**
 * - `powershell`: pwsh, powershell
 * - `cmd`: cmd.exe
 * - `posix`: bash, zsh, sh, fish and friends
 * - `wsl`: a POSIX shell inside WSL; Windows paths become `/mnt/<drive>/...`
 * - `agent`: everything else (agent CLIs, unknown programs)
 */
export type ShellKind = "powershell" | "cmd" | "posix" | "wsl" | "agent";

const POSIX_SHELLS = new Set(["bash", "zsh", "sh", "fish", "dash", "ksh", "mksh", "tcsh", "csh"]);

/** Decides by the program's file name; the command may be a full path with an extension. */
export function shellKind(command: string): ShellKind {
  const file = command.trim().split(/[\\/]/).pop() ?? "";
  const name = file.toLowerCase().replace(/\.(exe|cmd|bat|com)$/, "");
  if (name === "pwsh" || name === "powershell" || name === "pwsh-preview") return "powershell";
  if (name === "cmd") return "cmd";
  if (name === "wsl") return "wsl";
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
      // Windows file names cannot contain `"`, so plain double quotes are enough.
      return SAFE_WINDOWS.test(path) ? path : `"${path}"`;
    case "posix":
      return posixQuote(path);
    case "wsl":
      return posixQuote(wslPath(path));
    case "agent":
      return /^[^\s"']+$/u.test(path) ? path : `"${path.replace(/"/g, '\\"')}"`;
  }
}

function posixQuote(path: string): string {
  return SAFE_POSIX.test(path) ? path : `'${path.replace(/'/g, "'\\''")}'`;
}

/**
 * Text inserted for dropped paths: quoted, separated by spaces, with a trailing space so the
 * next drop or word does not stick to it. Empty for no paths.
 */
export function dropText(paths: readonly string[], kind: ShellKind): string {
  const quoted = paths.filter((path) => path !== "").map((path) => quotePath(path, kind));
  return quoted.length === 0 ? "" : `${quoted.join(" ")} `;
}
