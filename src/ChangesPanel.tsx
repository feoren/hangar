import { useCallback, useEffect, useRef, useState } from "react";

import { api, errorText } from "./api";
import { ChangeList, type FileActions, type LineCommentHandler } from "./ChangeList";
import type { Overlap, SessionClaims } from "./claims";
import { ClaimsSection } from "./ClaimsView";
import { CommitBox } from "./CommitBox";
import { CommitHistory } from "./CommitHistory";
import { Dialog } from "./Dialog";
import { folderName } from "./folders";
import { commitMode, discardDeletes, lineComment, pathBatches, stageAllPaths } from "./gitActions";
import { parseGitDiff, splitPath, type ChangeEntry, type GitDiff } from "./gitDiff";
import { CHANGES_VIEWS, type ChangesView, type CommitFocus } from "./gitHistory";
import { useT, type MessageKey } from "./i18n";
import { RestartIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { askFor, stillAsked, type SessionQuestion } from "./sessionQuestion";
import type { TeammateOverlap } from "./team";

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
  /** A commit to open in the history (quick access); a new token opens it again. */
  focusCommit?: CommitFocus | null;
  /** UI language, for relative times and dates. */
  language: string;
}

const VIEW_LABELS = {
  open: "changes.viewOpen",
  commits: "changes.viewCommits",
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
export function ChangesPanel(props: Props) {
  const { cwd, trigger, view } = props;
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
  const commit = (text: string) => {
    // Like a commit with `-a`: changed and deleted files, never untracked ones.
    const paths = stageAllPaths(files);
    runAction(
      async (id) => {
        if (mode === "stageAll") await api.gitStage(id, paths);
        await api.gitCommit(id, text);
      },
      () => {
        setDraft(null);
      },
    );
  };
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
        {CHANGES_VIEWS.map((option) => (
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
              onCommit={commit}
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
