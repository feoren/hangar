// Settings, Sub-agents: Claude Code's sub-agent definitions with their figures and models, Codex
// CLI's own definitions, the default model per program and the copies of the user's
// definitions in other programs. One row per name (global and project files of that name
// together), a figure picker below the row whose figure was clicked, and a form for a new
// definition from pasted text or a loaded file.

import { useEffect, useRef, useState } from "react";

import { AgentFigure } from "./AgentFigure";
import { withExtras } from "./agentStartMenu";
import { api, errorText } from "./api";
import { useT, type MessageKey } from "./i18n";
import { folderName } from "./folders";
import {
  BUILTIN_FIGURES,
  DEF_TEMPLATE,
  defFigure,
  figureErrorCode,
  FIGURE_TYPES,
  figureName,
  initialFigure,
  isFromHangar,
  isHangarAgent,
  libraryEntries,
  MAX_DEF_BYTES,
  MAX_FIGURE_BYTES,
  parseDefError,
  pastedName,
  type DefProblemKind,
  type FigureErrorCode,
  type FigureInput,
  type FigureView,
  type CodexDef,
  type Library,
  type SyncIssue,
  type SyncIssueKind,
  type LibraryEntry,
} from "./subAgentLibrary";
import {
  applyFigures,
  refreshLibrary,
  syncLibrary,
  useSubAgentLibrary,
} from "./subAgentLibraryStore";
import { ModelField } from "./SubAgentModelField";
import {
  backupsOf,
  codexSuggestions,
  copiesOf,
  failures,
  issuesOf,
  NO_CHOICE,
  ownCodexDefs,
  parseSubAgentSettings,
  provides,
  withBundled,
  withModel,
  withProvide,
  type DefProgram,
  type ModelChoice,
  type SubAgentSettings,
} from "./subAgentModels";

const PROBLEM_TEXTS: Record<DefProblemKind, MessageKey> = {
  tooLarge: "subAgentLib.problem.tooLarge",
  notText: "subAgentLib.problem.notText",
  noFrontmatter: "subAgentLib.problem.noFrontmatter",
  missingName: "subAgentLib.problem.missingName",
  invalidName: "subAgentLib.problem.invalidName",
  missingDescription: "subAgentLib.problem.missingDescription",
  duplicate: "subAgentLib.problem.duplicate",
  link: "subAgentLib.problem.link",
  unreadable: "subAgentLib.problem.unreadable",
  tooMany: "subAgentLib.problem.tooMany",
  notToml: "subAgentLib.problem.notToml",
  missingInstructions: "subAgentLib.problem.missingInstructions",
};

const FIGURE_ERROR_TEXTS: Record<FigureErrorCode, MessageKey> = {
  no_image: "subAgentLib.noClipboardImage",
  not_an_image: "subAgentLib.imageWrong",
  too_large: "subAgentLib.imageTooLarge",
};

function readFile(file: File, as: "text" | "base64"): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("read"));
        return;
      }
      resolve(as === "text" ? result : result.slice(result.indexOf(",") + 1));
    };
    reader.onerror = () => {
      reject(new Error("read"));
    };
    if (as === "text") reader.readAsText(file);
    else reader.readAsDataURL(file);
  });
}

const PROGRAM_NAMES: Record<DefProgram, string> = { claude: "Claude Code", codex: "Codex CLI" };

