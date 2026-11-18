// The connection test of a self-hosted server (Settings, Account; Rust: server_check.rs, serde
// camelCase): checks the backend's result and turns each of its three steps into one line with a
// mark and, when the step failed, one sentence on what to do.

import { field, isCount } from "./claims";
import type { AccountProblem } from "./account";
import type { MessageKey, Translate } from "./i18n";

export type ApiCheck =
  | { kind: "ok" }
  | { kind: "unreachable" }
  | { kind: "timeout" }
  | { kind: "redirected"; status: number }
  | { kind: "wrongPath" }
  | { kind: "badGateway"; status: number }
  | { kind: "serverError"; status: number }
  | { kind: "notHangar"; status: number };

export type RelayCheck =
  | { kind: "ok"; version: string | null }
  | { kind: "unreachable" }
  | { kind: "timeout" }
  | { kind: "noProxy"; status: number }
  | { kind: "badGateway"; status: number }
  | { kind: "unhealthy" }
  | { kind: "unexpected"; status: number };

export type LiveCheck =
  | { kind: "ok" }
  | { kind: "skipped" }
  | { kind: "unreachable" }
  | { kind: "timeout" }
  | { kind: "missingUpgrade"; status: number }
  | { kind: "noProxy"; status: number }
  | { kind: "badGateway"; status: number }
  | { kind: "busy" }
  | { kind: "unexpected"; status: number | null };

export interface ServerCheck {
  /** The server address the test ran against. */
  baseUrl: string;
  api: ApiCheck;
  relay: RelayCheck;
  live: LiveCheck;
  /** `host` or `host:port` of the relay. */
  relayHost: string;
  /** Whether a ticket of the signed-in account named the relay, or it was derived. */
  relaySource: "ticket" | "derived";
}

export type CheckStep = "api" | "relay" | "live";
export type CheckState = "ok" | "failed" | "skipped";

export interface CheckLine {
  step: CheckStep;
  state: CheckState;
  text: string;
  /** What to do, only for a failed or skipped step. */
  hint: string | null;
}

function statusOf(value: unknown): number | null {
  const status = field(value, "status");
  return isCount(status) ? status : null;
}

function parseApi(value: unknown): ApiCheck | null {
  const kind = field(value, "kind");
  const status = statusOf(value);
  switch (kind) {
    case "ok":
    case "unreachable":
    case "timeout":
    case "wrongPath":
      return { kind };
    case "redirected":
    case "badGateway":
    case "serverError":
    case "notHangar":
      return status === null ? null : { kind, status };
    default:
      return null;
  }
}

function parseRelay(value: unknown): RelayCheck | null {
  const kind = field(value, "kind");
  const status = statusOf(value);
  switch (kind) {
    case "ok": {
      const version = field(value, "version") ?? null;
      return version === null || typeof version === "string" ? { kind, version } : null;
    }
    case "unreachable":
    case "timeout":
    case "unhealthy":
      return { kind };
    case "noProxy":
    case "badGateway":
    case "unexpected":
      return status === null ? null : { kind, status };
    default:
      return null;
  }
}

function parseLive(value: unknown): LiveCheck | null {
  const kind = field(value, "kind");
  const status = statusOf(value);
  switch (kind) {
    case "ok":
    case "skipped":
    case "unreachable":
    case "timeout":
    case "busy":
      return { kind };
    case "missingUpgrade":
    case "noProxy":
    case "badGateway":
      return status === null ? null : { kind, status };
    case "unexpected":
      // `null`: the handshake broke off without a status.
      return (field(value, "status") ?? null) === null || status !== null ? { kind, status } : null;
    default:
      return null;
  }
}

/** Checks the backend's answer; `null` if anything is malformed. */
export function parseServerCheck(value: unknown): ServerCheck | null {
  const api = parseApi(field(value, "api"));
  const relay = parseRelay(field(value, "relay"));
  const live = parseLive(field(value, "live"));
  const baseUrl = field(value, "baseUrl");
  const relayHost = field(value, "relayHost");
  const relaySource = field(value, "relaySource");
  if (api === null || relay === null || live === null || typeof relayHost !== "string") {
    return null;
  }
  if (relaySource !== "ticket" && relaySource !== "derived") return null;
  if (typeof baseUrl !== "string") return null;
  return { baseUrl, api, relay, live, relayHost, relaySource };
}

/** The sentence on what to do for a failure every step can have. */
function commonHint(
  kind: "unreachable" | "timeout" | "badGateway",
  t: Translate,
  status = 0,
): string {
  switch (kind) {
    case "unreachable":
      return t("account.checkHintUnreachable");
    case "timeout":
      return t("account.checkHintTimeout");
    case "badGateway":
      return t("account.checkHintBadGateway", { status });
  }
}

