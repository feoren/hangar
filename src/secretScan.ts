// Secret guard (ADR 0015), the window's side: the same few shapes of keys and tokens as
// src-tauri/src/secret_scan.rs, for text the person pastes. Both sides check the shared vectors in
// src-tauri/tests/data/secret-scan-vectors.json. The result is only the kinds found, never the
// matching text. Pure.

import type { MessageKey, Translate } from "./i18n";

/** A kind of secret found in text, as the backend names it. */
export type TextKind =
  | "privateKey"
  | "awsAccessKey"
  | "githubToken"
  | "anthropicKey"
  | "openaiKey"
  | "slackToken"
  | "stripeKey";

/** Every kind the backend sends: the text kinds and a staged file named like a secret. */
export type SecretKind = TextKind | "secretFile";

/** What the window shows: also "unknown" when the text could not be checked (`scanSecrets`). */
export type ShownKind = SecretKind | "unknown";

/** Every text kind, in the backend's order. */
export const TEXT_KINDS: readonly TextKind[] = [
  "privateKey",
  "awsAccessKey",
  "githubToken",
  "anthropicKey",
  "openaiKey",
  "slackToken",
  "stripeKey",
];

/** Every kind the backend sends, in its order. */
export const SECRET_KINDS: readonly SecretKind[] = [...TEXT_KINDS, "secretFile"];

/**
 * Longest token body that counts, as in `secret_scan.rs` (`MAX_TOKEN_BODY`). It also bounds the
 * regular expressions: an open-ended repetition over megabytes of text exhausts the engine's
 * stack.
 */
export const MAX_TOKEN_BODY = 512;

/** A random-looking token body, not a placeholder (`secret_scan.rs` `plausible`). */
function plausible(body: string, minDistinct: number): boolean {
  if (new Set(body).size < minDistinct) return false;
  const lower = body.toLowerCase();
  return !["xxxx", "example", "placeholder", "redacted"].some((word) => lower.includes(word));
}

/** Whether any match of `pattern` (global) has a plausible first group that passes `extra`. */
function anyBody(
  text: string,
  pattern: RegExp,
  minDistinct: number,
  extra: (body: string) => boolean = () => true,
): boolean {
  for (const match of text.matchAll(pattern)) {
    const body = match[1] ?? "";
    if (plausible(body, minDistinct) && extra(body)) return true;
  }
  return false;
}

const PEM = /-----BEGIN ([A-Z0-9 ]{0,96})-----([\s\S]{0,512})/g;
const PEM_BODY = /[A-Za-z0-9+/=]{40}/;

function privateKey(text: string): boolean {
  for (const match of text.matchAll(PEM)) {
    const label = match[1] ?? "";
    if (!label.endsWith("PRIVATE KEY") && !label.endsWith("PRIVATE KEY BLOCK")) continue;
    if (PEM_BODY.test(match[2] ?? "")) return true;
  }
  return false;
}

const AWS = /(?<![A-Za-z0-9])(?:AKIA|ASIA|ABIA|ACCA)([A-Z0-9]{16})(?![A-Za-z0-9])/g;
const GITHUB_CLASSIC = /(?<![A-Za-z0-9_])gh[pousr]_([A-Za-z0-9]{36,512})(?![A-Za-z0-9_])/g;
const GITHUB_FINE = /(?<![A-Za-z0-9_])github_pat_([A-Za-z0-9_]{60,512})(?![A-Za-z0-9_])/g;
const ANTHROPIC = /(?<![A-Za-z0-9_-])sk-ant-([A-Za-z0-9_-]{40,512})(?![A-Za-z0-9_-])/g;
const OPENAI =
  /(?<![A-Za-z0-9_-])sk-(?:proj|svcacct|admin)-([A-Za-z0-9_-]{40,512})(?![A-Za-z0-9_-])/g;
const OPENAI_LEGACY = /(?<![A-Za-z0-9_-])sk-([A-Za-z0-9]{40,512})(?![A-Za-z0-9_-])/g;
const SLACK = /(?<![A-Za-z0-9_-])(?:xox[abposr]|xapp)-([A-Za-z0-9-]{20,512})(?![A-Za-z0-9-])/g;
const STRIPE = /(?<![A-Za-z0-9_])[sr]k_live_([A-Za-z0-9]{24,512})(?![A-Za-z0-9])/g;

const CHECKS: Record<TextKind, (text: string) => boolean> = {
  privateKey,
  awsAccessKey: (text) => anyBody(text, AWS, 8, (body) => !body.endsWith("EXAMPLE")),
  githubToken: (text) => anyBody(text, GITHUB_CLASSIC, 10) || anyBody(text, GITHUB_FINE, 10),
  anthropicKey: (text) => anyBody(text, ANTHROPIC, 10),
  openaiKey: (text) =>
    anyBody(text, OPENAI, 10) ||
    anyBody(text, OPENAI_LEGACY, 10, (body) => body.includes("T3BlbkFJ")),
  slackToken: (text) =>
    anyBody(text, SLACK, 10, (body) => body.includes("-") && /[0-9]/.test(body)),
  stripeKey: (text) => anyBody(text, STRIPE, 10),
};

/**
 * The kinds of secret `text` seems to hold, each once, in backend order; empty for none. Should
 * the check itself fail (an engine limit on huge text), the answer is "unknown": better one
 * question too many than a key pasted unasked.
 */
export function scanSecrets(text: string): ShownKind[] {
  try {
    return TEXT_KINDS.filter((kind) => CHECKS[kind](text));
  } catch {
    return ["unknown"];
  }
}

/** Whether `value` is a kind the backend may send. */
export function isSecretKind(value: unknown): value is SecretKind {
  return SECRET_KINDS.some((kind) => kind === value);
}

/** The known kinds of a backend list; anything else is left out. */
export function secretKinds(value: unknown): SecretKind[] {
  return Array.isArray(value) ? value.filter(isSecretKind) : [];
}

const LABELS: Record<ShownKind, MessageKey> = {
  privateKey: "secret.privateKey",
  awsAccessKey: "secret.awsAccessKey",
  githubToken: "secret.githubToken",
  anthropicKey: "secret.anthropicKey",
  openaiKey: "secret.openaiKey",
  slackToken: "secret.slackToken",
  stripeKey: "secret.stripeKey",
  secretFile: "secret.secretFile",
  unknown: "secret.unknown",
};

/** The kinds as a list in the window's language ("GitHub token, private key"). */
export function secretLabels(kinds: readonly ShownKind[], t: Translate): string {
  return kinds.map((kind) => t(LABELS[kind])).join(", ");
}