export function SubAgentSettings() {
  const t = useT();
  const { library, error } = useSubAgentLibrary();
  const [picking, setPicking] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const entries = libraryEntries(library);
  const ownCodex = ownCodexDefs(library);

  // Files may have changed outside Hangar since the last look; the copies follow them.
  useEffect(() => {
    void syncLibrary();
  }, []);

  return (
    <section className="settings-section sub-agent-settings" aria-labelledby="sub-agents-help">
      <p id="sub-agents-help" className="muted settings-intro">
        {t("subAgentLib.intro")}
      </p>
      {error !== null && <p className="muted error-text">{error}</p>}
      <ModelDefaults library={library} />
      {entries.length === 0 && ownCodex.length === 0 && !creating && (
        <p className="muted">{t("subAgentLib.empty")}</p>
      )}
      <ul className="sub-agent-list">
        {entries.map((entry) => (
          <SubAgentRow
            key={entry.name}
            entry={entry}
            picking={picking === entry.name}
            onPick={() => {
              setPicking((current) => (current === entry.name ? null : entry.name));
            }}
          />
        ))}
        {ownCodex.map((def) => (
          <CodexRow key={`${def.folder ?? ""}/${def.file}`} def={def} />
        ))}
      </ul>
      {library.problems.length > 0 && (
        <details className="sub-agent-problems">
          <summary className="muted">
            {t("subAgentLib.problems", { count: library.problems.length })}
          </summary>
          <ul>
            {library.problems.map((problem) => (
              <li
                key={`${problem.program}/${problem.folder ?? ""}/${problem.file}`}
                className="muted"
              >
                <code>{problem.file}</code>
                {problem.folder !== null && ` (${folderName(problem.folder)})`}:{" "}
                {t(PROBLEM_TEXTS[problem.problem])}
              </li>
            ))}
          </ul>
        </details>
      )}
      {creating ? (
        <NewSubAgent
          onDone={() => {
            setCreating(false);
          }}
        />
      ) : (
        <div className="settings-row">
          <button
            type="button"
            className="secondary-button"
            onClick={() => {
              setCreating(true);
            }}
          >
            {t("subAgentLib.new")}
          </button>
        </div>
      )}
    </section>
  );
}

/**
 * The default model of every sub-agent per installed program, and whether the user's
 * definitions go to the other programs. Only programs installed here are shown.
 */
function ModelDefaults({ library }: { library: Library }) {
  const t = useT();
  const [settings, setSettings] = useState<SubAgentSettings | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .subAgentSettings()
      .then((value) => {
        if (live) setSettings(parseSubAgentSettings(value));
      })
      .catch((e: unknown) => {
        if (live) setMessage(errorText(e));
      });
    return () => {
      live = false;
    };
  }, []);

  if (settings === null || library.installed.length === 0) {
    return message === null ? null : <p className="muted error-text">{message}</p>;
  }
  const save = (next: SubAgentSettings) => {
    setMessage(null);
    const before = settings;
    setSettings(next);
    api
      .saveSubAgentSettings(next)
      .then(() => syncLibrary())
      .catch((e: unknown) => {
        setSettings(before);
        setMessage(errorText(e));
      });
  };
  const suggestions = codexSuggestions(library, library.codexModels);
  return (
    <div className="sub-agent-defaults" role="group" aria-labelledby="sub-agent-defaults-title">
      <h4 id="sub-agent-defaults-title" className="settings-subtitle">
        {t("subAgentLib.defaults")}
      </h4>
      {library.installed.map((program) => (
        <div key={program} className="sub-agent-default">
          <span className="sub-agent-default-name">{PROGRAM_NAMES[program]}</span>
          <ModelField
            program={program}
            value={settings.models[program] ?? NO_CHOICE}
            suggestions={program === "codex" ? suggestions : []}
            label={t("subAgentLib.defaultFor", { program: PROGRAM_NAMES[program] })}
            onChange={(choice) => {
              save(withModel(settings, program, choice));
            }}
          />
        </div>
      ))}
      <p className="muted settings-help">{t("subAgentLib.defaultsHelp")}</p>
      <div className="sub-agent-provide">
        <label className="check">
          <input
            type="checkbox"
            checked={!settings.bundledOff}
            aria-describedby="sub-agent-bundled-help"
            onChange={(event) => {
              save(withBundled(settings, event.currentTarget.checked));
            }}
          />
          <span>{t("subAgentLib.provideBundled")}</span>
        </label>
        <p id="sub-agent-bundled-help" className="muted settings-help">
          {t("subAgentLib.provideBundledHelp")}
        </p>
      </div>
      {/* Providing the definitions is no model setting: its own line and help below the models. */}
      {library.targets.map((target) => (
        <div key={target.program} className="sub-agent-provide">
          <label className="check">
            <input
              type="checkbox"
              checked={!settings.provideOff.includes(target.program)}
              aria-describedby={`sub-agent-provide-help-${target.program}`}
              onChange={(event) => {
                save(withProvide(settings, target.program, event.currentTarget.checked));
              }}
            />
            <span>{t("subAgentLib.provide", { program: PROGRAM_NAMES[target.program] })}</span>
          </label>
          <p id={`sub-agent-provide-help-${target.program}`} className="muted settings-help">
            {t("subAgentLib.provideHelp")}
          </p>
          <IssueList issues={failures(library, target.program)} />
        </div>
      ))}
      {message !== null && <p className="muted error-text">{message}</p>}
    </div>
  );
}

