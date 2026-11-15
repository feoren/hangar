import { describe, expect, it } from "vitest";

import {
  DevServerDetector,
  devServerUrl,
  findDevServerUrl,
  lastDevServerUrl,
  stripAnsi,
} from "./devServer";

describe("stripAnsi", () => {
  it("removes colors, OSC titles and two-byte escapes", () => {
    expect(stripAnsi("\x1b[32mok\x1b[0m \x1b]0;title\x07done\x1b7")).toBe("ok done");
  });
});

describe("devServerUrl", () => {
  it("maps wildcard hosts to localhost", () => {
    expect(devServerUrl("http", "0.0.0.0", "3000")).toBe("http://localhost:3000/");
    expect(devServerUrl("http", "[::]", "3000")).toBe("http://localhost:3000/");
  });

  it("keeps loopback hosts", () => {
    expect(devServerUrl("HTTP", "127.0.0.1", "8000")).toBe("http://127.0.0.1:8000/");
    expect(devServerUrl("https", "[::1]", "8443")).toBe("https://[::1]:8443/");
  });

  it("refuses impossible ports", () => {
    expect(devServerUrl("http", "localhost", "0")).toBeNull();
    expect(devServerUrl("http", "localhost", "70000")).toBeNull();
  });
});

describe("lastDevServerUrl", () => {
  it("finds the Vite line", () => {
    expect(lastDevServerUrl("  Local:   http://localhost:5173/\n")).toBe("http://localhost:5173/");
  });

  it("keeps only the origin, never a path", () => {
    expect(lastDevServerUrl("Listening on http://0.0.0.0:8080/api.\n")).toBe(
      "http://localhost:8080/",
    );
    expect(lastDevServerUrl("curl http://localhost:3000/api/users?id=1\n")).toBe(
      "http://localhost:3000/",
    );
  });

  it("takes the most recent address", () => {
    expect(lastDevServerUrl("http://localhost:3000 then http://localhost:3001\n")).toBe(
      "http://localhost:3001/",
    );
  });

  it("ignores remote hosts, addresses without a port and unfinished ones", () => {
    expect(lastDevServerUrl("see https://example.com:443/x\n")).toBeNull();
    expect(lastDevServerUrl("http://localhost/\n")).toBeNull();
    expect(lastDevServerUrl("http://localhost:51")).toBeNull();
  });
});

describe("findDevServerUrl", () => {
  const filler = (length: number) => "x".repeat(length);
  const colored = (length: number) => "\x1b[38;2;10;20;30m.".repeat(length);
  const cases: string[] = [
    "",
    "plain output without any address\n",
    `${filler(5000)}  Local:   http://localhost:5173/\n${filler(5000)}`,
    `${filler(300)}http://localhost:3000 ${filler(300)} http://127.0.0.1:4000/ ${filler(300)}`,
    // Colored port, escapes all around, an OSC 8 hyperlink whose target alone holds a "://".
    `${colored(400)}http://localhost:\x1b[1m5173\x1b[22m/${colored(400)}`,
    `${filler(1000)}\x1b]8;;http://localhost:9999/\x1b\\docs\x1b]8;;\x1b\\${filler(1000)}`,
    `\x1b]8;;https://example.com/\x1b\\link\x1b]8;;\x1b\\ then http://[::1]:8443/x`,
    // Escapes piled against the scheme and the port: the window would cut them.
    `${filler(500)}a${"\x1b[0m".repeat(60)}http://localhost:${"\x1b[1m".repeat(60)}8080/${filler(500)}`,
    `${filler(500)}${"\x1b[0m".repeat(60)}http://localhost:8080/`,
    `wordhttp://localhost:3000/ ${filler(500)}`,
    // An unfinished port at the very end, and one in the middle followed by more output.
    `${filler(500)}http://localhost:51`,
    `${filler(500)}http://localhost:51${filler(500)}`,
    `${filler(500)}http://localhost:5173`,
    `${filler(500)}http://localhost:5173 `,
    `${filler(500)}http://localhost:${"\x1b[0m".repeat(40)}5173`,
    // An OSC that is never terminated runs on to the next escape, as stripAnsi has it.
    `${filler(500)}\x1b]0;title http://localhost:3000/ ${filler(500)}`,
    `${filler(500)}http://localhost:3000/ \x1b]0;title ${filler(500)} http://localhost:3001/`,
    `${filler(198)}https://localhost:3000/${filler(198)}`,
    `${filler(195)}\x1b[1mhttps\x1b[0m://localhost:3000/`,
  ];

  it("gives the same address as stripping and searching the whole text", () => {
    for (const text of cases) {
      expect(findDevServerUrl(text)).toBe(lastDevServerUrl(stripAnsi(text)));
    }
  });

  it("finds an address within a long chunk", () => {
    const text = `${filler(200_000)}ready at http://0.0.0.0:8080/${filler(50_000)}`;
    expect(findDevServerUrl(text)).toBe("http://localhost:8080/");
  });
});

