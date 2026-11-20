import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import { ChangeList, type FileActions, type LineCommentHandler } from "./ChangeList";
import type { Overlap, SessionClaims } from "./claims";
import { ClaimsSection } from "./ClaimsView";
import { CommitBox } from "./CommitBox";
import { CommitHistory } from "./CommitHistory";
import { Dialog } from "./Dialog";
import { folderName } from "./folders";
import {
  commitMode,
  discardDeletes,
  lineComment,
  parseCommitOutcome,
  pathBatches,
  stageAllPaths,
  type SecretFinding,
} from "./gitActions";
import { parseGitDiff, splitPath, type ChangeEntry, type GitDiff } from "./gitDiff";
import type { ChangesView, CommitFocus } from "./gitHistory";
import { useT, type MessageKey } from "./i18n";
import { RestartIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { secretLabels } from "./secretScan";
import { askFor, stillAsked, type SessionQuestion } from "./sessionQuestion";
import type { TeammateOverlap } from "./team";
import { changesViews, shownChangesView, type TimelineAvailability } from "./timeline";
import { TimelineView } from "./TimelineView";

interface Props {
  /** Working folder of the active session; `null` without a session. */
  cwd: string | null;
  /** The active session, whose folder is shown; the git actions name it. */
  sessionId: number | null;
  /** It runs an agent: diff lines take comments and the commit box asks it for a message. */
  agent: boolean;
  /** Text into the active session's prompt, as pasted (no Enter). */
  onSendToAgent: (text: string) => void;
  /**
   * Changes when the diff should be read again: the active session's attention state (an agent
   * that finished a step usually changed files).
   */
  trigger: string;
  /** Goes up when the panel should take the focus (opened via shortcut or menu). */
  focusToken: number;
  /** The active session's claims (claims.ts); `null` while off or not known yet. */
  claims: SessionClaims | null;
  /** Its collisions with other sessions, worst first. */
  overlaps: readonly Overlap[];
  /** Its collisions with teammates' sessions in shared projects, worst first. */
  teammates: readonly TeammateOverlap[];
  nameOf: (id: number) => string;
  onShowSession: (id: number) => void;
  /** Uncommitted changes or the commit history; kept by the window while the tab is closed. */
  view: ChangesView;
  onView: (view: ChangesView) => void;
  /**
   * Whether the timeline is offered (timelineAvailability): hidden where no snapshot is ever
   * taken, a hint while working-state detection is off.
   */
  timeline: TimelineAvailability;
  /** A commit to open in the history (quick access); a new token opens it again. */
  focusCommit?: CommitFocus | null;
  /** UI language, for relative times and dates. */
  language: string;
}

const VIEW_LABELS = {
  open: "changes.viewOpen",
  commits: "changes.viewCommits",
  timeline: "changes.viewTimeline",
} as const satisfies Record<ChangesView, MessageKey>;

/** A text that belongs to one folder: a draft or an error of another folder is not shown. */
interface ForFolder {
  cwd: string;
  text: string;
}

type Load =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "done"; result: GitDiff };

/**
 * Changes of the active session's folder, the second tab of the right panel (Ctrl+Shift+D): either
 * what is not committed yet (against the last commit) or the commits themselves. Read again on
 * open, on a new session, on "Refresh" and whenever the session's attention state changes. Large
 * diffs show a page of lines per file, more on request.
 */
/** A commit held back because its staged lines seem to hold secrets (ADR 0015). */
interface SecretAsk {
  sessionId: number;
  cwd: string;
  /** The message as it was sent. */
  text: string;
  findings: SecretFinding[];
  more: number;
  complete: boolean;
  /** The checked index, handed back by "Commit anyway"; `null`: it cannot be committed anyway. */
  tree: string | null;
}