/** A model change of one file; a project's file only after the question in the row. */
function useModelChange(onError: (message: string) => void) {
  const t = useT();
  const [pending, setPending] = useState<{
    program: DefProgram;
    name: string;
    folder: string;
    choice: ModelChoice;
  } | null>(null);
  const send = (
    program: DefProgram,
    name: string,
    folder: string | null,
    choice: ModelChoice,
    confirmed: boolean,
  ) => {
    api
      .setSubAgentModel({ program, name, folder, ...choice, confirmed })
      .then(() => refreshLibrary())
      .catch((e: unknown) => {
        const error = parseDefError(e);
        if (error?.kind === "model") onError(t("subAgentLib.modelInvalid"));
        else onError(error?.message ?? errorText(e));
      });
  };
  const change = (
    program: DefProgram,
    name: string,
    folder: string | null,
    choice: ModelChoice,
  ) => {
    if (folder !== null) setPending({ program, name, folder, choice });
    else send(program, name, null, choice, false);
  };
  const question =
    pending === null ? null : (
      <div className="settings-row sub-agent-confirm" role="group">
        <span>{t("subAgentLib.modelProjectConfirm", { folder: folderName(pending.folder) })}</span>
        <button
          type="button"
          className="primary-button"
          onClick={() => {
            send(pending.program, pending.name, pending.folder, pending.choice, true);
            setPending(null);
          }}
        >
          {t("subAgentLib.modelChange")}
        </button>
        <button
          type="button"
          className="secondary-button"
          onClick={() => {
            setPending(null);
          }}
        >
          {t("common.cancel")}
        </button>
      </div>
    );
  return { change, question };
}

const ISSUE_TEXTS: Record<SyncIssueKind, MessageKey> = {
  unreadable: "subAgentLib.issue.unreadable",
  io: "subAgentLib.issue.io",
  offMachine: "subAgentLib.issue.offMachine",
  noBackupName: "subAgentLib.issue.noBackupName",
  tracked: "subAgentLib.issue.tracked",
  link: "subAgentLib.issue.link",
};

function issueText(issue: SyncIssue, t: ReturnType<typeof useT>): string {
  return t(ISSUE_TEXTS[issue.kind], { folder: folderName(issue.folder ?? "~") });
}

/** What the copy runs left alone, file and reason, below the switch. */
export function IssueList({ issues }: { issues: SyncIssue[] }) {
  const t = useT();
  if (issues.length === 0) return null;
  return (
    <ul className="sub-agent-issues" role="status">
      {issues.map((issue) => (
        <li key={`${issue.folder ?? ""}/${issue.file}/${issue.kind}`} className="muted error-text">
          <code>{issue.file}</code>
          {issue.folder !== null && ` (${folderName(issue.folder)})`}: {issueText(issue, t)}
        </li>
      ))}
    </ul>
  );
}

/**
 * Where a copy stands, as a short line: provided, replaced with every backup named, or why it
 * is missing.
 */
function copyText(
  copy: CodexDef | undefined,
  issue: SyncIssue | undefined,
  t: ReturnType<typeof useT>,
): string {
  if (copy === undefined) {
    return issue === undefined ? t("subAgentLib.copyMissing") : issueText(issue, t);
  }
  const backups = backupsOf(copy);
  if (backups.length === 0) return t("subAgentLib.copyProvided");
  const place =
    copy.folder === null ? "~/.codex/agents" : `${folderName(copy.folder)}/.codex/agents`;
  return t("subAgentLib.copyReplaced", {
    backup: backups.map((backup) => `${place}/${backup}`).join(", "),
  });
}

