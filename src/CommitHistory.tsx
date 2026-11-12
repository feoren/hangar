import { useEffect, useMemo, useRef, useState } from "react";

import { api, errorText } from "./api";
import { ChangeList } from "./ChangeList";
import {
  appendPage,
  bodyPreview,
  COMMIT_PAGE,
  commitAge,
  commitDate,
  commitGroups,
  parseCommitDiff,
  parseGitLog,
  readLog,
  type CommitGroup,
  type CommitSummary,
  type GitCommitDiff,
  type GitLog,
} from "./gitHistory";
import { useT, type Translate } from "./i18n";
import { ArrowUpIcon, ChevronIcon } from "./icons";
import { revealInvisible } from "./invisible";
import { Revealed } from "./Revealed";
import { useClock } from "./useClock";

interface Props {
  /** The active session's folder; the parent remounts this view for another folder. */
  cwd: string;
  /** Changes when the list should be read again (attention state, "Refresh"). */
  reload: string;
  /** UI language, for relative times and dates. */
  language: string;
  /** Whether the list is being read, for the spinning refresh button. */
  onBusy: (busy: boolean) => void;
}

type ListLoad =
  { kind: "loading" } | { kind: "error"; message: string } | { kind: "done"; log: GitLog };

type DetailLoad =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "done"; result: GitCommitDiff };

type More = { kind: "idle" } | { kind: "loading" } | { kind: "error"; message: string };

const MINUTE_MS = 60_000;

/**
 * The commits of the active session's folder, newest first, in the "Commits" view of the changes
 * panel. Commits the upstream lacks come first. A click opens a commit in place with its message
 * and its files, shown like the work tree's changes; each commit's diff is read once per panel.
 */