function apiLine(check: ApiCheck, t: Translate): CheckLine {
  if (check.kind === "ok") {
    return { step: "api", state: "ok", text: t("account.checkApiOk"), hint: null };
  }
  let hint: string;
  switch (check.kind) {
    case "unreachable":
    case "timeout":
      hint = commonHint(check.kind, t);
      break;
    case "badGateway":
      hint = commonHint("badGateway", t, check.status);
      break;
    case "redirected":
      hint = t("account.checkHintRedirected", { status: check.status });
      break;
    case "wrongPath":
      hint = t("account.checkHintWrongPath");
      break;
    case "serverError":
      hint = t("account.checkHintServerError", { status: check.status });
      break;
    case "notHangar":
      hint = t("account.checkHintNotHangar", { status: check.status });
      break;
  }
  return { step: "api", state: "failed", text: t("account.checkApiFailed"), hint };
}

function relayLine(check: RelayCheck, host: string, t: Translate): CheckLine {
  if (check.kind === "ok") {
    const text =
      check.version === null
        ? t("account.checkRelayOk", { host })
        : t("account.checkRelayOkVersion", { host, version: check.version });
    return { step: "relay", state: "ok", text, hint: null };
  }
  let hint: string;
  switch (check.kind) {
    case "unreachable":
    case "timeout":
      hint = commonHint(check.kind, t);
      break;
    case "badGateway":
      hint = commonHint("badGateway", t, check.status);
      break;
    case "noProxy":
      hint = t("account.checkHintNoProxy", { status: check.status });
      break;
    case "unhealthy":
      hint = t("account.checkHintUnhealthy");
      break;
    case "unexpected":
      hint = t("account.checkHintRelayStatus", { status: check.status });
      break;
  }
  return { step: "relay", state: "failed", text: t("account.checkRelayFailed", { host }), hint };
}

function liveLine(check: LiveCheck, t: Translate): CheckLine {
  if (check.kind === "ok") {
    return { step: "live", state: "ok", text: t("account.checkLiveOk"), hint: null };
  }
  if (check.kind === "skipped") {
    return {
      step: "live",
      state: "skipped",
      text: t("account.checkLiveSkipped"),
      hint: null,
    };
  }
  let hint: string;
  switch (check.kind) {
    case "unreachable":
    case "timeout":
      hint = commonHint(check.kind, t);
      break;
    case "badGateway":
      hint = commonHint("badGateway", t, check.status);
      break;
    case "missingUpgrade":
      hint = t("account.checkHintMissingUpgrade", { status: check.status });
      break;
    case "noProxy":
      hint = t("account.checkHintNoProxy", { status: check.status });
      break;
    case "busy":
      hint = t("account.checkHintBusy");
      break;
    case "unexpected":
      hint =
        check.status === null
          ? t("account.checkHintHandshake")
          : t("account.checkHintLiveStatus", { status: check.status });
      break;
  }
  return { step: "live", state: "failed", text: t("account.checkLiveFailed"), hint };
}

/** What a test run gave: the result, or why the backend refused or could not run it. */
export type CheckOutcome = { check: ServerCheck } | { problem: AccountProblem };

/**
 * The result to show for the server `baseUrl` now set: a test that ran against another address
 * (the address changed while it ran) shows nothing.
 */
export function checkForServer(check: ServerCheck, baseUrl: string): ServerCheck | null {
  return check.baseUrl === baseUrl ? check : null;
}

/** The sentence for a test that did not run: one already runs, or it failed as a whole. */
export function checkProblemText(problem: AccountProblem, t: Translate): string {
  return problem.kind === "checkRunning"
    ? t("account.checkRunning")
    : t("account.checkUnavailable");
}

/** The three lines of a result, in the order the backend ran the steps. */
export function serverCheckLines(check: ServerCheck, t: Translate): CheckLine[] {
  return [
    apiLine(check.api, t),
    relayLine(check.relay, check.relayHost, t),
    liveLine(check.live, t),
  ];
}

/** Every key the result list can show, for the translation test. */
export const SERVER_CHECK_KEYS: readonly MessageKey[] = [
  "account.checkApiOk",
  "account.checkApiFailed",
  "account.checkRelayOk",
  "account.checkRelayOkVersion",
  "account.checkRelayFailed",
  "account.checkLiveOk",
  "account.checkLiveFailed",
  "account.checkLiveSkipped",
  "account.checkHintUnreachable",
  "account.checkHintTimeout",
  "account.checkHintBadGateway",
  "account.checkHintRedirected",
  "account.checkHintWrongPath",
  "account.checkHintServerError",
  "account.checkHintNotHangar",
  "account.checkHintNoProxy",
  "account.checkHintUnhealthy",
  "account.checkHintRelayStatus",
  "account.checkHintMissingUpgrade",
  "account.checkHintBusy",
  "account.checkHintHandshake",
  "account.checkHintLiveStatus",
  "account.checkRunning",
  "account.checkUnavailable",
];
