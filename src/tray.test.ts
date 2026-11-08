import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import { trayLabels, TrayState } from "./tray";

const labels = { open: "Open", quit: "Quit", tooltip: "Hangar" };

function deferred() {
  let resolve: () => void = () => undefined;
  let reject: (error: Error) => void = () => undefined;
  const promise = new Promise<void>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe("trayLabels", () => {
  it("names the waiting sessions in the tooltip", () => {
    const t = createTranslator("en");
    expect(trayLabels(t, 0)).toEqual({
      open: t("tray.open"),
      quit: t("menu.quit"),
      tooltip: "Hangar",
    });
    expect(trayLabels(t, 3).tooltip).toBe(t("tray.tooltipWaiting", { count: 3 }));
  });
});

describe("TrayState", () => {
  it("is active only after the icon was added", async () => {
    const call = deferred();
    const tray = new TrayState(() => call.promise);
    const done = tray.configure(true, labels);
    expect(tray.active).toBe(false);
    call.resolve();
    await done;
    expect(tray.active).toBe(true);
  });

  it("stays inactive when adding the icon fails", async () => {
    const tray = new TrayState(() => Promise.reject(new Error("no tray")));
    await tray.configure(true, labels);
    expect(tray.active).toBe(false);
  });

  it("turns off at once, even before the backend answers", async () => {
    const off = deferred();
    const tray = new TrayState((enabled) => (enabled ? Promise.resolve() : off.promise));
    await tray.configure(true, labels);
    const pending = tray.configure(false, labels);
    expect(tray.active).toBe(false);
    off.resolve();
    await pending;
    expect(tray.active).toBe(false);
  });

  it("ignores a late success of an older call after turning off", async () => {
    const first = deferred();
    const calls = [first.promise, Promise.resolve()];
    const tray = new TrayState(() => calls.shift() ?? Promise.resolve());
    const on = tray.configure(true, labels);
    await tray.configure(false, labels);
    first.resolve();
    await on;
    expect(tray.active).toBe(false);
  });

  it("drops to inactive when an update of the texts fails", async () => {
    const calls = [Promise.resolve(), Promise.reject(new Error("menu"))];
    const tray = new TrayState(() => calls.shift() ?? Promise.resolve());
    await tray.configure(true, labels);
    await tray.configure(true, labels);
    expect(tray.active).toBe(false);
  });
});
