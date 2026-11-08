import { useT } from "./i18n";
import { versionParts } from "./version";

/** "0.12.125-5655" followed by "beta" as a small badge while the major version is 0. */
export function VersionText({ version, className }: { version: string; className?: string }) {
  const t = useT();
  const { number, beta } = versionParts(version);
  return (
    <span className={className ? `version-text ${className}` : "version-text"}>
      {number}
      {beta && <span className="beta-badge">{t("version.beta")}</span>}
    </span>
  );
}
