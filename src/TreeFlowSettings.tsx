// Settings > Appearance > Sidebar: the tree flow (treeFlow.ts). One switch; only while it is on,
// the style as a segmented choice and the switch for the rate on the rows. Left out entirely where
// the WebView cannot draw the flow (`treeFlowSupported`), since a switch that does nothing would
// only confuse.

import { useT, type MessageKey } from "./i18n";
import { SegmentedField, SwitchRow } from "./SettingsParts";
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
    <SwitchRow
      label={t("treeFlow.enabled")}
      help={t("treeFlow.enabledHelp")}
      checked={value.enabled}
      onChange={(enabled) => {
        onChange({ ...value, enabled });
      }}
    >
      <SegmentedField
        label={t("treeFlow.style")}
        value={value.style}
        options={TREE_FLOW_STYLES.map((style) => ({
          value: style,
          label: t(TREE_FLOW_STYLE_NAMES[style]),
        }))}
        onChange={(style) => {
          onChange({ ...value, style });
        }}
      />
      <SwitchRow
        label={t("treeFlow.showRate")}
        help={t("treeFlow.showRateHelp")}
        checked={value.showRate}
        onChange={(showRate) => {
          onChange({ ...value, showRate });
        }}
      />
    </SwitchRow>
  );
}
