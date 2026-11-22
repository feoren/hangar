// Settings, Sub-agents: Claude Code's sub-agent definitions with their figures and models, Codex
// CLI's own definitions, the default model per program and the copies of the user's
// definitions in other programs. The list comes first (what people come here for), with a form
// for a new definition right below it; the display, the default models and which files Hangar
// adds follow as groups of settings.

import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";

import { AgentFigure } from "./AgentFigure";
import { withExtras } from "./agentStartMenu";
import { api, errorText } from "./api";
import { useT, type MessageKey } from "./i18n";
import { ChevronIcon, CloseIcon, GridIcon, RowsIcon } from "./icons";
import { folderName } from "./folders";
import type { Settings as AppSettings } from "./settings";
import { Advanced, SettingRow, SettingsGroup, Switch, SwitchRow } from "./SettingsParts";
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
  offeredEffort,
  ownCodexDefs,
  parseSubAgentSettings,
  provides,
  modelName,
  withBundled,
  withModel,
  withProvide,
  type DefProgram,
  type ModelChoice,
  type SubAgentSettings,
} from "./subAgentModels";
import {
  AGENT_LIST_VIEWS,
  AGENT_SIDES,
  agentItems,
  loadAgentListPrefs,
  pickSide,
  saveAgentListPrefs,
  splitBySide,
  type AgentItem,
  type AgentListPrefs,
  type AgentListView,
  type AgentSide,
} from "./subAgentView";

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

