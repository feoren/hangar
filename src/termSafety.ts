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
 * `null`. Hangar never opens `javascript:`, `file:` or custom schemes, nor an address with a user
 * name or password: `https://github.com@example.net/` reads like GitHub and leads elsewhere.
 */
export function safeLinkUrl(text: string): string | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  if (url.username !== "" || url.password !== "") return null;
  return url.href;
}

/** An address cut around its host, so the UI can set the host apart from the rest. */
export interface AddressParts {
  before: string;
  host: string;
  after: string;
}

/**
 * `url` split at its host name (`https://` · `github.com` · `/org/repo`). Without a host that
 * can be found in it, all of it is `before`.
 */
export function addressParts(url: string): AddressParts {
  let host: string;
  try {
    host = new URL(url).hostname;
  } catch {
    return { before: url, host: "", after: "" };
  }
  const start = url.indexOf("//") + 2;
  if (host === "" || start < 2 || url.slice(start, start + host.length) !== host) {
    return { before: url, host: "", after: "" };
  }
  return { before: url.slice(0, start), host, after: url.slice(start + host.length) };
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
/** An IPv6 address in brackets, as in an address: `[::1]:3000`. */
const IPV6_LIKE = /^\[[0-9a-f:.]*:[0-9a-f:.]*\](?::\d{1,5})?(?:[/?#].*)?$/iu;
/** A top-level domain: letters, or an internationalized one in Punycode. */
const TLD = /^(?:\p{L}{2,}|xn--[a-z0-9-]+)$/iu;
const IPV4 = /^\d{1,3}(?:\.\d{1,3}){3}$/u;
/**
 * Endings that name a file far more often than a domain. A bare `README.md` or `install.sh` as
 * link text is a file name, not a claimed host; with a path or port (`example.md/x`) the text
 * still counts as an address.
 */
const FILE_ENDINGS = new Set(
  (
    "apk bat cfg cjs cmd conf cpp cs css csv deb diff dll dmg env exe gif go gz hpp html ini jar " +
    "java jpeg jpg js json jsx lock log md mjs msi patch pdf pkg png ps1 py rb rpm rs sh sql svg " +
    "tar tgz toml ts tsx txt xml yaml yml zip"
  ).split(" "),
);
/** A bare file name: no path, no scheme, ending in one of `FILE_ENDINGS`. */
const FILE_NAME = /^[^\s/\\:?#]+\.(?<ending>[a-z0-9]{1,8})$/iu;
/** A file name with a line and maybe a column, as tools print a place in a file: `app.ts:42:7`. */
const FILE_PLACE = /^(?<name>[^\s/\\:?#]+\.[a-z0-9]{1,8})(?::\d+){1,2}$/iu;
/**
 * Characters nobody sees: zero-width spaces and joiners, the soft hyphen, the word joiner, bidi
 * marks and overrides, variation selectors. In link text they split a host name so that it reads
 * `github.com` while no check recognizes it as one, or turn its letters around.
 */
const IGNORABLE = /\p{Default_Ignorable_Code_Point}/u;
/** A variation selector after a pictograph and a joiner between two draw an emoji. */
const EMOJI_GLUE =
  /(?<=\p{Extended_Pictographic})\uFE0F|(?<=\p{Extended_Pictographic}\uFE0F?)\u200d(?=\p{Extended_Pictographic})/gu;
/** Ideographic and fullwidth full stops, which browsers read as the dot in a host name. */
const WIDE_DOTS = /[\u3002\uFF0E\uFF61]/gu;
/** Opening brackets and quotes before a word: `(github.com)`, `"github.com"`. */
const LEADING = /^[([{<"'`\u00AB\u2039\u201C\u2018\u300C\u300E]+/u;
/** Punctuation and closing brackets after a word: `github.com.`, `github.com),`. */
const TRAILING = /[)\]}>"'`\u00BB\u203A\u201D\u2019\u300D\u300F.,;:!?]+$/u;

/**
 * Whether a word without a scheme claims a host: `github.com/org`, `localhost:3000` or
 * `10.0.0.1` do; a version (`v0.8.0`) or a file name (`README.md`) does not.
 */
function claimsHost(word: string): boolean {
  const match = HOST_LIKE.exec(word);
  const host = match?.groups?.host;
  if (match === null || host === undefined) return false;
  if (host.toLowerCase() === "localhost" || IPV4.test(host)) return true;
  const tld = host.slice(host.lastIndexOf(".") + 1);
  if (!TLD.test(tld)) return false;
  return (match.groups?.rest ?? "") !== "" || !FILE_ENDINGS.has(tld.toLowerCase());
}

/** Whether a word is a bare file name such as `install.sh`. */
function isFileName(word: string): boolean {
  const ending = FILE_NAME.exec(word)?.groups?.ending;
  return ending !== undefined && FILE_ENDINGS.has(ending.toLowerCase());
}

/** A host name without a leading `www.`, which names the same site as the bare host. */
function siteOf(hostname: string): string {
  return hostname.startsWith("www.") ? hostname.slice(4) : hostname;
}

/** The last name in the target's path, decoded; "" for none. */
function lastPathName(target: URL): string {
  const name = target.pathname.slice(target.pathname.lastIndexOf("/") + 1);
  try {
    return decodeURIComponent(name);
  } catch {
    return name;
  }
}

/**
 * Whether one word of the link text claims something the target is not: another host, a path,
 * a file of another name.
 */
function wordMisleads(raw: string, target: URL | null): boolean {
  if (PATH_LIKE.test(raw)) return true;
  const unpunctuated = raw.replace(TRAILING, "");
  const word = unpunctuated.replace(LEADING, "");
  if (word === "") return false;
  if (PATH_LIKE.test(word)) return true;
  const place = FILE_PLACE.exec(word)?.groups?.name;
  // `app.ts:42` on a link to `.../app.ts#L42` names the target's own file, not a host and port.
  if (place !== undefined && isFileName(place) && target !== null) {
    if (lastPathName(target).toLowerCase() === place.toLowerCase()) return false;
  }
  let claimed: URL;
  try {
    if (IPV6_LIKE.test(unpunctuated)) claimed = new URL(`https://${unpunctuated}`);
    else if (WITH_SCHEME.test(word)) claimed = new URL(word);
    else if (claimsHost(word)) claimed = new URL(`https://${word}`);
    else if (isFileName(word)) {
      // A file name says what the click brings: fine only when the target is that very file.
      return target === null || lastPathName(target).toLowerCase() !== word.toLowerCase();
    } else return false;
  } catch {
    // Reads like an address but is none: the target cannot be what it claims.
    return true;
  }
  if (claimed.protocol !== "http:" && claimed.protocol !== "https:") return true;
  if (claimed.username !== "" || claimed.password !== "") return true;
  return target === null || siteOf(claimed.hostname) !== siteOf(target.hostname);
}

/**
 * An OSC 8 link shows one text and leads elsewhere: a program can print
 * `https://github.com/org/repo` as the visible text of a link to another host. When the visible
 * text claims an address of another host, looks like a path, names a file the target is not, or
 * holds characters nobody sees, the click shows the real target first ("confirm"). Plain words
 * and versions claim nothing; those links open at once ("open"); the hover still shows where they
 * lead.
 *
 * `shown` is the whole word around the link on screen (`linkWord` in terminalLinkWord.ts), so
 * text printed right before or after the link counts too. Each word is read on its own, without
 * the brackets and punctuation around it, so `(github.com).` or `see github.com/org` claim GitHub
 * as well. `target` is the link's address, already checked by `safeLinkUrl`.
 */
export function osc8Decision(shown: string, target: string): Osc8Decision {
  if (IGNORABLE.test(shown.replace(EMOJI_GLUE, ""))) return "confirm";
  const text = shown
    .normalize("NFKC")
    .replace(WIDE_DOTS, ".")
    // `github .com` and `github . com` still read as one host name.
    .replace(/\s+\.\s*/gu, ".");
  let real: URL | null;
  try {
    real = new URL(target);
  } catch {
    real = null;
  }
  const misleads = text.split(/\s+/u).some((word) => word !== "" && wordMisleads(word, real));
  return misleads ? "confirm" : "open";
}
