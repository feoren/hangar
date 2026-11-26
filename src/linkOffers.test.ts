import { describe, expect, it } from "vitest";

import { keepOffered, offerMemberNames, parseLinkOffers, shownOffer } from "./linkOffers";

const raw = {
  id: 3,
  members: [
    { session: 11, name: "app", role: "lead" },
    { session: 12, name: "web", role: "member" },
  ],
  missing: ["docs"],
  askFirst: true,
};

describe("parseLinkOffers", () => {
  it("reads the backend's offers", () => {
    expect(parseLinkOffers([raw])).toEqual([
      {
        id: 3,
        members: [
          { session: 11, name: "app", role: "lead" },
          { session: 12, name: "web", role: "member" },
        ],
        missing: ["docs"],
        askFirst: true,
      },
    ]);
  });

  it("leaves out what no offer can be", () => {
    const one = { ...raw, members: [raw.members[0]] };
    const badName = { ...raw, members: [...raw.members, { session: 13, name: "No Name" }] };
    const twice = { ...raw, members: [raw.members[0], { ...raw.members[1], session: 11 }] };
    const noToggle = { ...raw, askFirst: "yes" };
    expect(parseLinkOffers([one, badName, twice, noToggle, null, 7])).toEqual([]);
    expect(parseLinkOffers("x")).toEqual([]);
    // Missing names that are no member name are dropped, the offer stays.
    expect(parseLinkOffers([{ ...raw, missing: ["ok", 5, "\u001b[2J"] }])[0]?.missing).toEqual([
      "ok",
    ]);
    // An id twice: the first counts.
    expect(parseLinkOffers([raw, { ...raw, askFirst: false }])).toHaveLength(1);
  });
});

describe("the bar's offer", () => {
  const offers = parseLinkOffers([raw, { ...raw, id: 4 }]);

  it("shows one offer at a time, none that was decided", () => {
    expect(shownOffer(offers, new Set())?.id).toBe(3);
    expect(shownOffer(offers, new Set([3]))?.id).toBe(4);
    expect(shownOffer(offers, new Set([3, 4]))).toBeNull();
  });

  it("forgets a decision once the backend no longer offers it", () => {
    const decided = new Set([3, 9]);
    expect([...keepOffered(decided, offers)]).toEqual([3]);
    const same = new Set([3]);
    expect(keepOffered(same, offers)).toBe(same);
  });

  it("names members by their session title, else by member name", () => {
    const [first] = offers;
    if (first === undefined) throw new Error("no offer");
    expect(offerMemberNames(first, (id) => (id === 11 ? " Shop " : undefined))).toEqual([
      "Shop",
      "web",
    ]);
  });
});
