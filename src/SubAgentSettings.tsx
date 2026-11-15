// Settings, Sub-agents: Claude Code's sub-agent definitions with their figures. One row per
// name (global and project files of that name together), a figure picker below the row whose
// figure was clicked, and a form for a new definition from pasted text or a loaded file.

import { useEffect, useRef, useState } from "react";

import { AgentFigure } from "./AgentFigure";
import { withExtras } from "./agentStartMenu";
import { api, errorText } from "./api";
import { useT, type MessageKey } from "./i18n";
import { folderName } from "./folders";
import {
  BUILTIN_FIGURES,
  DEF_TEMPLATE,
  figureErrorCode,
  FIGURE_TYPES,
  figureFor,
  figureName,
  libraryEntries,
  MAX_DEF_BYTES,
  MAX_FIGURE_BYTES,
  parseDefError,
  pastedName,
  type DefProblemKind,
  type FigureErrorCode,
  type FigureInput,
  type FigureView,
  type LibraryEntry,
} from "./subAgentLibrary";
import { applyFigures, refreshLibrary, useSubAgentLibrary } from "./subAgentLibraryStore";

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

export function SubAgentSettings() {
  const t = useT();
  const { library, error } = useSubAgentLibrary();
  const [picking, setPicking] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const entries = libraryEntries(library);

  // Files may have changed outside Hangar since the last look.
  useEffect(() => {
    void refreshLibrary();
  }, []);

  return (
    <section className="settings-section sub-agent-settings" aria-labelledby="sub-agents-help">
      <p id="sub-agents-help" className="muted settings-intro">
        {t("subAgentLib.intro")}
      </p>
      {error !== null && <p className="muted error-text">{error}</p>}
      {entries.length === 0 && !creating && <p className="muted">{t("subAgentLib.empty")}</p>}
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
      </ul>
      {library.problems.length > 0 && (
        <details className="sub-agent-problems">
          <summary className="muted">
            {t("subAgentLib.problems", { count: library.problems.length })}
          </summary>
          <ul>
            {library.problems.map((problem) => (
              <li key={`${problem.folder ?? ""}/${problem.file}`} className="muted">
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
  const { figures } = useSubAgentLibrary();
  const [confirming, setConfirming] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const figure = figureFor(entry.name, entry.def.color, figures);
  const places = [
    ...(entry.global === null ? [] : [t("subAgentLib.global")]),
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
  const meta = [
    ...places,
    ...(entry.def.model === null ? [] : [t("subAgentLib.model", { model: entry.def.model })]),
  ].join(" · ");
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

  return (
    <li className="sub-agent-row">
      <button
        type="button"
        className="sub-agent-figure-button"
        aria-expanded={picking}
        aria-label={t("subAgentLib.figureFor", { name: entry.name })}
        title={t("subAgentLib.figureFor", { name: entry.name })}
        onClick={onPick}
      >
        <AgentFigure figure={figure} size={56} crop="head" />
      </button>
      <div className="sub-agent-text">
        <div className="sub-agent-head">
          <strong className="sub-agent-name">{entry.name}</strong>
          <span className="muted sub-agent-meta">{meta}</span>
        </div>
        <p className="sub-agent-description" title={entry.def.description}>
          {entry.def.description}
        </p>
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
        {picking && <FigurePicker name={entry.name} chosen={figures.get(entry.name)} />}
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
              <AgentFigure figure={{ kind: "builtin", id }} size={44} crop="head" still />
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
