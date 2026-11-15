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

/**
 * A file path at the start of a word or right after a sign: `/tmp/x`, `./x`, `~/x`, `C:\x` or a
 * share (`\\server`), also behind `(` or `=`. Not a `/` between two names (`src/app.ts`).
 */
const PATH_LIKE = /(?:^|[^\p{L}\p{N}._~\-/\\:])(?:\/|\.{1,2}[\\/]|~[\\/]|[a-z]:[\\/]|\\\\)/iu;
/** A run of the characters a host name is made of; anything else separates two of them. */
const RUN = /[\p{L}\p{N}.-]+/gu;
/** An IPv6 address in brackets, anywhere in a word, as in `[::1]:3000`. */
const IPV6 = /\[[0-9a-f:.]*:[0-9a-f:.]*\](?::(?<port>\d+))?/giu;
/** A top-level domain: letters, or an internationalized one in Punycode. */
const TLD = /^(?:\p{L}{2,}|xn--[a-z0-9-]+)$/iu;
const IPV4 = /^\d{1,3}(?:\.\d{1,3}){3}$/u;
/** A port right after a host: `:3000`. */
const PORT = /:(\d+)/y;
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
/**
 * Characters nobody sees: zero-width spaces and joiners, the soft hyphen, the word joiner, bidi
 * marks and overrides, variation selectors. In link text they split a host name so that it reads
 * `github.com` while no check recognizes it as one, or turn its letters around.
 */
const IGNORABLE = /\p{Default_Ignorable_Code_Point}/u;
/** A variation selector after a pictograph and a joiner between two draw an emoji. */
const EMOJI_GLUE =
  /(?<=\p{Extended_Pictographic})\uFE0F|(?<=\p{Extended_Pictographic}\uFE0F?)\u200d(?=\p{Extended_Pictographic})/gu;
/** Combining marks: drawn onto the letter before them, so `githu\u0325b` still reads `github`. */
const MARKS = /\p{M}/gu;
/**
 * Characters that look like the dot of a host name (Unicode's confusables, UTS 39): ideographic
 * and fullwidth full stops, the Lisu tone mark, Arabic-Indic zeros, katakana and other middle
 * dots, bullet and dot operators.
 */
const DOT_LIKE = /[\u3002\uFF0E\uFF61\uA4F8\u0660\u06F0\u30FB\uFF65\u00B7\u2027\u2219\u22C5]/gu;
/** Characters that look like a slash: division, fraction and big solidus. */
const SLASH_LIKE = /[\u2215\u2044\u29F8]/gu;
/**
 * Any other sign outside ASCII between two ASCII letters or digits may pass for a dot as well
 * (`github\u2E31com`). Only between ASCII ones, so prose in other scripts keeps its punctuation.
 */
const SIGN_BETWEEN = /(?<=[A-Za-z0-9])[^\p{L}\p{N}\s\p{ASCII}](?=[A-Za-z0-9])/gu;
/** A character outside ASCII right before or after a dot. */
const NON_ASCII_AT_DOT = /\P{ASCII}\.|\.\P{ASCII}/u;
const NON_ASCII = /\P{ASCII}/u;

/** The link text as it reads: look-alikes of the dot and the slash become the real ones. */
function readable(shown: string): string {
  return (
    shown
      .normalize("NFKC")
      .replace(MARKS, "")
      .replace(DOT_LIKE, ".")
      .replace(SLASH_LIKE, "/")
      .replace(SIGN_BETWEEN, ".")
      // `github .com` and `github . com` still read as one host name.
      .replace(/\s+\.\s*/gu, ".")
  );
}

/** Whether a run names a host: `localhost`, an IPv4 address, or labels ending in a TLD. */
function hostLike(run: string): boolean {
  if (run.toLowerCase() === "localhost" || IPV4.test(run)) return true;
  const labels = run.split(".");
  const tld = labels[labels.length - 1] ?? "";
  return labels.length > 1 && TLD.test(tld);
}

/** Whether a run names a file (`README.md`): a name, a dot and one of `FILE_ENDINGS`. */
function isFileName(run: string): boolean {
  const dot = run.lastIndexOf(".");
  return dot > 0 && FILE_ENDINGS.has(run.slice(dot + 1).toLowerCase());
}

/** A host name without a leading `www.`, which names the same site as the bare host. */
function siteOf(hostname: string): string {
  return hostname.startsWith("www.") ? hostname.slice(4) : hostname;
}