function SubAgentRow({
  entry,
  picking,
  onPick,
}: {
  entry: LibraryEntry;
  picking: boolean;
  onPick: () => void;
}) {
  const t = useT();
  const { library, figures } = useSubAgentLibrary();
  const [confirming, setConfirming] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const models = useModelChange(setProblem);
  const figure = defFigure(entry.global ?? entry.def, library, figures);
  const places = [
    ...(entry.global === null ? [] : [t("subAgentLib.global")]),
    ...(isFromHangar(library, entry) ? [t("subAgentLib.fromHangar")] : []),
    // A project file of a global name replaces the global one in that project: said so.
    ...entry.projects.map((def) =>
      withExtras(
        t(entry.global === null ? "subAgentLib.project" : "subAgentLib.overridesGlobal", {
          folder: folderName(def.folder ?? ""),
        }),
        def,
        t,
      ),
    ),
  ];
  const fail = (e: unknown) => {
    setProblem(parseDefError(e)?.message ?? errorText(e));
  };
  const open = () => {
    setProblem(null);
    const def = entry.global ?? entry.def;
    api.openSubAgent(entry.name, def.scope === "global" ? null : def.folder).catch(fail);
  };
  const remove = () => {
    setConfirming(false);
    api
      .deleteSubAgent(entry.name)
      .then(() => refreshLibrary())
      .catch(fail);
  };
  const def = entry.global ?? entry.def;
  // Copies exist of global definitions only, and only for programs Hangar hands them to.
  const copies =
    entry.global === null || !provides(library, "codex") ? null : copiesOf(library, entry.name);
  const globalCopy = copies?.find((copy) => copy.scope === "global");
  const projectCopies =
    copies?.filter((copy) => copy.scope !== "global" && backupsOf(copy).length > 0) ?? [];
  const issues = copies === null ? [] : issuesOf(library, entry.name);
  const globalIssue = issues.find((issue) => issue.scope === "global");
  const projectIssues = issues.filter((issue) => issue.scope !== "global");

  return (
    <li className="sub-agent-row">
      {/* Only Hangar's own agents wear figures, so only they can pick one. */}
      {isHangarAgent(entry.name) ? (
        <button
          type="button"
          className="sub-agent-figure-button"
          aria-expanded={picking}
          aria-label={t("subAgentLib.figureFor", { name: entry.name })}
          title={t("subAgentLib.figureFor", { name: entry.name })}
          onClick={onPick}
        >
          <AgentFigure figure={figure} size={96} crop="full" />
        </button>
      ) : (
        <span className="sub-agent-figure-button">
          <AgentFigure figure={figure} size={96} crop="full" />
        </span>
      )}
      <div className="sub-agent-text">
        <div className="sub-agent-head">
          <strong className="sub-agent-name">{entry.name}</strong>
          <span className="muted sub-agent-meta">{places.join(" · ")}</span>
        </div>
        <p className="sub-agent-description" title={entry.def.description}>
          {entry.def.description}
        </p>
        <div className="sub-agent-models">
          <span className="sub-agent-model">
            <span className="muted">{PROGRAM_NAMES.claude}</span>
            <ModelField
              program="claude"
              value={{ model: def.model, effort: null }}
              label={t("subAgentLib.modelOf", { name: entry.name })}
              onChange={(choice) => {
                setProblem(null);
                models.change(
                  "claude",
                  entry.name,
                  def.scope === "global" ? null : def.folder,
                  choice,
                );
              }}
            />
          </span>
          {copies !== null && (
            <span className="sub-agent-model">
              <span className="muted">{PROGRAM_NAMES.codex}</span>
              {globalCopy !== undefined && (
                <ModelField
                  program="codex"
                  value={{ model: globalCopy.model, effort: globalCopy.effort }}
                  suggestions={codexSuggestions(library, library.codexModels)}
                  label={t("subAgentLib.modelIn", {
                    name: entry.name,
                    program: PROGRAM_NAMES.codex,
                  })}
                  onChange={(choice) => {
                    setProblem(null);
                    models.change("codex", entry.name, null, choice);
                  }}
                />
              )}
              <span className="muted sub-agent-copy">{copyText(globalCopy, globalIssue, t)}</span>
            </span>
          )}
        </div>
        {projectCopies.map((copy) => (
          <p key={`${copy.folder ?? ""}/${copy.file}`} className="muted sub-agent-copy">
            {PROGRAM_NAMES.codex}: {copyText(copy, undefined, t)}
          </p>
        ))}
        {projectIssues.map((issue) => (
          <p key={`${issue.folder ?? ""}/${issue.file}`} className="muted sub-agent-copy">
            {PROGRAM_NAMES.codex}: {issueText(issue, t)}
          </p>
        ))}
        {models.question}
        <div className="sub-agent-actions">
          <button
            type="button"
            className="link-button"
            title={t("subAgentLib.openHelp")}
            onClick={open}
          >
            {t("subAgentLib.open")}
          </button>
          {entry.global !== null && !confirming && (
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setConfirming(true);
              }}
            >
              {t("subAgentLib.delete")}
            </button>
          )}
        </div>
        {confirming && (
          <div className="settings-row sub-agent-confirm" role="group">
            <span>{t("subAgentLib.deleteConfirm", { name: entry.name })}</span>
            <button type="button" className="primary-button danger" onClick={remove}>
              {t("subAgentLib.deleteYes")}
            </button>
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                setConfirming(false);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        )}
        {problem !== null && <p className="muted error-text">{problem}</p>}
        {picking && isHangarAgent(entry.name) && (
          <FigurePicker name={entry.name} chosen={figures.get(entry.name)} />
        )}
      </div>
    </li>
  );
}

