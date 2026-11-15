import { Fragment, useEffect, useState } from "react";

import { api, type Profile } from "./api";
import {
  ARENA_MAX,
  arenaProblem,
  canApply,
  clockText,
  contestantState,
  elapsedMs,
  leader,
  parseDiffStat,
  type ApplyOutcome,
  type ArenaRun,
  type ContestantState,
  type DiffStat,
  type Standing,
  type TestState,
} from "./arena";
import type { AttentionState } from "./attention";
import { Badge } from "./Badge";
import { Dialog } from "./Dialog";
import { folderName } from "./folders";
import { shortPath } from "./format";
import { useT, type MessageKey } from "./i18n";
import { CloseIcon } from "./icons";
import type { BadgeLook } from "./programPrefs";
import type { SessionView } from "./sessions";
import { useClock } from "./useClock";

/** What the dialog hands over to start an arena. */
export interface ArenaInput {
  prompt: string;
  profileIds: string[];
  folder: string;
  testCommand: string;
}

interface Props {
  /** Installed agents that can take part (shells are left out). */
  agents: readonly Profile[];
  /** Folders inside a git repository, the start target first. */
  folders: readonly string[];
  badgeFor: (profileId: string) => BadgeLook;
  /** The running arena; `null` shows the start form. */
  arena: ArenaRun | null;
  sessions: readonly SessionView[];
  attention: ReadonlyMap<number, AttentionState>;
  /** Starts the arena and closes the dialog, or answers why it could not start. */
  onStart: (input: ArenaInput) => Promise<string | null>;
  onShow: (sessionId: number) => void;
  onResend: (sessionId: number) => void;
  onRunTests: (sessionId: number) => void;
  /** Takes the winner over; a text when it failed outright. */
  onApply: (sessionId: number) => Promise<ApplyOutcome | string>;
  /** Closes the other contestants and deletes their worktrees with all changes. */
  onCloseOthers: (winnerId: number) => void;
  onClose: () => void;
}

/** How often the changes of the contestants are counted again while the comparison is open. */
const STATS_INTERVAL_MS = 4000;

const STATE_TEXT: Record<ContestantState, MessageKey> = {
  starting: "arena.stateStarting",
  working: "arena.stateWorking",
  done: "arena.stateDone",
  ended: "arena.stateEnded",
};

type Stats = ReadonlyMap<number, DiffStat | "error">;

/** After "Übernehmen": the result, and for a success the offer to close the others. */
type Applied =
  | { kind: "applied"; winnerId: number }
  | { kind: "conflict"; files: string[] }
  | { kind: "error"; message: string };

/**
 * The agent arena: a form to start one task with 2 to 4 agents, and once it runs, the comparison
 * of the contestants with their state, time, changes and tests, and "Take over" for the winner.
 */
export function ArenaDialog(props: Props) {
  const t = useT();
  const [setup, setSetup] = useState(props.arena === null);
  const running = setup ? null : props.arena;
  const showSetup = running === null;
  return (
    <Dialog labelledBy="arena-title" className="arena" onClose={props.onClose}>
      <div className="dialog-header">
        <h2 id="arena-title">{showSetup ? t("arena.title") : t("arena.compare")}</h2>
        <button
          type="button"
          className="icon-button"
          aria-label={t("common.close")}
          title={t("common.close")}
          onClick={props.onClose}
        >
          <CloseIcon />
        </button>
      </div>
      {running === null ? (
        <ArenaSetupForm {...props} />
      ) : (
        <ArenaCompare
          {...props}
          arena={running}
          onNew={() => {
            setSetup(true);
          }}
        />
      )}
    </Dialog>
  );
}

