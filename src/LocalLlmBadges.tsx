// One small badge per local model server in the top bar (ADR 0022): the server's mark and a dot,
// green while it runs and grey while it does not. The tooltip and the accessible name say it in
// words, so the color is never the only sign. Nothing shows until the backend first answered.

import lmstudioSvg from "./assets/local-llm/lmstudio.svg?raw";
import ollamaSvg from "./assets/local-llm/ollama.svg?raw";
import type { LocalServerId } from "./api";
import { useT } from "./i18n";
import { LOCAL_SERVERS, serverText, type LocalLlmState } from "./localLlm";

/** The marks' path data, taken unchanged from the bundled Simple Icons files (NOTICE.md there). */
function pathOf(svg: string): string {
  return /\sd="([^"]+)"/.exec(svg)?.[1] ?? "";
}

const MARKS: Readonly<Record<LocalServerId, string>> = {
  ollama: pathOf(ollamaSvg),
  lmstudio: pathOf(lmstudioSvg),
};

export function LocalLlmBadges({ state }: { state: LocalLlmState | null }) {
  const t = useT();
  if (state === null) return null;
  return (
    <span className="local-llm">
      {LOCAL_SERVERS.map((id) => {
        const server = state[id];
        const text = serverText(id, server, t);
        return (
          <span
            key={id}
            className={server.running ? "local-llm-badge running" : "local-llm-badge"}
            role="img"
            aria-label={text}
            title={text}
            data-server={id}
          >
            <svg viewBox="0 0 24 24" width="13" height="13" fill="currentColor" aria-hidden="true">
              <path d={MARKS[id]} />
            </svg>
            <span className="local-llm-dot" aria-hidden="true" />
          </span>
        );
      })}
    </span>
  );
}
