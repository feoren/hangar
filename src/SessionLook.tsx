// A session's look wherever its program badge stands: the badge, or for a session that runs as a
// sub-agent definition (`subAgent`) the figure's face with the program's badge small on a corner.
// The whole figure where there is room, the face where there is not (`crop`). One place decides it, so the sidebar rows (in every row builder form), the rail, the peek, the
// overview, the split view's pane headers, search and the drag ghost look alike.

import { AgentFigure } from "./AgentFigure";
import { Badge } from "./Badge";
import { folderName } from "./folders";
import { useT } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import { sessionFigure } from "./subAgentLibrary";
import { useSubAgentLibrary } from "./subAgentLibraryStore";

export interface SessionLookProps {
  /** The program's badge. */
  look: BadgeLook;
  /** What decides the figure: the definition's name and the folder the session runs in. */
  session: { subAgent?: string | null; cwd: string };
  /** The program's name for the tooltip; missing: no tooltip. */
  programName?: string;
  /** `full` where the place has room for a whole figure, `head` (default) where it is tight. */
  crop?: "full" | "head";
}

export function SessionLook({ look, session, programName, crop = "head" }: SessionLookProps) {
  const t = useT();
  const { library, figures } = useSubAgentLibrary();
  const agent = session.subAgent ?? null;
  if (agent === null) return <Badge look={look} />;
  const { figure, project } = sessionFigure(library, figures, agent, session.cwd);
  const title =
    programName === undefined
      ? undefined
      : [
          t("subAgentLib.sessionLook", { program: programName, agent }),
          project === null ? null : t("subAgentLib.fromProject", { folder: folderName(project) }),
        ]
          .filter((part) => part !== null)
          .join("\n");
  return (
    <span className={`session-figure ${crop}`} title={title} data-agent={agent}>
      <AgentFigure figure={figure} crop={crop} />
      <span className="figure-corner" aria-hidden="true">
        <Badge look={look} />
      </span>
    </span>
  );
}
