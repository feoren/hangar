// Export of a session's scrollback as a text or HTML file. The backend opens the save dialog
// and writes the file (`export_session`); here the content and the suggested name are built.

export type ExportFormat = "text" | "html";

/** The backend's limit for one export. */
export const MAX_EXPORT_BYTES = 32 * 1024 * 1024;

/** One row of the terminal buffer; `wrapped` continues the previous row (soft line break). */
export interface BufferRow {
  text: string;
  wrapped: boolean;
}

/** Rows to lines: wrapped rows join their predecessor, empty lines at the end are dropped. */
export function bufferText(rows: readonly BufferRow[], eol: string): string {
  const lines: string[] = [];
  for (const row of rows) {
    const last = lines.length - 1;
    if (row.wrapped && last >= 0) lines[last] = `${lines[last] ?? ""}${row.text}`;
    else lines.push(row.text);
  }
  const trimmed = lines.map((line) => line.replace(/\s+$/, ""));
  while (trimmed.length > 0 && trimmed[trimmed.length - 1] === "") trimmed.pop();
  return trimmed.length === 0 ? "" : `${trimmed.join(eol)}${eol}`;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/**
 * "Claude Code 2026-11-08 15-30.txt": session title plus local time, without characters that
 * Windows, macOS or Linux forbid in file names.
 */
export function exportFileName(title: string, date: Date, format: ExportFormat): string {
  const stamp = `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}-${pad(date.getMinutes())}`;
  const clean = title
    // eslint-disable-next-line no-control-regex
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[. ]+$/, "")
    .slice(0, 80)
    .trim();
  const base = clean === "" ? "Hangar" : clean;
  return `${base} ${stamp}.${format === "html" ? "html" : "txt"}`;
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * xterm's HTML serialization is made for the clipboard (`<html><body><!--StartFragment-->`).
 * As a file it needs a charset, otherwise umlauts and emoji break, and a title.
 */
export function htmlDocument(fragment: string, title: string, background: string): string {
  const head = `<!DOCTYPE html>\n<html>\n<head>\n<meta charset="utf-8">\n<title>${escapeHtml(title)}</title>\n</head>\n<body style="margin:0;padding:16px;background:${escapeHtml(background)}">`;
  const body = fragment.replace(/^\s*<html>\s*<body>/i, "").replace(/<\/body>\s*<\/html>\s*$/i, "");
  return `${head}${body}</body>\n</html>\n`;
}

/** Line ending of a text export: CRLF on Windows, so Notepad and friends show the lines. */
export function exportEol(userAgent: string): string {
  return userAgent.includes("Windows") ? "\r\n" : "\n";
}

/** UTF-8 size, as the backend counts it. */
export function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}