export function ChangesPanel(props: Props) {
  const { cwd, trigger } = props;
  const view = shownChangesView(props.view, props.timeline);
  const t = useT();
  const [manual, setManual] = useState(0);
  const [historyBusy, setHistoryBusy] = useState(false);
  const onHistoryBusy = useCallback((busy: boolean) => {
    setHistoryBusy(busy);
  }, []);
  /** Last answer, with the request it belongs to. */
  const [answer, setAnswer] = useState<{ key: string; cwd: string; load: Load } | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const latest = useRef("");
  /** A git action runs; the panel reads the changes again when it ends. */
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<ForFolder | null>(null);
  const [draft, setDraft] = useState<ForFolder | null>(null);
  /**
   * The commit that waits because its staged lines seem to hold secrets (ADR 0015), for the
   * session and folder it was asked in, with its message: "Commit anyway" commits exactly that.
   */
  const [secretAsk, setSecretAsk] = useState<SecretAsk | null>(null);
  /** The last commit in this folder went in without its changes being checked in full. */
  const [checkNote, setCheckNote] = useState<{
    cwd: string;
    checked: "partial" | "skipped";
  } | null>(null);
  /**
   * The file whose changes the user wants to throw away, while the question is open, with the
   * session and folder it was asked for: the answer acts there, never on a session switched to.
   */
  const [discarding, setDiscarding] = useState<SessionQuestion<ChangeEntry> | null>(null);
  const send = useRef(props.onSendToAgent);
  useEffect(() => {
    send.current = props.onSendToAgent;
  });
  const reload = `${trigger}\n${String(manual)}`;
  const key = `${cwd ?? ""}\n${String(props.sessionId)}\n${reload}\n${view}`;

  useEffect(() => {
    latest.current = key;
    const id = props.sessionId;
    if (cwd === null || id === null || view !== "open") return;
    const settle = (load: Load) => {
      // An older answer that arrives late must not replace a newer one.
      if (latest.current === key) setAnswer({ key, cwd, load });
    };
    api
      .gitDiff(id)
      .then((value) => {
        const result = parseGitDiff(value);
        settle(
          result === null
            ? { kind: "error", message: t("error.unknown") }
            : { kind: "done", result },
        );
      })
      .catch((e: unknown) => {
        settle({ kind: "error", message: errorText(e) });
      });
    // `t` only changes with the language; a new language needs no new git call.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // While reading again, the previous answer of the same folder stays visible (no flicker); a new
  // folder starts empty instead of showing another folder's changes.
  const load: Load =
    cwd === null
      ? { kind: "idle" }
      : answer !== null && answer.cwd === cwd
        ? answer.load
        : { kind: "loading" };
  // A running git action counts too: a commit with slow hooks shows that something happens.
  const refreshing = cwd !== null && (view === "open" ? busy || answer?.key !== key : historyBusy);

  const { sessionId } = props;
  /**
   * Runs one action for `target` (the active session unless a question names its own), shows
   * git's message if it fails, then reads again.
   */
  const runAction = (
    work: (id: number) => Promise<unknown>,
    done?: () => void,
    target: { sessionId: number | null; cwd: string | null } = { sessionId, cwd },
  ) => {
    const folder = target.cwd;
    if (target.sessionId === null || folder === null || busy) return;
    setBusy(true);
    setActionError(null);
    work(target.sessionId)
      .then(() => {
        done?.();
      })
      .catch((e: unknown) => {
        setActionError({ cwd: folder, text: errorText(e) });
      })
      .finally(() => {
        setBusy(false);
        setManual((value) => value + 1);
      });
  };
  const actions: FileActions | undefined =
    sessionId === null
      ? undefined
      : {
          busy,
          onStage: (entry, stage) => {
            runAction((id) =>
              stage ? api.gitStage(id, [entry.path]) : api.gitUnstage(id, [entry.path]),
            );
          },
          onStageAll: (paths, stage) => {
            runAction(async (id) => {
              // One call per batch; the backend takes a bounded number of paths per call.
              for (const batch of pathBatches(paths)) {
                await (stage ? api.gitStage(id, batch) : api.gitUnstage(id, batch));
              }
            });
          },
          onDiscard: (entry) => {
            setDiscarding(askFor(entry, sessionId, cwd));
          },
        };
  const onComment = useCallback<LineCommentHandler>((path, line, comment) => {
    const text = lineComment(path, line, comment);
    if (text !== null) send.current(text);
  }, []);
  const files = load.kind === "done" && load.result.isRepo ? load.result.files : [];
  const mode = commitMode(files);
  const message = draft !== null && draft.cwd === cwd ? draft.text : "";
  const error = actionError !== null && actionError.cwd === cwd ? actionError.text : null;
  const commit = (text: string, allowTree: string | null = null) => {
    // Like a commit with `-a`: changed and deleted files, never untracked ones.
    const paths = stageAllPaths(files);
    let waits = false;
    setCheckNote(null);
    runAction(
      async (id) => {
        if (mode === "stageAll") await api.gitStage(id, paths);
        const outcome = parseCommitOutcome(await api.gitCommit(id, text, allowTree));
        if (cwd === null) return;
        if (outcome.kind === "secrets") {
          waits = true;
          setSecretAsk({
            sessionId: id,
            cwd,
            text,
            findings: outcome.findings,
            more: outcome.more,
            complete: outcome.complete,
            tree: outcome.tree,
          });
        } else if (outcome.checked !== "full") {
          setCheckNote({ cwd, checked: outcome.checked });
        }
      },
      () => {
        if (!waits) setDraft(null);
      },
    );
  };
  // Asked for one session and folder only, like the discard question.
  const secretShown =
    secretAsk !== null && secretAsk.sessionId === sessionId && secretAsk.cwd === cwd
      ? secretAsk
      : null;
  if (secretAsk !== null && secretShown === null) setSecretAsk(null);
  const closeDiscard = () => {
    setDiscarding(null);
    panel.current?.focus();
  };
  // Asked for one session and folder: shown only while the panel still shows those, and closed
  // for good on a switch (state adjusted while rendering), so coming back does not bring it up
  // again.
  const discardShown = stillAsked(discarding, sessionId, cwd);
  if (discarding !== null && discardShown === null) setDiscarding(null);

  useEffect(() => {
    if (props.focusToken === 0) return;
    // The panel itself, so no button shows a focus ring; Tab goes on to Refresh and the files.
    panel.current?.focus();
  }, [props.focusToken]);

  return (
    <div ref={panel} tabIndex={-1} className="changes">
      <div className="changes-header">
        <p className="changes-title">
          {cwd !== null ? (
            <span className="changes-folder" title={revealInvisible(cwd)}>
              <Revealed text={folderName(cwd)} />
            </span>
          ) : (
            <span className="changes-folder">{t("changes.title")}</span>
          )}
        </p>
        <button
          type="button"
          className={refreshing ? "icon-button changes-refresh spinning" : "icon-button"}
          aria-label={t("changes.refresh")}
          title={t("changes.refresh")}
          disabled={cwd === null}
          onClick={() => {
            setManual((value) => value + 1);
          }}
        >
          <RestartIcon />
        </button>
      </div>
      <div className="changes-views segmented" role="radiogroup" aria-label={t("changes.view")}>
        {changesViews(props.timeline).map((option) => (
          <label key={option} className="segment">
            <input
              type="radio"
              name="changes-view"
              value={option}
              checked={view === option}
              onChange={() => {
                props.onView(option);
              }}
            />
            <span>{t(VIEW_LABELS[option])}</span>
          </label>
        ))}
      </div>
      {view === "open" ? (
        <>
          {sessionId !== null && cwd !== null && mode !== "none" && (
            <CommitBox
              message={message}
              onMessage={(text) => {
                setDraft({ cwd, text });
              }}
              mode={mode}
              busy={busy}
              onCommit={(text) => {
                setSecretAsk(null);
                commit(text);
              }}
              onSuggest={
                props.agent
                  ? () => {
                      props.onSendToAgent(
                        t(mode === "commit" ? "changes.suggestStaged" : "changes.suggestAll"),
                      );
                    }
                  : undefined
              }
            />
          )}
          {secretShown !== null && (
            <CommitSecretAsk
              ask={secretShown}
              busy={busy}
              onCancel={() => {
                setSecretAsk(null);
              }}
              onCommitAnyway={(tree) => {
                const asked = secretShown;
                setSecretAsk(null);
                commit(asked.text, tree);
              }}
            />
          )}
          {checkNote !== null && checkNote.cwd === cwd && (
            <CommitCheckNote checked={checkNote.checked} />
          )}
          {error !== null && (
            <p className="changes-hint error-text selectable" role="alert">
              {t("changes.actionFailed", { error })}
            </p>
          )}
          <ClaimsSection
            key={props.claims?.sessionId ?? "none"}
            claims={props.claims}
            overlaps={props.overlaps}
            teammates={props.teammates}
            nameOf={props.nameOf}
            onShowSession={props.onShowSession}
          />
          <ChangesBody
            cwd={cwd}
            load={load}
            actions={actions}
            onComment={props.agent ? onComment : undefined}
          />
        </>
      ) : cwd === null || props.sessionId === null ? (
        <p className="changes-message">{t("changes.noSession")}</p>
      ) : view === "timeline" && props.timeline === "detectionOff" ? (
        <p className="changes-message">{t("timeline.detectionOff")}</p>
      ) : view === "timeline" ? (
        <TimelineView
          key={props.sessionId}
          sessionId={props.sessionId}
          reload={reload}
          language={props.language}
          working={trigger === "working"}
          onBusy={onHistoryBusy}
          onRestored={() => {
            setManual((value) => value + 1);
          }}
        />
      ) : (
        <CommitHistory
          key={`${cwd}\n${String(props.sessionId)}`}
          sessionId={props.sessionId}
          cwd={cwd}
          reload={reload}
          language={props.language}
          onBusy={onHistoryBusy}
          focus={props.focusCommit ?? null}
        />
      )}
      {discardShown !== null && (
        <DiscardDialog
          entry={discardShown.item}
          cwd={discardShown.cwd}
          onCancel={closeDiscard}
          onConfirm={() => {
            const path = discardShown.item.path;
            closeDiscard();
            runAction((id) => api.gitDiscard(id, [path]), undefined, discardShown);
          }}
        />
      )}
    </div>
  );
}

/**
 * Asks before a file's changes are thrown away; an untracked or newly added file is deleted. The
 * folder it names is the one the question was asked for.
 */
/**
 * The commit box's question when the staged changes seem to hold secrets (ADR 0015): files and
 * kinds, then "Cancel" and "Commit anyway". Without a checked tree only "Cancel" is offered.
 */
export function CommitSecretAsk({
  ask,
  busy,
  onCancel,
  onCommitAnyway,
}: {
  ask: Pick<SecretAsk, "findings" | "more" | "complete" | "tree">;
  busy: boolean;
  onCancel: () => void;
  onCommitAnyway: (tree: string) => void;
}) {
  const t = useT();
  const { tree } = ask;
  return (
    <div className="changes-hint commit-secrets" role="alert">
      <p className="commit-secrets-title">{t("changes.secretTitle")}</p>
      {ask.findings.length > 0 && (
        <ul className="commit-secrets-list">
          {ask.findings.map((finding) => (
            <li key={finding.path}>
              <Revealed text={finding.path} />
              {": "}
              {secretLabels(finding.kinds, t)}
            </li>
          ))}
        </ul>
      )}
      {ask.more > 0 && <p>{t("changes.secretMore", { count: String(ask.more) })}</p>}
      {!ask.complete && <p>{t("changes.secretPartial")}</p>}
      {tree === null && <p>{t("changes.secretNoTree")}</p>}
      <div className="link-actions">
        <button type="button" className="secondary-button" disabled={busy} onClick={onCancel}>
          {t("common.cancel")}
        </button>
        {tree !== null && (
          <button
            type="button"
            className="primary-button"
            disabled={busy}
            onClick={() => {
              onCommitAnyway(tree);
            }}
          >
            {t("changes.commitAnyway")}
          </button>
        )}
      </div>
    </div>
  );
}

/** After a commit whose staged changes were not checked for secrets in full (ADR 0015). */
export function CommitCheckNote({ checked }: { checked: "partial" | "skipped" }) {
  const t = useT();
  return (
    <p className="changes-hint" role="status">
      {t(checked === "partial" ? "changes.checkedPartial" : "changes.checkedSkipped")}
    </p>
  );
}

export function DiscardDialog({
  entry,
  cwd,
  onCancel,
  onConfirm,
}: {
  entry: ChangeEntry;
  cwd: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const t = useT();
  const deletes = discardDeletes(entry.status);
  const name = revealInvisible(splitPath(entry.path.replace(/\/$/, "")).name);
  const path = revealInvisible(entry.path);
  const folder = revealInvisible(folderName(cwd));
  return (
    <Dialog labelledBy="discard-title" onClose={onCancel}>
      <h2 id="discard-title">
        {deletes ? t("changes.deleteTitle", { name }) : t("changes.discardTitle", { name })}
      </h2>
      <p className="selectable" title={revealInvisible(cwd)}>
        {t("changes.inFolder", { folder })}
      </p>
      <p className="muted selectable">
        {deletes ? t("changes.deleteBody", { path }) : t("changes.discardBody", { path })}
      </p>
      <div className="dialog-actions">
        <button type="button" className="secondary-button" data-autofocus onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button type="button" className="primary-button danger" onClick={onConfirm}>
          {deletes ? t("changes.deleteConfirm") : t("changes.discardConfirm")}
        </button>
      </div>
    </Dialog>
  );
}

function ChangesBody({
  cwd,
  load,
  actions,
  onComment,
}: {
  cwd: string | null;
  load: Load;
  actions: FileActions | undefined;
  onComment: LineCommentHandler | undefined;
}) {
  const t = useT();
  if (cwd === null) return <p className="changes-message">{t("changes.noSession")}</p>;
  switch (load.kind) {
    case "idle":
    case "loading":
      return (
        <p className="changes-message" role="status">
          {t("changes.loading")}
        </p>
      );
    case "error":
      return (
        <p className="changes-message error-text selectable" role="alert">
          {t("changes.failed", { error: load.message })}
        </p>
      );
    case "done":
      if (!load.result.isRepo) {
        return <p className="changes-message">{t("changes.notRepo")}</p>;
      }
      if (load.result.files.length === 0 && load.result.diff === "") {
        return <p className="changes-message">{t("changes.none")}</p>;
      }
      return (
        <div className="changes-body">
          <ChangeList result={load.result} actions={actions} onComment={onComment} />
        </div>
      );
  }
}
