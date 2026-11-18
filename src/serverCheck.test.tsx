import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { ServerCheckList, ServerCheckOutcome } from "./AccountSettings";
import { createTranslator, DICTIONARIES, I18nContext, LANGUAGES, placeholders } from "./i18n";
import {
  checkForServer,
  parseServerCheck,
  SERVER_CHECK_KEYS,
  serverCheckLines,
  type CheckOutcome,
  type ServerCheck,
} from "./serverCheck";

const BASE = "https://team.example.test/api/v1";
const en = createTranslator("en");
const de = createTranslator("de");

const healthy: ServerCheck = {
  baseUrl: BASE,
  api: { kind: "ok" },
  relay: { kind: "ok", version: "0.4.1" },
  live: { kind: "ok" },
  relayHost: "team.example.test",
  relaySource: "derived",
};

/** The case of the report: nginx without a rule for `/`, so `/health` is its HTML 404 page. */
const missingRule: ServerCheck = {
  baseUrl: BASE,
  api: { kind: "ok" },
  relay: { kind: "noProxy", status: 404 },
  live: { kind: "skipped" },
  relayHost: "team.example.test",
  relaySource: "derived",
};

describe("parseServerCheck", () => {
  it("reads the backend's answer", () => {
    expect(parseServerCheck(healthy)).toEqual(healthy);
    expect(parseServerCheck(missingRule)).toEqual(missingRule);
    const broken = {
      ...healthy,
      live: { kind: "unexpected", status: null },
      relay: { kind: "ok" },
    };
    expect(parseServerCheck(broken)).toEqual({
      ...healthy,
      live: { kind: "unexpected", status: null },
      relay: { kind: "ok", version: null },
    });
  });

  it("refuses anything malformed", () => {
    expect(parseServerCheck(null)).toBeNull();
    expect(parseServerCheck({ ...healthy, api: { kind: "fine" } })).toBeNull();
    expect(parseServerCheck({ ...healthy, relay: { kind: "noProxy" } })).toBeNull();
    expect(parseServerCheck({ ...healthy, live: { kind: "unexpected", status: "x" } })).toBeNull();
    expect(parseServerCheck({ ...healthy, relaySource: "guess" })).toBeNull();
    expect(parseServerCheck({ ...healthy, relayHost: 3 })).toBeNull();
    expect(parseServerCheck({ ...healthy, baseUrl: undefined })).toBeNull();
  });
});

describe("ServerCheckOutcome", () => {
  function render(outcome: CheckOutcome | null, baseUrl: string): string {
    return renderToStaticMarkup(
      <I18nContext.Provider value={en}>
        <ServerCheckOutcome outcome={outcome} baseUrl={baseUrl} />
      </I18nContext.Provider>,
    );
  }

  it("shows a result only while the server it ran against is set", () => {
    expect(checkForServer(healthy, BASE)).toBe(healthy);
    expect(checkForServer(healthy, "https://other.example.test/api/v1")).toBeNull();
    expect(render({ check: healthy }, BASE)).toContain("Account API answers");
    // The address changed while the test ran: the old server's result stays hidden.
    expect(render({ check: healthy }, "https://other.example.test/api/v1")).toBe("");
    expect(render(null, BASE)).toBe("");
  });

  it("says when a test already runs or could not run at all", () => {
    expect(render({ problem: { kind: "checkRunning", detail: null } }, BASE)).toContain(
      "A connection test is already running.",
    );
    expect(render({ problem: { kind: "badResponse", detail: null } }, BASE)).toContain(
      "The test could not run.",
    );
  });
});

describe("serverCheckLines", () => {
  it("marks every step of a healthy server as fine, with the relay's version", () => {
    const lines = serverCheckLines(healthy, en);
    expect(lines.map((line) => line.state)).toEqual(["ok", "ok", "ok"]);
    expect(lines[1]?.text).toBe("Relay answers (team.example.test, version 0.4.1)");
    expect(lines.every((line) => line.hint === null)).toBe(true);
  });

  it("names the missing proxy rule and what to do", () => {
    const lines = serverCheckLines(missingRule, de);
    expect(lines.map((line) => line.state)).toEqual(["ok", "failed", "skipped"]);
    expect(lines[1]?.hint).toBe(
      "Die Weiterleitung zum Relay fehlt (HTTP 404). Prüfe die Proxyregel für / auf den Containerport.",
    );
  });

  it("tells missing upgrade headers and a timeout apart", () => {
    const lines = serverCheckLines(
      {
        ...healthy,
        api: { kind: "timeout" },
        live: { kind: "missingUpgrade", status: 400 },
      },
      en,
    );
    expect(lines[0]?.state).toBe("failed");
    expect(lines[0]?.hint).toContain("No answer in time");
    expect(lines[2]?.hint).toContain("Upgrade and Connection");
    expect(lines[2]?.hint).toContain("HTTP 400");
  });

  it("has every text in every language with the same placeholders", () => {
    for (const language of LANGUAGES) {
      for (const key of SERVER_CHECK_KEYS) {
        const text = DICTIONARIES[language][key];
        expect(text, `${language} ${key}`).not.toBe("");
        expect(placeholders(text).sort(), `${language} ${key}`).toEqual(
          placeholders(DICTIONARIES.en[key]).sort(),
        );
      }
    }
  });
});

describe("ServerCheckList", () => {
  it("shows one line per step with its mark and the hint of a failure", () => {
    const html = renderToStaticMarkup(
      <ServerCheckList lines={serverCheckLines(missingRule, en)} />,
    );
    expect(html.match(/<li /g)?.length).toBe(3);
    expect(html).toContain('class="server-check-line server-check-ok"');
    expect(html).toContain('class="server-check-line server-check-failed"');
    expect(html).toContain('class="server-check-line server-check-skipped"');
    expect(html).toContain("Account API answers");
    expect(html).toContain("Relay does not answer correctly (team.example.test)");
    expect(html).toContain("The forwarding to the relay is missing (HTTP 404).");
    expect(html).toContain("Live connection can be tested once the relay answers");
    // A mark for the two decided steps, none for the skipped one.
    expect(html.match(/<svg /g)?.length).toBe(2);
  });
});