export function CommitHistory({ cwd, reload, language, onBusy }: Props) {
  const t = useT();
  const now = useClock(MINUTE_MS);
  const [answer, setAnswer] = useState<{ key: string; load: ListLoad } | null>(null);
  const [more, setMore] = useState<More>({ kind: "idle" });
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [details, setDetails] = useState<ReadonlyMap<string, DetailLoad>>(() => new Map());
  const latest = useRef("");
  /** Commits listed right now, so a reload reads as many again. */
  const shown = useRef(0);

  useEffect(() => {
    latest.current = reload;
    const settle = (load: ListLoad) => {
      // An older answer that arrives late must not replace a newer one.
      if (latest.current !== reload) return;
      setAnswer({ key: reload, load });
      setMore({ kind: "idle" });
    };
    readLog((skip, limit) => api.gitLog(cwd, skip, limit), shown.current)
      .then((log) => {
        settle(
          log === null ? { kind: "error", message: t("error.unknown") } : { kind: "done", log },
        );
      })
      .catch((e: unknown) => {
        settle({ kind: "error", message: errorText(e) });
      });
    // `t` only changes with the language; a new language needs no new git call.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cwd, reload]);

  const load: ListLoad = answer?.load ?? { kind: "loading" };
  const busy = answer?.key !== reload || more.kind === "loading";
  const listed = load.kind === "done" ? load.log.commits.length : 0;
  useEffect(() => {
    shown.current = listed;
  }, [listed]);
  useEffect(() => {
    onBusy(busy);
  }, [busy, onBusy]);
  useEffect(
    () => () => {
      onBusy(false);
    },
    [onBusy],
  );

  const loadMore = (log: GitLog) => {
    const key = reload;
    setMore({ kind: "loading" });
    api
      .gitLog(cwd, log.commits.length, COMMIT_PAGE)
      .then((value) => {
        if (latest.current !== key) return;
        const page = parseGitLog(value);
        if (page === null) {
          setMore({ kind: "error", message: t("error.unknown") });
          return;
        }
        setAnswer((current) =>
          current?.key === key && current.load.kind === "done"
            ? { key, load: { kind: "done", log: appendPage(current.load.log, page) } }
            : current,
        );
        setMore({ kind: "idle" });
      })
      .catch((e: unknown) => {
        if (latest.current === key) setMore({ kind: "error", message: errorText(e) });
      });
  };

  const readCommit = (sha: string) => {
    setDetails((current) => new Map(current).set(sha, { kind: "loading" }));
    const settle = (detail: DetailLoad) => {
      setDetails((current) => new Map(current).set(sha, detail));
    };
    api
      .gitCommitDiff(cwd, sha)
      .then((value) => {
        const result = parseCommitDiff(value);
        settle(
          result === null
            ? { kind: "error", message: t("error.unknown") }
            : { kind: "done", result },
        );
      })
      .catch((e: unknown) => {
        settle({ kind: "error", message: errorText(e) });
      });
  };

  const toggle = (sha: string) => {
    const open = expanded.has(sha);
    const next = new Set(expanded);
    if (open) next.delete(sha);
    else next.add(sha);
    setExpanded(next);
    // A commit never changes, so its diff is read once; a failed read is tried again.
    const known = details.get(sha);
    if (!open && (known === undefined || known.kind === "error")) readCommit(sha);
  };

  switch (load.kind) {
    case "loading":
      return (
        <p className="changes-message" role="status">
          {t("changes.loading")}
        </p>
      );
    case "error":
      return (
        <p className="changes-message error-text selectable" role="alert">
          {t("history.failed", { error: load.message })}
        </p>
      );
    case "done":
      break;
  }
  const log = load.log;
  if (!log.isRepo) return <p className="changes-message">{t("changes.notRepo")}</p>;
  if (log.commits.length === 0) return <p className="changes-message">{t("history.none")}</p>;

  return (
    <div className="changes-body">
      {commitGroups(log).map((group) => (
        <section key={group.kind} className="commit-group">
          {group.kind !== "all" && (
            <h3 className="commit-group-title">{groupLabel(group, log, t)}</h3>
          )}
          <ul className="commit-list">
            {group.commits.map((commit) => (
              <CommitItem
                key={commit.sha}
                commit={commit}
                unpushed={log.upstream !== null && !commit.pushed}
                open={expanded.has(commit.sha)}
                detail={details.get(commit.sha)}
                age={commitAge(commit.authoredAt, now, language)}
                date={commitDate(commit.authoredAt, language)}
                onToggle={() => {
                  toggle(commit.sha);
                }}
              />
            ))}
          </ul>
        </section>
      ))}
      {more.kind === "error" && (
        <p className="changes-hint selectable" role="alert">
          {t("history.failed", { error: more.message })}
        </p>
      )}
      {log.hasMore && (
        <button
          type="button"
          className="link-button change-more files"
          // While the list is read again, a page would be counted from the old list and could
          // repeat only known commits, which ends the paging too early.
          disabled={busy}
          onClick={() => {
            loadMore(log);
          }}
        >
          {more.kind === "loading" ? t("changes.loading") : t("history.more")}
        </button>
      )}
    </div>
  );
}

function groupLabel(group: CommitGroup, log: GitLog, t: Translate): string {
  switch (group.kind) {
    case "unpushed":
      return t("history.unpushed", { count: log.aheadCount });
    case "pushed":
      return t("history.pushed", { upstream: log.upstream ?? "" });
    case "all":
      return t("changes.viewCommits");
  }
}

function CommitItem({
  commit,
  unpushed,
  open,
  detail,
  age,
  date,
  onToggle,
}: {
  commit: CommitSummary;
  unpushed: boolean;
  open: boolean;
  detail: DetailLoad | undefined;
  age: string;
  date: string;
  onToggle: () => void;
}) {
  const t = useT();
  const subject = commit.subject === "" ? t("history.noSubject") : commit.subject;
  const tooltip = [
    revealInvisible(subject),
    `${commit.shortSha} · ${revealInvisible(commit.author)} · ${date}`,
    unpushed ? t("history.notPushed") : "",
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <li className={open ? "commit open" : "commit"}>
      <button
        type="button"
        className="commit-head"
        aria-expanded={open}
        title={tooltip}
        onClick={onToggle}
      >
        <ChevronIcon size={12} />
        <span className="commit-text">
          <span className="commit-subject">
            <Revealed text={subject} />
          </span>
          <span className="commit-meta">
            <span className="commit-sha">{commit.shortSha}</span>
            <span className="commit-author">
              <Revealed text={commit.author} />
            </span>
            <span className="commit-age">{age}</span>
          </span>
        </span>
        {unpushed && (
          <span className="commit-unpushed">
            <ArrowUpIcon size={12} />
            <span className="sr-only">{t("history.notPushed")}</span>
          </span>
        )}
      </button>
      {open && <CommitDetail detail={detail} />}
    </li>
  );
}

function CommitDetail({ detail }: { detail: DetailLoad | undefined }) {
  const t = useT();
  if (detail === undefined || detail.kind === "loading") {
    return (
      <p className="change-note" role="status">
        {t("changes.loading")}
      </p>
    );
  }
  if (detail.kind === "error") {
    return (
      <p className="change-note error-text selectable" role="alert">
        {t("history.diffFailed", { error: detail.message })}
      </p>
    );
  }
  const { result } = detail;
  return (
    <div className="commit-detail">
      <CommitBody body={result.commit.body} />
      {result.commit.parents.length > 1 && <p className="commit-note">{t("history.merge")}</p>}
      {result.files.length === 0 && result.diff === "" ? (
        <p className="commit-note">{t("history.empty")}</p>
      ) : (
        <ChangeList result={result} />
      )}
    </div>
  );
}

function CommitBody({ body }: { body: string }) {
  const t = useT();
  const [whole, setWhole] = useState(false);
  const preview = useMemo(() => bodyPreview(body), [body]);
  if (preview.text === "") return null;
  const text = whole ? body.replace(/\s+$/, "") : preview.text;
  return (
    <div className="commit-body selectable">
      {text.split(/\r?\n/).map((line, index) => (
        // Lines of a fixed text; the index is stable.
        <div key={index} className="commit-body-line">
          {line === "" ? " " : <Revealed text={line} />}
        </div>
      ))}
      {preview.long && (
        <button
          type="button"
          className="link-button commit-body-toggle"
          aria-expanded={whole}
          onClick={() => {
            setWhole((value) => !value);
          }}
        >
          {whole ? t("history.lessBody") : t("history.moreBody")}
        </button>
      )}
    </div>
  );
}