/** A Codex CLI definition of the user's own: its model and effort; it never starts as an agent. */
function CodexRow({ def }: { def: CodexDef }) {
  const t = useT();
  const { library } = useSubAgentLibrary();
  const [problem, setProblem] = useState<string | null>(null);
  const models = useModelChange(setProblem);
  const place =
    def.folder === null
      ? t("subAgentLib.global")
      : t("subAgentLib.project", { folder: folderName(def.folder) });
  return (
    <li className="sub-agent-row">
      <span className="sub-agent-figure-button">
        <AgentFigure figure={initialFigure(def.name, null)} size={96} crop="full" />
      </span>
      <div className="sub-agent-text">
        <div className="sub-agent-head">
          <strong className="sub-agent-name">{def.name}</strong>
          <span className="sub-agent-badge">{PROGRAM_NAMES.codex}</span>
          <span className="muted sub-agent-meta">{place}</span>
        </div>
        <p className="sub-agent-description" title={def.description}>
          {def.description}
        </p>
        <div className="sub-agent-models">
          <span className="sub-agent-model">
            <ModelField
              program="codex"
              value={{ model: def.model, effort: def.effort }}
              suggestions={codexSuggestions(library, library.codexModels)}
              label={t("subAgentLib.modelOf", { name: def.name })}
              onChange={(choice) => {
                setProblem(null);
                models.change("codex", def.name, def.folder, choice);
              }}
            />
          </span>
        </div>
        {models.question}
        {problem !== null && <p className="muted error-text">{problem}</p>}
      </div>
    </li>
  );
}
/** `chosen`: the figure the user picked; a figure only derived from the name marks none. */
function FigurePicker({ name, chosen }: { name: string; chosen: FigureView | undefined }) {
  const t = useT();
  const fileInput = useRef<HTMLInputElement>(null);
  const [message, setMessage] = useState<string | null>(null);

  const fail = (e: unknown) => {
    const code = figureErrorCode(e);
    setMessage(code === null ? errorText(e) : t(FIGURE_ERROR_TEXTS[code]));
  };
  const choose = (figure: FigureInput) => {
    setMessage(null);
    api.setSubAgentFigure(name, figure).then(applyFigures).catch(fail);
  };
  const onFile = async (file: File | undefined) => {
    if (file === undefined) return;
    if (!FIGURE_TYPES.some((type) => type === file.type)) {
      setMessage(t("subAgentLib.imageWrong"));
      return;
    }
    if (file.size > MAX_FIGURE_BYTES) {
      setMessage(t("subAgentLib.imageTooLarge"));
      return;
    }
    try {
      choose({ kind: "image", data: await readFile(file, "base64") });
    } catch (e) {
      setMessage(errorText(e));
    }
  };
  const paste = () => {
    setMessage(null);
    api.pasteSubAgentFigure(name).then(applyFigures).catch(fail);
  };

  return (
    <div className="figure-picker" role="group" aria-label={t("subAgentLib.figureFor", { name })}>
      <div className="figure-grid">
        {BUILTIN_FIGURES.map((id) => {
          const selected = chosen?.kind === "builtin" && chosen.id === id;
          const label = figureName(id, t);
          return (
            <button
              key={id}
              type="button"
              className={selected ? "figure-choice selected" : "figure-choice"}
              aria-pressed={selected}
              aria-label={label}
              title={label}
              onClick={() => {
                choose({ kind: "builtin", id });
              }}
            >
              <AgentFigure figure={{ kind: "builtin", id }} size={72} crop="full" />
            </button>
          );
        })}
      </div>
      <div className="settings-row">
        <button
          type="button"
          className="secondary-button"
          onClick={() => fileInput.current?.click()}
        >
          {t("subAgentLib.ownImage")}
        </button>
        <input
          ref={fileInput}
          type="file"
          accept={FIGURE_TYPES.join(",")}
          className="sr-only"
          tabIndex={-1}
          aria-hidden="true"
          onChange={(event) => {
            const input = event.currentTarget;
            void onFile(input.files?.[0]).finally(() => {
              // Picking the same file again should fire an event again.
              input.value = "";
            });
          }}
        />
        <button type="button" className="secondary-button" onClick={paste}>
          {t("subAgentLib.pasteImage")}
        </button>
        <button
          type="button"
          className="link-button"
          onClick={() => {
            choose({ kind: "default" });
          }}
        >
          {t("subAgentLib.resetFigure")}
        </button>
      </div>
      <p className="muted settings-help" role="status">
        {message ?? t("subAgentLib.figureHelp")}
      </p>
    </div>
  );
}