function ArenaSetupForm({ agents, folders, badgeFor, onStart, onClose }: Props) {
  const t = useT();
  const [prompt, setPrompt] = useState("");
  const [chosen, setChosen] = useState<readonly string[]>(() =>
    agents.slice(0, 2).map((agent) => agent.id),
  );
  const [folder, setFolder] = useState<string | null>(folders[0] ?? null);
  const [testCommand, setTestCommand] = useState("");
  const [starting, setStarting] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  // Keep the order of the list, whatever order the boxes were ticked in.
  const picked = agents.filter((agent) => chosen.includes(agent.id)).map((agent) => agent.id);
  const ready = arenaProblem(prompt, picked.length, folder) === null && !starting;

  const start = () => {
    if (!ready || folder === null) return;
    setStarting(true);
    setProblem(null);
    void onStart({ prompt, profileIds: picked, folder, testCommand: testCommand.trim() }).then(
      (answer) => {
        setStarting(false);
        setProblem(answer);
      },
    );
  };

  return (
    <>
      <p className="muted arena-intro">{t("arena.intro")}</p>
      <label className="arena-field">
        <span>{t("arena.task")}</span>
        <textarea
          className="text-input arena-task"
          placeholder={t("arena.taskPlaceholder")}
          rows={4}
          value={prompt}
          spellCheck={false}
          data-autofocus
          onChange={(event) => {
            setPrompt(event.currentTarget.value);
            setProblem(null);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && event.ctrlKey) {
              event.preventDefault();
              start();
            }
          }}
        />
      </label>
      <fieldset className="settings-group arena-agents">
        <legend>{t("arena.agents")}</legend>
        {agents.length < 2 ? (
          <p className="muted">{t("arena.fewAgents")}</p>
        ) : (
          <ul className="arena-agent-list">
            {agents.map((agent) => {
              const on = chosen.includes(agent.id);
              return (
                <li key={agent.id}>
                  <label className="check arena-agent">
                    <input
                      type="checkbox"
                      checked={on}
                      disabled={!on && picked.length >= ARENA_MAX}
                      onChange={(event) => {
                        const checked = event.currentTarget.checked;
                        setChosen((current) =>
                          checked
                            ? [...current, agent.id]
                            : current.filter((id) => id !== agent.id),
                        );
                      }}
                    />
                    <Badge look={badgeFor(agent.id)} />
                    <span>{agent.name}</span>
                  </label>
                </li>
              );
            })}
          </ul>
        )}
      </fieldset>
      <label className="arena-field">
        <span>{t("arena.folder")}</span>
        {folders.length === 0 ? (
          <span className="muted">{t("arena.noFolder")}</span>
        ) : (
          <select
            className="select"
            value={folder ?? ""}
            title={folder ?? undefined}
            onChange={(event) => {
              setFolder(event.currentTarget.value);
            }}
          >
            {folders.map((path) => (
              <option key={path} value={path}>
                {`${folderName(path)} (${shortPath(path)})`}
              </option>
            ))}
          </select>
        )}
      </label>
      <label className="arena-field">
        <span>{t("arena.tests")}</span>
        <input
          className="text-input"
          placeholder={t("arena.testsPlaceholder")}
          value={testCommand}
          spellCheck={false}
          aria-describedby="arena-tests-help"
          onChange={(event) => {
            setTestCommand(event.currentTarget.value);
            setProblem(null);
          }}
        />
        <span id="arena-tests-help" className="muted arena-help">
          {t("arena.testsHelp")}
        </span>
      </label>
      <p className="muted arena-help">{t("arena.sendNote")}</p>
      {problem !== null && (
        <p className="arena-problem" role="alert">
          {problem}
        </p>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={onClose}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button" disabled={!ready} onClick={start}>
          {starting ? t("arena.starting") : t("arena.start")}
        </button>
      </div>
    </>
  );
}

function testText(tests: TestState, t: ReturnType<typeof useT>): string {
  switch (tests.kind) {
    case "none":
      return t("arena.testsNone");
    case "running":
      return t("arena.testsRunning");
    case "error":
      return t("arena.testsError", { error: tests.message });
    case "done":
      if (tests.run.timedOut) return t("arena.testsTimedOut");
      return tests.run.passed ? t("arena.testsPassed") : t("arena.testsFailed");
  }
}

function testClass(tests: TestState): string {
  if (tests.kind === "error") return "failed";
  if (tests.kind !== "done") return "";
  return tests.run.passed ? "passed" : "failed";
}

function ArenaCompare({
  arena,
  sessions,
  attention,
  badgeFor,
  onShow,
  onResend,
  onRunTests,
  onApply,
  onCloseOthers,
  onNew,
}: Props & { arena: ArenaRun; onNew: () => void }) {
  const t = useT();
  const now = useClock(1000);
  const [stats, setStats] = useState<Stats>(new Map());
  const [open, setOpen] = useState<number | null>(null);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [applying, setApplying] = useState(false);
  const [applied, setApplied] = useState<Applied | null>(null);

  const byId = new Map(sessions.map((session) => [session.id, session]));
  const contestants = arena.contestants.filter((c) => byId.has(c.sessionId));

  // Counted again while the dialog is open: agents keep changing files.
  const keys = contestants.map((c) => `${String(c.sessionId)}:${c.worktree.path}`).join("|");
  useEffect(() => {
    let alive = true;
    const load = () => {
      for (const contestant of contestants) {
        void api
          .arenaDiffStat(contestant.worktree, arena.baseCommit)
          .then((value) => parseDiffStat(value) ?? ("error" as const))
          .catch(() => "error" as const)
          .then((stat) => {
            if (!alive) return;
            setStats((current) => new Map(current).set(contestant.sessionId, stat));
          });
      }
    };
    load();
    const timer = window.setInterval(load, STATS_INTERVAL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
    // `keys` stands for the contestants; the list itself is rebuilt on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys, arena.baseCommit]);

  const rows = contestants.map((contestant) => {
    const session = byId.get(contestant.sessionId);
    const state = contestantState(
      contestant,
      attention.get(contestant.sessionId),
      session?.status === "exited",
    );
    const found = stats.get(contestant.sessionId);
    const stat = found === undefined || found === "error" ? null : found;
    return {
      contestant,
      state,
      stat,
      statFailed: found === "error",
      ms: elapsedMs(arena, contestant, now),
    };
  });
  const standings: Standing[] = rows.map((row) => ({
    sessionId: row.contestant.sessionId,
    state: row.state,
    elapsedMs: row.ms,
    stat: row.stat,
    tests: row.contestant.tests,
  }));
  const lead = leader(standings);
  const confirmRow = rows.find((row) => row.contestant.sessionId === confirming);

  const apply = (sessionId: number) => {
    setApplying(true);
    void onApply(sessionId).then((outcome) => {
      setApplying(false);
      setConfirming(null);
      if (typeof outcome === "string") setApplied({ kind: "error", message: outcome });
      else if (outcome.kind === "conflict") setApplied({ kind: "conflict", files: outcome.files });
      else setApplied({ kind: "applied", winnerId: sessionId });
    });
  };

  return (
    <>
      <p className="muted arena-prompt" title={arena.prompt}>
        {arena.prompt}
      </p>
      <table className="arena-table">
        <thead>
          <tr>
            <th scope="col">{t("arena.agent")}</th>
            <th scope="col">{t("arena.state")}</th>
            <th scope="col">{t("arena.time")}</th>
            <th scope="col">{t("arena.files")}</th>
            <th scope="col">{t("arena.lines")}</th>
            {arena.testCommand !== "" && <th scope="col">{t("arena.testsColumn")}</th>}
            {/* Cost column: add it here once a per-session cost API exists on the backend
                (tokens and cost per session); until then it stays out instead of showing
                empty cells. */}
            <th scope="col">
              <span className="sr-only">{t("arena.apply")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map(({ contestant, state, stat, statFailed, ms }) => {
            const id = contestant.sessionId;
            const expanded = open === id;
            const tests = contestant.tests;
            return (
              <Fragment key={id}>
                <tr className={id === lead ? "arena-row leading" : "arena-row"}>
                  <td>
                    <button
                      type="button"
                      className="arena-agent-name"
                      title={contestant.worktree.branch}
                      onClick={() => {
                        onShow(id);
                      }}
                    >
                      <Badge look={badgeFor(contestant.profileId)} />
                      <span>{contestant.name}</span>
                    </button>
                    {id === lead && <span className="arena-lead">{t("arena.leader")}</span>}
                  </td>
                  <td className={`arena-state ${state}`}>{t(STATE_TEXT[state])}</td>
                  <td className="arena-number">{clockText(ms)}</td>
                  <td className="arena-number">
                    {statFailed ? (
                      <span className="muted" title={t("arena.statsFailed")}>
                        ?
                      </span>
                    ) : stat === null ? (
                      ""
                    ) : (
                      <button
                        type="button"
                        className="link-button"
                        aria-expanded={expanded}
                        title={stat.truncated ? t("arena.truncated") : t("arena.changedFiles")}
                        onClick={() => {
                          setOpen(expanded ? null : id);
                        }}
                      >
                        {stat.files.length}
                        {stat.truncated ? "+" : ""}
                      </button>
                    )}
                  </td>
                  <td className="arena-number">
                    {stat !== null && (
                      <>
                        <span className="arena-added">+{stat.added}</span>{" "}
                        <span className="arena-removed">-{stat.removed}</span>
                      </>
                    )}
                  </td>
                  {arena.testCommand !== "" && (
                    <td className={`arena-tests ${testClass(tests)}`}>
                      {tests.kind === "none" && state !== "starting" && state !== "working" ? (
                        <button
                          type="button"
                          className="link-button"
                          onClick={() => {
                            onRunTests(id);
                          }}
                        >
                          {t("arena.runTests")}
                        </button>
                      ) : (
                        testText(tests, t)
                      )}
                    </td>
                  )}
                  <td className="arena-actions">
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={!canApply(state, stat) || applying}
                      onClick={() => {
                        setApplied(null);
                        setConfirming(id);
                      }}
                    >
                      {t("arena.apply")}
                    </button>
                  </td>
                </tr>
                {expanded && stat !== null && (
                  <tr className="arena-details">
                    <td colSpan={arena.testCommand !== "" ? 7 : 6}>
                      <ul className="arena-files">
                        {stat.files.map((file) => (
                          <li key={file.path}>
                            <span className="arena-file">{file.path}</span>
                            <span className="arena-added">+{file.added}</span>
                            <span className="arena-removed">-{file.removed}</span>
                          </li>
                        ))}
                      </ul>
                      {tests.kind === "done" && tests.run.output !== "" && (
                        <details>
                          <summary>{t("arena.output")}</summary>
                          <pre className="arena-output selectable">{tests.run.output}</pre>
                        </details>
                      )}
                      <div className="arena-detail-actions">
                        <button
                          type="button"
                          className="link-button"
                          onClick={() => {
                            onResend(id);
                          }}
                        >
                          {t("arena.resend")}
                        </button>
                        {arena.testCommand !== "" && tests.kind !== "running" && (
                          <button
                            type="button"
                            className="link-button"
                            onClick={() => {
                              onRunTests(id);
                            }}
                          >
                            {t("arena.runTests")}
                          </button>
                        )}
                      </div>
                    </td>
                  </tr>
                )}
              </Fragment>
            );
          })}
        </tbody>
      </table>
      {confirmRow !== undefined && (
        <div className="arena-confirm" role="group" aria-labelledby="arena-confirm-text">
          <p id="arena-confirm-text">
            {t("arena.applyConfirm", {
              name: confirmRow.contestant.name,
              folder: folderName(arena.repoRoot),
            })}
          </p>
          <div className="dialog-actions">
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                setConfirming(null);
              }}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              className="primary-button"
              disabled={applying}
              onClick={() => {
                apply(confirmRow.contestant.sessionId);
              }}
            >
              {t("arena.apply")}
            </button>
          </div>
        </div>
      )}
      {applied?.kind === "conflict" && (
        <p className="arena-problem" role="alert">
          {t("arena.conflict", { files: applied.files.join(", ") })}
        </p>
      )}
      {applied?.kind === "error" && (
        <p className="arena-problem" role="alert">
          {t("arena.applyFailed", { error: applied.message })}
        </p>
      )}
      {applied?.kind === "applied" && (
        <div className="arena-confirm" role="status">
          <p>{t("arena.applied", { folder: folderName(arena.repoRoot) })}</p>
          {contestants.some((c) => c.sessionId !== applied.winnerId) && (
            <>
              <p className="muted">{t("arena.closeOthers")}</p>
              <div className="dialog-actions">
                <button
                  type="button"
                  className="secondary-button"
                  onClick={() => {
                    setApplied(null);
                  }}
                >
                  {t("arena.keep")}
                </button>
                <button
                  type="button"
                  className="primary-button danger"
                  onClick={() => {
                    onCloseOthers(applied.winnerId);
                    setApplied(null);
                  }}
                >
                  {t("arena.closeOthersButton")}
                </button>
              </div>
            </>
          )}
        </div>
      )}
      <div className="dialog-actions">
        <button type="button" className="secondary-button" onClick={onNew}>
          {t("arena.new")}
        </button>
      </div>
    </>
  );
}
