import { useRef, useState } from "react";

import { useT } from "./i18n";
import { EditIcon, TemplateIcon } from "./icons";
import { PopupMenu, type PopupItem } from "./PopupMenu";
import type { WorkspaceTemplate } from "./templates";

interface Props {
  templates: readonly WorkspaceTemplate[];
  onStart: (id: string) => void;
  onRename: (id: string) => void;
  onDelete: (id: string) => void;
  /** "Save sessions as template"; `null` while no session runs. */
  onSave: (() => void) | null;
  /** A template started or a dialog opens: the caller decides where the focus goes. */
  onAction: () => void;
}

/**
 * The templates button next to the project folder card: one click on a template starts all its
 * sessions. Each row has a pencil to rename it and an x to delete it. Shown only once a template
 * exists; the first one is saved from the File menu or quick access.
 */
export function TemplatesMenu({ templates, onStart, onRename, onDelete, onSave, onAction }: Props) {
  const t = useT();
  const button = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; flipAbove: number } | null>(null);
  if (templates.length === 0) {
    // The last template was deleted while the menu was open.
    if (menu !== null) setMenu(null);
    return null;
  }

  const items: PopupItem[] = [
    { type: "heading", key: "heading", label: t("templates.menu") },
    ...templates.map((template): PopupItem => ({
      type: "action",
      key: template.id,
      label: template.name,
      detail: t("templates.count", { count: template.sessions.length }),
      icon: <TemplateIcon size={14} />,
      title: t("templates.startNamed", { name: template.name }),
      secondary: {
        label: t("templates.renameNamed", { name: template.name }),
        icon: <EditIcon size={14} />,
        run: () => {
          onRename(template.id);
        },
      },
      remove: {
        label: t("templates.deleteNamed", { name: template.name }),
        run: () => {
          onDelete(template.id);
        },
      },
      run: () => {
        onStart(template.id);
      },
    })),
  ];
  if (onSave !== null) {
    items.push(
      { type: "separator", key: "sep" },
      {
        type: "action",
        key: "save",
        label: t("templates.save"),
        run: onSave,
      },
    );
  }

  return (
    <>
      <button
        ref={button}
        type="button"
        className="icon-button cwd-templates"
        aria-label={t("templates.menu")}
        title={t("templates.menu")}
        aria-haspopup="menu"
        aria-expanded={menu !== null}
        onClick={(event) => {
          if (menu !== null) {
            setMenu(null);
            return;
          }
          const rect = event.currentTarget.getBoundingClientRect();
          setMenu({ x: rect.left, y: rect.bottom + 4, flipAbove: rect.top - 4 });
        }}
      >
        <TemplateIcon size={15} />
      </button>
      {menu !== null && (
        <PopupMenu
          items={items}
          label={t("templates.menu")}
          position={menu}
          flipAbove={menu.flipAbove}
          anchor={button}
          className="templates-menu"
          onClose={(reason) => {
            setMenu(null);
            if (reason === "dismiss") button.current?.focus();
            else onAction();
          }}
        />
      )}
    </>
  );
}
