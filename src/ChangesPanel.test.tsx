import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { CommitCheckNote, CommitSecretAsk, DiscardDialog } from "./ChangesPanel";
import { createTranslator, I18nContext } from "./i18n";

describe("DiscardDialog", () => {
  it("names the folder the question was asked for", () => {
    const t = createTranslator("de");
    const html = renderToStaticMarkup(
      <I18nContext.Provider value={t}>
        <DiscardDialog
          entry={{
            path: "src/app.ts",
            status: " M",
            additions: 1,
            deletions: 0,
            kind: "modified",
            diff: null,
            hidden: false,
          }}
          cwd={"C:\\Work\\shop"}
          onCancel={() => undefined}
          onConfirm={() => undefined}
        />
      </I18nContext.Provider>,
    );
    expect(html).toContain(t("changes.inFolder", { folder: "shop" }));
    expect(html).toContain('title="C:\\Work\\shop"');
  });
});

describe("CommitSecretAsk", () => {
  const t = createTranslator("de");
  const render = (ask: Parameters<typeof CommitSecretAsk>[0]["ask"]) =>
    renderToStaticMarkup(
      <I18nContext.Provider value={t}>
        <CommitSecretAsk
          ask={ask}
          busy={false}
          onCancel={() => undefined}
          onCommitAnyway={() => undefined}
        />
      </I18nContext.Provider>,
    );
  const tree = "a1".repeat(20);

  it("lists files and kinds and offers to commit the checked tree anyway", () => {
    const html = render({
      findings: [
        { path: "src/config.rs", kinds: ["githubToken", "stripeKey"] },
        { path: ".env", kinds: ["secretFile"] },
      ],
      more: 2,
      complete: true,
      tree,
    });
    expect(html).toContain(t("changes.secretTitle"));
    expect(html).toContain("src/config.rs");
    expect(html).toContain(`${t("secret.githubToken")}, ${t("secret.stripeKey")}`);
    expect(html).toContain(t("secret.secretFile"));
    expect(html).toContain(t("changes.secretMore", { count: "2" }));
    expect(html).toContain(t("changes.commitAnyway"));
    expect(html).toContain(t("common.cancel"));
    expect(html).not.toContain(t("changes.secretPartial"));
    expect(html).not.toContain(t("changes.secretNoTree"));
  });

  it("offers only Cancel and says why when git could not name the tree", () => {
    const html = render({
      findings: [{ path: "a.txt", kinds: ["privateKey"] }],
      more: 0,
      complete: false,
      tree: null,
    });
    expect(html).toContain(t("changes.secretNoTree"));
    expect(html).toContain(t("changes.secretPartial"));
    expect(html).toContain(t("common.cancel"));
    expect(html).not.toContain(t("changes.commitAnyway"));
    expect(html).not.toContain(t("changes.secretMore", { count: "0" }));
  });

  it("keeps the warning without an empty list when no finding was readable", () => {
    const html = render({ findings: [], more: 0, complete: true, tree });
    expect(html).toContain(t("changes.secretTitle"));
    expect(html).not.toContain("<ul");
    expect(html).toContain(t("changes.commitAnyway"));
  });
});

describe("CommitCheckNote", () => {
  it("says how far a commit was checked", () => {
    const t = createTranslator("de");
    const note = (checked: "partial" | "skipped") =>
      renderToStaticMarkup(
        <I18nContext.Provider value={t}>
          <CommitCheckNote checked={checked} />
        </I18nContext.Provider>,
      );
    expect(note("partial")).toContain(t("changes.checkedPartial"));
    expect(note("skipped")).toContain(t("changes.checkedSkipped"));
    expect(note("partial")).toContain('role="status"');
  });
});
