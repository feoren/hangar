// Guard at the boundary between the clipboard or program output and the terminal. Pure logic,
// so it can be tested without xterm.

/**
 * Control characters that have no business in a paste: C0 except tab, line feed and carriage
 * return, plus DEL and the C1 characters (0x80 to 0x9f, e.g. 0x9b as 8-bit CSI).
 * ESC is what matters: a pasted `ESC[201~` would otherwise end bracketed paste early, and the
 * rest of the text would run into the shell or the agent as typed commands.
 */
// eslint-disable-next-line no-control-regex -- control characters are exactly the target here.
const PASTE_CONTROL = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/g;

/** Clipboard text, sanitized for `term.paste`. Line breaks are preserved. */
export function sanitizePaste(text: string): string {
  return text.replace(PASTE_CONTROL, "");
}

/**
 * Address of a terminal link (OSC 8) if it is safe to show: only http and https, otherwise
 * `null`. Hangar never opens `javascript:`, `file:` or custom schemes.
 */
export function safeLinkUrl(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  return url.href;
}

/** What a click on an OSC 8 link does: open at once, or show the real address first. */
export type Osc8Decision = "open" | "confirm";

/** Text that reads like a file path: absolute, relative with `./`, home, a drive or a share. */
const PATH_LIKE = /^(?:\/|\.{1,2}[\\/]|~[\\/]|[a-z]:[\\/]|\\\\)/iu;
/** An address with a scheme of its own (`https://`, `file://`). */
const WITH_SCHEME = /^[a-z][a-z0-9+.-]*:\/\//iu;
/** A host name with at least one dot, or `localhost`, optionally with a port and a path. */
const HOST_LIKE =
  /^(?<host>localhost|[\p{L}\p{N}-]+(?:\.[\p{L}\p{N}-]+)+)(?<rest>(?::\d{1,5})?(?:[/?#].*)?)$/iu;
/** A top-level domain: letters, or an internationalized one in Punycode. */
const TLD = /^(?:\p{L}{2,}|xn--[a-z0-9-]+)$/iu;
const IPV4 = /^\d{1,3}(?:\.\d{1,3}){3}$/u;
/**
 * Endings that name a file far more often than a domain. A bare `README.md` or `app.test.ts`
 * as link text is a file name, not a claimed host; with a path or port (`example.md/x`) the text
 * still counts as an address.
 */
const FILE_ENDINGS = new Set(
  (
    "bat cfg cjs cmd conf cpp cs css csv diff dll env exe gif go hpp html ini java jpeg jpg js " +
    "json jsx lock log md mjs patch pdf png py rb rs sh sql svg toml ts tsx txt xml yaml yml zip"
  ).split(" "),
);

/**
 * Whether link text without a scheme claims a host: `github.com/org`, `localhost:3000` or
 * `10.0.0.1` do; a version (`v0.8.0`) or a file name (`README.md`) does not.
 */
function claimsHost(text: string): boolean {
  const match = HOST_LIKE.exec(text);
  const host = match?.groups?.host;
  if (match === null || host === undefined) return false;
  if (host.toLowerCase() === "localhost" || IPV4.test(host)) return true;
  const tld = host.slice(host.lastIndexOf(".") + 1);
  if (!TLD.test(tld)) return false;
  return (match.groups?.rest ?? "") !== "" || !FILE_ENDINGS.has(tld.toLowerCase());
}

/**
 * An OSC 8 link shows one text and leads elsewhere: a program can print
 * `https://github.com/org/repo` as the visible text of a link to another host. When the visible
 * text claims an address of another host, or looks like a path, the click shows the real
 * target first ("confirm"). Plain words, versions and bare file names claim nothing; those
 * links open at once ("open"); the hover still shows where they lead.
 * `target` is the link's address, already checked by `safeLinkUrl`.
 */
export function osc8Decision(shown: string, target: string): Osc8Decision {
  const text = shown.trim();
  if (text === "") return "open";
  if (PATH_LIKE.test(text)) return "confirm";
  let claimed: URL;
  try {
    if (WITH_SCHEME.test(text)) claimed = new URL(text);
    else if (claimsHost(text)) claimed = new URL(`https://${text}`);
    else return "open";
  } catch {
    // Reads like an address but is none: the target cannot be what it claims.
    return "confirm";
  }
  if (claimed.protocol !== "http:" && claimed.protocol !== "https:") return "confirm";
  let real: URL;
  try {
    real = new URL(target);
  } catch {
    return "confirm";
  }
  return claimed.hostname === real.hostname ? "open" : "confirm";
}