/** The port an address reaches, also when it names none (80 for http, 443 for https). */
function portOf(url: URL): string {
  if (url.port !== "") return url.port;
  return url.protocol === "http:" ? "80" : "443";
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

/** Whether `host`, with `port` where one is named, is the target's site and port. */
function sameSite(host: string, port: string | null, target: URL): boolean {
  if (port !== null && port.length > 5) return false;
  let claimed: URL;
  try {
    claimed = new URL(`${target.protocol}//${host}${port === null ? "" : `:${port}`}`);
  } catch {
    return false;
  }
  if (siteOf(claimed.hostname) !== siteOf(target.hostname)) return false;
  return port === null || portOf(claimed) === portOf(target);
}

/** Whether a host run has a character outside ASCII at a dot or at a slash next to it. */
function oddAtSeparator(word: string, core: string, begin: number, end: number): boolean {
  if (NON_ASCII_AT_DOT.test(core)) return true;
  if (word[begin - 1] === "/" && NON_ASCII.test(core.charAt(0))) return true;
  return word[end] === "/" && NON_ASCII.test(core.charAt(core.length - 1));
}

/**
 * Whether one word of the link text names something the target is not. Every host found anywhere
 * in the word counts, wherever it starts or ends (`**github.com**`, `url=github.com`,
 * `git@github.com:org/repo`): each must be the target's site, with the target's port and scheme
 * where the word names them. A file name must be the target's file, a path is never a web
 * address.
 */
function wordMisleads(word: string, target: URL): boolean {
  if (PATH_LIKE.test(word)) return true;
  for (const match of word.matchAll(IPV6)) {
    const host = match[0].slice(0, match[0].indexOf("]") + 1);
    if (!sameSite(host, match.groups?.port ?? null, target)) return true;
  }
  let onTargetSite = false;
  let previous = "";
  for (const match of word.matchAll(RUN)) {
    const raw = match[0];
    const lead = raw.length - raw.replace(/^[.-]+/u, "").length;
    const core = raw.slice(lead).replace(/[.-]+$/u, "");
    const begin = match.index + lead;
    const end = begin + core.length;
    // After `https://` comes a host, whatever it looks like (`https://git`).
    const afterScheme = begin >= 3 && word.startsWith("://", begin - 3);
    const scheme = previous;
    previous = core;
    if (core === "") continue;
    if (word.startsWith("://", end)) {
      // A scheme: only the target's own.
      if (core.toLowerCase() !== target.protocol.slice(0, -1)) return true;
      continue;
    }
    if (afterScheme && scheme.toLowerCase() !== target.protocol.slice(0, -1)) return true;
    // A name in the path behind the target's own host (`vercel/next.js`, `acme/app.git`) is a
    // page of that site, not another host.
    if (onTargetSite && !afterScheme && word[begin - 1] === "/") continue;
    const addressy = word[end] === "/" || /^:\d/u.test(word.slice(end, end + 2));
    if (!afterScheme && isFileName(core)) {
      if (lastPathName(target).toLowerCase() === core.toLowerCase()) continue;
      if (!addressy) return true;
    } else if (!afterScheme && !hostLike(core)) continue;
    if (oddAtSeparator(word, core, begin, end)) return true;
    PORT.lastIndex = end;
    const port = PORT.exec(word)?.[1] ?? null;
    if (!sameSite(core, port, target)) return true;
    onTargetSite = true;
  }
  return false;
}

/**
 * An OSC 8 link shows one text and leads elsewhere: a program can print
 * `https://github.com/org/repo` as the visible text of a link to another host. A click opens at
 * once ("open") only when the visible text names no address at all (plain words, `PR #123`,
 * `v0.8.0`, `Python 3.12`), or when every host it names anywhere is the target's site, with the
 * target's port and scheme where it names them, and every file it names is the target's file.
 * Anything else shows the real target first ("confirm"): another host, a path, characters nobody
 * sees, a sign outside ASCII at a dot of a host, a target host in Punycode.
 *
 * The text is read as a person sees it: compatibility forms folded (NFKC), combining marks
 * dropped, look-alikes of the dot and the slash taken for them. Hosts are searched, not matched
 * against the whole word, so no sign around them (`**`, `|`, `=`, `@`, `~`) hides one.
 *
 * `shown` is the whole word around the link on screen (`linkWord` in terminalLinkWord.ts), so
 * text printed right before or after the link counts too. `target` is the link's address, already
 * checked by `safeLinkUrl`.
 */
export function osc8Decision(shown: string, target: string): Osc8Decision {
  if (IGNORABLE.test(shown.replace(EMOJI_GLUE, ""))) return "confirm";
  let real: URL;
  try {
    real = new URL(target);
  } catch {
    return "confirm";
  }
  // An internationalized host can pass for another one letter by letter.
  if (real.hostname.split(".").some((label) => label.startsWith("xn--"))) return "confirm";
  const misleads = readable(shown)
    .split(/\s+/u)
    .some((word) => word !== "" && wordMisleads(word, real));
  return misleads ? "confirm" : "open";
}
