// Settings > Appearance: the tree flow (treeFlow.ts). One switch; only while it is on, the style
// as a segmented choice and the switch for the rate on the rows. Left out entirely where the
// WebView cannot draw the flow (`treeFlowSupported`), since a switch that does nothing would only
// confuse.

import { useT, type MessageKey } from "./i18n";
import { TREE_FLOW_STYLES, type TreeFlowPrefs, type TreeFlowStyle } from "./treeFlow";

export const TREE_FLOW_STYLE_NAMES: Record<TreeFlowStyle, MessageKey> = {
  stream: "treeFlow.styleStream",
  pulse: "treeFlow.stylePulse",
  dots: "treeFlow.styleDots",
};

export function TreeFlowField({
  value,
  supported,
  onChange,
}: {
  value: TreeFlowPrefs;
  /** The WebView can draw the flow; without, the field is not shown. */
  supported: boolean;
  onChange: (value: TreeFlowPrefs) => void;
}) {
  const t = useT();
  if (!supported) return null;
  return (
    <section className="settings-section tree-flow-settings" aria-labelledby="settings-tree-flow">
      <h4 id="settings-tree-flow" className="settings-subtitle">
        {t("treeFlow.title")}
      </h4>
      <label className="check">
        <input
          type="checkbox"
          checked={value.enabled}
          aria-describedby="tree-flow-help"
          onChange={(event) => {
            onChange({ ...value, enabled: event.currentTarget.checked });
          }}
        />
        <span>{t("treeFlow.enabled")}</span>
      </label>
      <p id="tree-flow-help" className="muted settings-help">
        {t("treeFlow.enabledHelp")}
      </p>
      {value.enabled && (
        <>
          <div className="field">
            <span id="settings-tree-flow-style">{t("treeFlow.style")}</span>
            <div
              className="segmented"
              role="radiogroup"
              aria-labelledby="settings-tree-flow-style"
              aria-describedby="settings-tree-flow-style-help"
            >
              {TREE_FLOW_STYLES.map((style) => (
                <label key={style} className="segment">
                  <input
                    type="radio"
                    name="tree-flow-style"
                    value={style}
                    checked={value.style === style}
                    onChange={() => {
                      onChange({ ...value, style });
                    }}
                  />
                  <span>{t(TREE_FLOW_STYLE_NAMES[style])}</span>
                </label>
              ))}
            </div>
            <p id="settings-tree-flow-style-help" className="muted">
              {t("treeFlow.styleHelp")}
            </p>
          </div>
          <label className="check">
            <input
              type="checkbox"
              checked={value.showRate}
              aria-describedby="tree-flow-rate-help"
              onChange={(event) => {
                onChange({ ...value, showRate: event.currentTarget.checked });
              }}
            />
            <span>{t("treeFlow.showRate")}</span>
          </label>
          <p id="tree-flow-rate-help" className="muted settings-help">
            {t("treeFlow.showRateHelp")}
          </p>
        </>
      )}
    </section>
  );
}
