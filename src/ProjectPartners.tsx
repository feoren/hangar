import { useId, useState } from "react";

import { useT } from "./i18n";
import { CloseIcon } from "./icons";
import { namedAt, partnersOf, suggestName, validName, type PartnerBook } from "./partners";

export interface PartnerLink {
  thisFolder: string;
  thisName: string;
  partnerFolder: string;
  partnerName: string;
}

export interface PartnerActions {
  /** Links two projects; resolves to the reason when the backend refused, else `null`. */
  onLink: (link: PartnerLink) => Promise<string | null>;
  onUnlink: (a: string, b: string) => void;
  /** The folder dialog; `null` when cancelled. */
  onPickFolder: () => Promise<string | null>;
}

interface Props extends PartnerActions {
  /** The card's folder: the repository's main folder or the plain folder. */
  root: string;
  book: PartnerBook;
}

/**
 * The partner projects of one project card (ADR 0012): the partners by short name, each with a
 * button that ends the pair, and a small form that adds one.
 */
export function ProjectPartners({ root, book, onLink, onUnlink, onPickFolder }: Props) {
  const t = useT();
  const id = useId();
  const own = namedAt(book, root);
  const partners = own === undefined ? [] : partnersOf(book, own.name);
  const [adding, setAdding] = useState(false);
  const [thisName, setThisName] = useState(own?.name ?? suggestName(root));
  const [folder, setFolder] = useState("");
  const [partnerName, setPartnerName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const effectiveName = own?.name ?? thisName;
  const ready = validName(effectiveName) && validName(partnerName) && folder.trim() !== "" && !busy;

  const pick = async () => {
    const picked = await onPickFolder();
    if (picked === null) return;
    setFolder(picked);
    setPartnerName((current) => current || (namedAt(book, picked)?.name ?? suggestName(picked)));
  };

  const submit = async () => {
    if (!ready) return;
    setBusy(true);
    const reason = await onLink({
      thisFolder: root,
      thisName: effectiveName,
      partnerFolder: folder.trim(),
      partnerName,
    });
    setBusy(false);
    setError(reason);
    if (reason !== null) return;
    setAdding(false);
    setFolder("");
    setPartnerName("");
  };

  return (
    <div className="project-block project-partners">
      <h4 className="project-label">{t("partners.title")}</h4>
      {partners.length > 0 && (
        <ul className="project-partner-list">
          {partners.map((partner) => (
            <li key={partner.name} className="project-partner" title={partner.folder}>
              <span className="project-partner-name">{partner.name}</span>
              <button
                type="button"
                className="icon-button"
                aria-label={t("partners.remove", { name: partner.name })}
                title={t("partners.remove", { name: partner.name })}
                onClick={() => {
                  if (own !== undefined) onUnlink(own.name, partner.name);
                }}
              >
                <CloseIcon size={10} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {!adding ? (
        <button
          type="button"
          className="project-link"
          onClick={() => {
            setThisName(own?.name ?? suggestName(root));
            setError(null);
            setAdding(true);
          }}
        >
          {t("partners.add")}
        </button>
      ) : (
        <form
          className="project-partner-form"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          {own === undefined && (
            <label>
              <span>{t("partners.thisName")}</span>
              <input
                className="text-input"
                value={thisName}
                maxLength={32}
                spellCheck={false}
                aria-invalid={!validName(thisName) || undefined}
                onChange={(event) => {
                  setThisName(event.target.value);
                }}
              />
            </label>
          )}
          <label>
            <span>{t("partners.folder")}</span>
            <span className="project-partner-folder">
              <input
                className="text-input"
                value={folder}
                spellCheck={false}
                onChange={(event) => {
                  setFolder(event.target.value);
                }}
              />
              <button type="button" className="secondary-button" onClick={() => void pick()}>
                {t("partners.pick")}
              </button>
            </span>
          </label>
          <label>
            <span>{t("partners.partnerName")}</span>
            <input
              className="text-input"
              value={partnerName}
              maxLength={32}
              spellCheck={false}
              aria-describedby={`${id}-help`}
              aria-invalid={(partnerName !== "" && !validName(partnerName)) || undefined}
              onChange={(event) => {
                setPartnerName(event.target.value);
              }}
            />
          </label>
          <p id={`${id}-help`} className="muted project-partner-help">
            {t("partners.nameHelp")}
          </p>
          {error !== null && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <div className="project-partner-actions">
            <button
              type="button"
              className="secondary-button"
              onClick={() => {
                setAdding(false);
                setError(null);
              }}
            >
              {t("common.cancel")}
            </button>
            <button type="submit" className="primary-button" disabled={!ready}>
              {t("partners.link")}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
