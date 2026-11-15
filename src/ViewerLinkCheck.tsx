// The file viewer's question before it opens a Markdown link whose text claims another address
// (viewerLink.ts): the real address, host in bold, with "Open" and "Cancel". Cancel has the focus,
// so Enter right after the click opens nothing.

import { useEffect, useRef } from "react";

import { useT } from "./i18n";
import { LinkAddress } from "./LinkAddress";

export function ViewerLinkCheck(props: { url: string; onOpen: () => void; onCancel: () => void }) {
  const { url, onOpen, onCancel } = props;
  const t = useT();
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancel.current?.focus();
  }, [url]);
  return (
    <div
      className="viewer-hint viewer-link-check"
      role="alertdialog"
      aria-label={t("link.checkTarget")}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        onCancel();
      }}
    >
      <span>{t("link.checkTarget")}</span>
      <LinkAddress url={url} />
      <span className="viewer-link-actions">
        <button type="button" className="primary-button" onClick={onOpen}>
          {t("link.open")}
        </button>
        <button type="button" className="secondary-button" ref={cancel} onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </span>
    </div>
  );
}
