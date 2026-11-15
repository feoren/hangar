// A web address as the link notices show it: whole and selectable, with the host in bold, since
// the host is the part that decides where the address leads (termSafety.ts `addressParts`).

import { addressParts } from "./termSafety";

export function LinkAddress({ url }: { url: string }) {
  const { before, host, after } = addressParts(url);
  return (
    <span className="link-url selectable">
      {before}
      {host !== "" && <strong className="link-host">{host}</strong>}
      {after}
    </span>
  );
}