function NewSubAgent({ onDone }: { onDone: () => void }) {
  const t = useT();
  const [text, setText] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [exists, setExists] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    field.current?.focus();
  }, []);

  const save = (replace: boolean) => {
    setMessage(null);
    setExists(null);
    api
      .createSubAgent(text, replace)
      .then(async () => {
        await refreshLibrary();
        onDone();
      })
      .catch((e: unknown) => {
        const error = parseDefError(e);
        if (error?.kind === "exists") {
          setExists(pastedName(text) ?? "");
        } else if (error?.kind === "invalid" && error.problem !== null) {
          setMessage(t(PROBLEM_TEXTS[error.problem]));
        } else if (error?.kind === "reserved") {
          setMessage(t("subAgentLib.reserved"));
        } else if (error?.kind === "tooLarge") {
          setMessage(t("subAgentLib.problem.tooLarge"));
        } else {
          setMessage(error?.message ?? errorText(e));
        }
      });
  };
  const onFile = async (file: File | undefined) => {
    if (file === undefined) return;
    if (file.size > MAX_DEF_BYTES) {
      setMessage(t("subAgentLib.problem.tooLarge"));
      return;
    }
    try {
      setText(await readFile(file, "text"));
      setMessage(null);
    } catch (e) {
      setMessage(errorText(e));
    }
  };

  return (
    <div className="sub-agent-new">
      <h4 className="settings-subtitle">{t("subAgentLib.new")}</h4>
      <p className="muted settings-help">{t("subAgentLib.newHelp")}</p>
      <textarea
        ref={field}
        className="sub-agent-text-field"
        aria-label={t("subAgentLib.textLabel")}
        placeholder={DEF_TEMPLATE}
        spellCheck={false}
        rows={10}
        value={text}
        onChange={(event) => {
          setText(event.currentTarget.value);
          setExists(null);
        }}
      />
      {exists !== null ? (
        <div className="settings-row sub-agent-confirm" role="group">
          <span>{t("subAgentLib.exists", { name: exists })}</span>
          <button
            type="button"
            className="primary-button danger"
            onClick={() => {
              save(true);
            }}
          >
            {t("subAgentLib.replace")}
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={() => {
              setExists(null);
            }}
          >
            {t("common.cancel")}
          </button>
        </div>
      ) : (
        <div className="settings-row">
          <button
            type="button"
            className="primary-button"
            disabled={text.trim() === ""}
            onClick={() => {
              save(false);
            }}
          >
            {t("subAgentLib.save")}
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={() => fileInput.current?.click()}
          >
            {t("subAgentLib.loadFile")}
          </button>
          <input
            ref={fileInput}
            type="file"
            accept=".md,text/markdown,text/plain"
            className="sr-only"
            tabIndex={-1}
            aria-hidden="true"
            onChange={(event) => {
              const input = event.currentTarget;
              void onFile(input.files?.[0]).finally(() => {
                input.value = "";
              });
            }}
          />
          <button type="button" className="link-button" onClick={onDone}>
            {t("common.cancel")}
          </button>
        </div>
      )}
      {message !== null && (
        <p className="muted error-text" role="status">
          {message}
        </p>
      )}
    </div>
  );
}