describe("DevServerDetector", () => {
  it("finds an address with a colored port", () => {
    const detector = new DevServerDetector();
    expect(
      detector.feed("  \x1b[32mLocal\x1b[39m:   http://localhost:\x1b[1m5173\x1b[22m/\r\n"),
    ).toBe(true);
    expect(detector.url).toBe("http://localhost:5173/");
  });

  it("joins an address split between chunks", () => {
    const detector = new DevServerDetector();
    expect(detector.feed("ready at http://localhost:30")).toBe(false);
    expect(detector.feed("00/app\r\n")).toBe(true);
    expect(detector.url).toBe("http://localhost:3000/");
  });

  it("joins an address split at the end of a long chunk", () => {
    const detector = new DevServerDetector();
    expect(detector.feed(`${"x".repeat(100_000)}\r\n  Local:   http://localhost:51`)).toBe(false);
    expect(detector.feed(`73/\r\n${"y".repeat(100_000)}`)).toBe(true);
    expect(detector.url).toBe("http://localhost:5173/");
  });

  it("joins an address whose separator is split between chunks", () => {
    const detector = new DevServerDetector();
    expect(detector.feed(`${"x".repeat(100_000)} http:/`)).toBe(false);
    expect(detector.feed(`/127.0.0.1:4000/ ${"y".repeat(100_000)}`)).toBe(true);
    expect(detector.url).toBe("http://127.0.0.1:4000/");
  });

  it("reports only changes", () => {
    const detector = new DevServerDetector();
    expect(detector.feed("http://localhost:3000/\n")).toBe(true);
    expect(detector.feed("compiled\n")).toBe(false);
    expect(detector.feed("http://localhost:3000/\n")).toBe(false);
    expect(detector.feed("http://127.0.0.1:4000/\n")).toBe(true);
    expect(detector.url).toBe("http://127.0.0.1:4000/");
  });

  it("reports one origin once, however many paths are logged", () => {
    const detector = new DevServerDetector();
    let reports = 0;
    for (let index = 0; index < 1000; index += 1) {
      if (detector.feed(`GET http://localhost:3000/api/items/${String(index)} 200\n`)) {
        reports += 1;
      }
    }
    expect(reports).toBe(1);
    expect(detector.url).toBe("http://localhost:3000/");
    expect(detector.feed("GET http://localhost:3001/api/items/1 200\n")).toBe(true);
    expect(detector.url).toBe("http://localhost:3001/");
  });

  it("forgets the address on reset", () => {
    const detector = new DevServerDetector();
    detector.feed("http://localhost:3000/\n");
    expect(detector.reset()).toBe(true);
    expect(detector.url).toBeNull();
    expect(detector.reset()).toBe(false);
    expect(detector.feed("plain\n")).toBe(false);
  });
});
