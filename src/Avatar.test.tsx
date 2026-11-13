import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Avatar } from "./Avatar";

const PNG = "data:image/png;base64,iVBORw0KGgo=";

describe("Avatar", () => {
  it("shows the picture in the same round shape when there is one", () => {
    const html = renderToStaticMarkup(<Avatar name="Ada Lovelace" src={PNG} size="small" />);
    expect(html).toBe(
      `<span class="avatar small picture" aria-hidden="true"><img src="${PNG}" alt="" draggable="false"/></span>`,
    );
  });

  it("falls back to the initials without a picture", () => {
    const initials = '<span class="avatar normal" aria-hidden="true">AL</span>';
    expect(renderToStaticMarkup(<Avatar name="Ada Lovelace" />)).toBe(initials);
    for (const src of [null, ""]) {
      expect(renderToStaticMarkup(<Avatar name="Ada Lovelace" src={src} />)).toBe(initials);
    }
    expect(renderToStaticMarkup(<Avatar name="" text="AB" size="mini" session />)).toBe(
      '<span class="avatar mini session" aria-hidden="true">AB</span>',
    );
  });
});