export function SubAgentSettings({
  settings: appSettings,
  onChange,
}: {
  settings: AppSettings;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const t = useT();
  const { library, error } = useSubAgentLibrary();
  const [creating, setCreating] = useState(false);
  const newButton = useRef<HTMLButtonElement>(null);
  // The form goes away with the focus in it: it returns to the button that opened it.
  const returnFocus = useRef(false);
  const [settings, setSettings] = useState<SubAgentSettings | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  // Files may have changed outside Hangar since the last look; the copies follow them.
  useEffect(() => {
    void syncLibrary();
  }, []);

  useEffect(() => {
    if (creating || !returnFocus.current) return;
    returnFocus.current = false;
    newButton.current?.focus();
  }, [creating]);

  useEffect(() => {
    let live = true;
    api
      .subAgentSettings()
      .then((value) => {
        if (live) setSettings(parseSubAgentSettings(value));
      })
      .catch((e: unknown) => {
        if (live) setSettingsError(errorText(e));
      });
    return () => {
      live = false;
    };
  }, []);

  const saveSettings = (next: SubAgentSettings) => {
    if (settings === null) return;
    setSettingsError(null);
    const before = settings;
    setSettings(next);
    api
      .saveSubAgentSettings(next)
      .then(() => syncLibrary())
      .catch((e: unknown) => {
        setSettings(before);
        setSettingsError(errorText(e));
      });
  };

  return (
    <div className="sub-agent-settings">
      <p className="muted settings-intro">{t("subAgentLib.intro")}</p>
      {error !== null && <p className="muted error-text">{error}</p>}
      <AgentBrowser library={library} bundledOff={settings?.bundledOff === true} />
      {library.problems.length > 0 && (
        <Advanced summary={t("subAgentLib.problems", { count: library.problems.length })}>
          <ul className="sub-agent-problem-list">
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
        </Advanced>
      )}
      {creating ? (
        <NewSubAgent
          onDone={() => {
            returnFocus.current = true;
            setCreating(false);
          }}
        />
      ) : (
        <div className="settings-row">
          <button
            ref={newButton}
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
      <SettingsGroup title={t("settings.groupSubAgentDisplay")}>
        <SwitchRow
          label={t("settings.crewColumn")}
          help={t("settings.crewColumnHelp")}
          checked={appSettings.crewColumn}
          onChange={(crewColumn) => {
            onChange({ crewColumn });
          }}
        />
        <SwitchRow
          label={t("settings.subAgentExcerpts")}
          help={t("settings.subAgentExcerptsHelp")}
          checked={appSettings.subAgentExcerpts}
          onChange={(subAgentExcerpts) => {
            onChange({ subAgentExcerpts });
          }}
        />
      </SettingsGroup>
      <ModelDefaults
        library={library}
        settings={settings}
        message={settingsError}
        onSave={saveSettings}
      />
    </div>
  );
}

const SIDE_TEXTS: Record<AgentSide, MessageKey> = {
  hangar: "subAgentLib.side.hangar",
  others: "subAgentLib.side.others",
};

const VIEW_TEXTS: Record<AgentListView, MessageKey> = {
  grid: "subAgentLib.view.grid",
  table: "subAgentLib.view.table",
};

const VIEW_ICONS: Record<AgentListView, ReactNode> = {
  grid: <GridIcon />,
  table: <RowsIcon />,
};

/**
 * The definitions on two sides of a switch (Hangar's own, every other one) as cards or as a
 * table. Clicking an entry opens its details below it; one at a time.
 */
function AgentBrowser({ library, bundledOff }: { library: Library; bundledOff: boolean }) {
  const t = useT();
  const [prefs, setPrefs] = useState<AgentListPrefs>(loadAgentListPrefs);
  const [openKey, setOpenKey] = useState<string | null>(null);
  // A side clicked here is shown even when empty, so its empty note can say why; only the side
  // remembered from last time gives way to one with entries.
  const [clicked, setClicked] = useState<AgentSide | null>(null);
  const sides = splitBySide(agentItems(libraryEntries(library), ownCodexDefs(library)));
  const side =
    clicked ?? pickSide(prefs.side, { hangar: sides.hangar.length, others: sides.others.length });
  const items = sides[side];

  const change = (next: AgentListPrefs) => {
    setPrefs(next);
    saveAgentListPrefs(next);
  };
  const toggle = (key: string) => {
    setOpenKey((current) => (current === key ? null : key));
  };
  const close = () => {
    setOpenKey(null);
  };

  return (
    <div className="sub-agent-browser">
      <div className="sub-agent-toolbar">
        <div
          className="segmented sub-agent-sides"
          role="radiogroup"
          aria-label={t("subAgentLib.side")}
        >
          {AGENT_SIDES.map((option) => (
            <label key={option} className="segment">
              <input
                type="radio"
                name="sub-agent-side"
                value={option}
                checked={side === option}
                onChange={() => {
                  setOpenKey(null);
                  setClicked(option);
                  change({ ...prefs, side: option });
                }}
              />
              <span>{t(SIDE_TEXTS[option])}</span>
              <span className="sub-agent-count">{sides[option].length}</span>
            </label>
          ))}
        </div>
        <div
          className="segmented sub-agent-views"
          role="radiogroup"
          aria-label={t("subAgentLib.view")}
        >
          {AGENT_LIST_VIEWS.map((option) => (
            <label key={option} className="segment" title={t(VIEW_TEXTS[option])}>
              <input
                type="radio"
                name="sub-agent-view"
                value={option}
                checked={prefs.view === option}
                aria-label={t(VIEW_TEXTS[option])}
                onChange={() => {
                  change({ ...prefs, view: option });
                }}
              />
              {VIEW_ICONS[option]}
            </label>
          ))}
        </div>
      </div>
      {items.length === 0 ? (
        <p className="muted sub-agent-empty">
          {side === "others"
            ? t("subAgentLib.emptyOthers")
            : t(bundledOff ? "subAgentLib.emptyHangarOff" : "subAgentLib.emptyHangar")}
        </p>
      ) : prefs.view === "grid" ? (
        <AgentGrid items={items} openKey={openKey} onToggle={toggle} onClose={close} />
      ) : (
        <AgentTable items={items} openKey={openKey} onToggle={toggle} onClose={close} />
      )}
    </div>
  );
}

interface ListProps {
  items: AgentItem[];
  openKey: string | null;
  onToggle: (key: string) => void;
  onClose: () => void;
}

/** Id of an entry's details, for `aria-controls`; keys hold characters an id should not. */
function detailsId(item: AgentItem): string {
  return `sub-agent-details-${item.key.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

function modelText(model: string | null, t: ReturnType<typeof useT>): string {
  return model === null ? t("subAgentLib.modelDefault") : modelName(model);
}

/** What an entry shows in the list: its figure, one short place and its models. */
function useItemSummary(item: AgentItem) {
  const t = useT();
  const { library, figures } = useSubAgentLibrary();
  if (item.kind === "codex") {
    const { def } = item;
    return {
      figure: initialFigure(def.name, null),
      description: def.description,
      place: `${PROGRAM_NAMES.codex} · ${
        def.folder === null
          ? t("subAgentLib.global")
          : t("subAgentLib.project", { folder: folderName(def.folder) })
      }`,
      claudeModel: null,
      codexModel: modelText(def.model, t),
    };
  }
  const { entry } = item;
  const def = entry.global ?? entry.def;
  // Copies exist of global definitions only, and only for programs Hangar hands them to.
  const copy =
    entry.global === null || !provides(library, "codex")
      ? undefined
      : copiesOf(library, entry.name).find((each) => each.scope === "global");
  return {
    figure: defFigure(def, library, figures),
    description: entry.def.description,
    place:
      entry.global !== null
        ? t("subAgentLib.global")
        : t("subAgentLib.project", { folder: folderName(entry.def.folder ?? "") }),
    claudeModel: modelText(def.model, t),
    codexModel: copy === undefined ? null : modelText(copy.model, t),
  };
}

function AgentGrid({ items, openKey, onToggle, onClose }: ListProps) {
  return (
    <ul className="sub-agent-grid">
      {items.map((item) => {
        const open = openKey === item.key;
        return (
          <Fragment key={item.key}>
            <li className="sub-agent-cell">
              <AgentCard
                item={item}
                open={open}
                onToggle={() => {
                  onToggle(item.key);
                }}
              />
            </li>
            {/* Spans the whole grid; `dense` lets the rest of the card's row fill in above it. */}
            {open && (
              <li className="sub-agent-cell wide">
                <AgentDetails item={item} onClose={onClose} />
              </li>
            )}
          </Fragment>
        );
      })}
    </ul>
  );
}

function AgentCard({
  item,
  open,
  onToggle,
}: {
  item: AgentItem;
  open: boolean;
  onToggle: () => void;
}) {
  const summary = useItemSummary(item);
  const model = item.kind === "codex" ? summary.codexModel : summary.claudeModel;
  return (
    <button
      type="button"
      className={open ? "sub-agent-card open" : "sub-agent-card"}
      aria-expanded={open}
      aria-controls={open ? detailsId(item) : undefined}
      onClick={onToggle}
    >
      <AgentFigure figure={summary.figure} size={96} crop="full" />
      <span className="sub-agent-card-name">{item.name}</span>
      <span className="sub-agent-card-description">{summary.description}</span>
      <span className="muted sub-agent-card-meta">
        {summary.place}
        {model !== null && ` · ${model}`}
      </span>
    </button>
  );
}

function AgentTable({ items, openKey, onToggle, onClose }: ListProps) {
  const t = useT();
  const { library } = useSubAgentLibrary();
  // A Codex column only where Codex CLI is installed or one of its own definitions is listed.
  const codex = library.installed.includes("codex") || items.some((item) => item.kind === "codex");
  const columns = codex ? 7 : 6;
  return (
    <table className="sub-agent-table">
      <thead>
        <tr>
          <th scope="col" className="col-figure">
            <span className="sr-only">{t("subAgentLib.col.figure")}</span>
          </th>
          <th scope="col" className="col-name">
            {t("subAgentLib.col.name")}
          </th>
          <th scope="col" className="col-description">
            {t("subAgentLib.col.description")}
          </th>
          <th scope="col" className="col-place">
            {t("subAgentLib.col.place")}
          </th>
          <th scope="col" className="col-model">
            {PROGRAM_NAMES.claude}
          </th>
          {codex && (
            <th scope="col" className="col-model col-codex">
              {PROGRAM_NAMES.codex}
            </th>
          )}
          <th scope="col" className="col-toggle">
            <span className="sr-only">{t("subAgentLib.col.details")}</span>
          </th>
        </tr>
      </thead>
      <tbody>
        {items.map((item) => {
          const open = openKey === item.key;
          return (
            <Fragment key={item.key}>
              <AgentTableRow
                item={item}
                open={open}
                codex={codex}
                onToggle={() => {
                  onToggle(item.key);
                }}
              />
              {open && (
                <tr className="sub-agent-table-details">
                  <td colSpan={columns}>
                    <AgentDetails item={item} onClose={onClose} />
                  </td>
                </tr>
              )}
            </Fragment>
          );
        })}
      </tbody>
    </table>
  );
}

function AgentTableRow({
  item,
  open,
  codex,
  onToggle,
}: {
  item: AgentItem;
  open: boolean;
  codex: boolean;
  onToggle: () => void;
}) {
  const summary = useItemSummary(item);
  return (
    // The name's button opens the details for the keyboard; the whole row does for the mouse.
    <tr className={open ? "sub-agent-table-row open" : "sub-agent-table-row"} onClick={onToggle}>
      <td className="col-figure">
        <AgentFigure figure={summary.figure} size={32} crop="head" />
      </td>
      <td className="col-name">
        <button
          type="button"
          className="sub-agent-table-name"
          aria-expanded={open}
          aria-controls={open ? detailsId(item) : undefined}
          onClick={(event) => {
            event.stopPropagation();
            onToggle();
          }}
        >
          {item.name}
        </button>
      </td>
      <td className="col-description muted" title={summary.description}>
        {summary.description}
      </td>
      <td className="col-place muted">{summary.place}</td>
      <td className="col-model" title={summary.claudeModel ?? undefined}>
        {summary.claudeModel ?? ""}
      </td>
      {codex && (
        <td className="col-model col-codex" title={summary.codexModel ?? undefined}>
          {summary.codexModel ?? ""}
        </td>
      )}
      <td className="col-toggle">
        <span className={open ? "sub-agent-chevron open" : "sub-agent-chevron"}>
          <ChevronIcon />
        </span>
      </td>
    </tr>
  );
}

/** The opened entry: everything a row used to show, with its fields and actions. */
function AgentDetails({ item, onClose }: { item: AgentItem; onClose: () => void }) {
  const t = useT();
  return (
    <div
      id={detailsId(item)}
      className="sub-agent-details"
      role="group"
      aria-label={t("subAgentLib.details", { name: item.name })}
    >
      <button
        type="button"
        className="icon-button sub-agent-details-close"
        aria-label={t("common.close")}
        title={t("common.close")}
        onClick={() => {
          // The details go away with the focus in them: it returns to the card or name that
          // opened them, not to the start of the window.
          const opener = document.querySelector<HTMLElement>(
            `[aria-controls="${detailsId(item)}"]`,
          );
          onClose();
          opener?.focus();
        }}
      >
        <CloseIcon />
      </button>
      {item.kind === "claude" ? (
        <ClaudeDetails entry={item.entry} />
      ) : (
        <CodexDetails def={item.def} />
      )}
    </div>
  );
}

/**
 * The default model of every sub-agent per installed program, and which definitions Hangar adds
 * to the programs' folders. Only programs installed here are shown.
 */
function ModelDefaults({
  library,
  settings,
  message,
  onSave: save,
}: {
  library: Library;
  settings: SubAgentSettings | null;
  message: string | null;
  onSave: (next: SubAgentSettings) => void;
}) {
  const t = useT();
  if (settings === null || library.installed.length === 0) {
    return message === null ? null : <p className="muted error-text">{message}</p>;
  }
  const suggestions = codexSuggestions(library, library.codexModels);
  return (
    <>
      <SettingsGroup title={t("subAgentLib.defaults")} description={t("subAgentLib.defaultsHelp")}>
        {library.installed.map((program) => (
          <SettingRow
            key={program}
            label={PROGRAM_NAMES[program]}
            control={() => (
              <ModelField
                program={program}
                value={settings.models[program] ?? NO_CHOICE}
                suggestions={program === "codex" ? suggestions : []}
                label={t("subAgentLib.defaultFor", { program: PROGRAM_NAMES[program] })}
                onChange={(choice) => {
                  save(withModel(settings, program, choice));
                }}
              />
            )}
          />
        ))}
      </SettingsGroup>
      <SettingsGroup title={t("subAgentLib.files")}>
        <SwitchRow
          label={t("subAgentLib.provideBundled")}
          help={t("subAgentLib.provideBundledHelp")}
          checked={!settings.bundledOff}
          onChange={(on) => {
            save(withBundled(settings, on));
          }}
        />
        {library.targets.map((target) => {
          const issues = failures(library, target.program);
          // Not a SwitchRow: what the last copy run left alone stays listed even while off.
          return (
            <SettingRow
              key={target.program}
              label={t("subAgentLib.provide", { program: PROGRAM_NAMES[target.program] })}
              help={t("subAgentLib.provideHelp")}
              clickable
              control={({ labelId, helpId }) => (
                <Switch
                  checked={!settings.provideOff.includes(target.program)}
                  labelledBy={labelId}
                  describedBy={helpId}
                  onChange={(on) => {
                    save(withProvide(settings, target.program, on));
                  }}
                />
              )}
            >
              {issues.length > 0 && <IssueList issues={issues} />}
            </SettingRow>
          );
        })}
      </SettingsGroup>
      {message !== null && <p className="muted error-text">{message}</p>}
    </>
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
        // A file may hold an effort Hangar does not offer (`minimal`); changing only the model
        // keeps it, and the backend refuses it, so the hint names the effort, not the model.
        if (error?.kind === "model" && !offeredEffort(choice.effort)) {
          onError(t("subAgentLib.effortNotOffered"));
        } else if (error?.kind === "model") onError(t("subAgentLib.modelInvalid"));
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

/** A Claude Code definition opened: places, models, copies, figure and file actions. */
function ClaudeDetails({ entry }: { entry: LibraryEntry }) {
  const t = useT();
  const { library, figures } = useSubAgentLibrary();
  const [confirming, setConfirming] = useState(false);
  const [picking, setPicking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const models = useModelChange(setProblem);
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
    <>
      <div className="sub-agent-head">
        <strong className="sub-agent-name">{entry.name}</strong>
        <span className="muted sub-agent-meta">{places.join(" · ")}</span>
      </div>
      <p className="sub-agent-details-description">{entry.def.description}</p>
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
        {/* Only Hangar's own agents wear figures, so only they can pick one. */}
        {isHangarAgent(entry.name) && (
          <button
            type="button"
            className="link-button"
            aria-expanded={picking}
            onClick={() => {
              setPicking((value) => !value);
            }}
          >
            {t("subAgentLib.chooseFigure")}
          </button>
        )}
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
    </>
  );
}

/** A Codex CLI definition of the user's own: its model and effort; it never starts as an agent. */
function CodexDetails({ def }: { def: CodexDef }) {
  const t = useT();
  const { library } = useSubAgentLibrary();
  const [problem, setProblem] = useState<string | null>(null);
  const models = useModelChange(setProblem);
  const place =
    def.folder === null
      ? t("subAgentLib.global")
      : t("subAgentLib.project", { folder: folderName(def.folder) });
  return (
    <>
      <div className="sub-agent-head">
        <strong className="sub-agent-name">{def.name}</strong>
        <span className="sub-agent-badge">{PROGRAM_NAMES.codex}</span>
        <span className="muted sub-agent-meta">{place}</span>
      </div>
      <p className="sub-agent-details-description">{def.description}</p>
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
    </>
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
      <p className="muted figure-help" role="status">
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
    <SettingsGroup title={t("subAgentLib.new")} description={t("subAgentLib.newHelp")}>
      <div className="sub-agent-new">
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
    </SettingsGroup>
  );
}
