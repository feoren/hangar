import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { createTranslator, I18nContext } from "./i18n";
import { SessionLook } from "./SessionLook";

const LOOK = { kind: "letters", text: "C", color: "#d97757" } as const;

function render(element: ReactElement): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={createTranslator("en")}>{element}</I18nContext.Provider>,
  );
}

describe("SessionLook", () => {
  it("is the badge without a sub-agent", () => {
    const html = render(<SessionLook look={LOOK} session={{ cwd: "C:\\src" }} />);
    expect(html).toBe('<span class="badge" style="background:#d97757">C</span>');
  });

  it("is the figure's face with the badge on a corner for a sub-agent session", () => {
    const html = render(
      <SessionLook
        look={LOOK}
        session={{ subAgent: "tester", cwd: "C:\\src" }}
        programName="Claude Code"
        busy
      />,
    );
    expect(html).toContain('title="Claude Code as tester"');
    expect(html).toContain('class="agent-figure head busy"');
    expect(html).toContain('<span class="figure-corner" aria-hidden="true"><span class="badge"');
  });
});
