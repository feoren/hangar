import { useLayoutEffect, type RefObject } from "react";

/**
 * Puts a scroll area back at its top when it appears and whenever `page` changes (a dialog's
 * section), before the browser paints, so a new page never opens halfway down. The same page
 * keeps its place across renders.
 */
export function useScrollToTop(area: RefObject<HTMLElement | null>, page: string): void {
  useLayoutEffect(() => {
    const element = area.current;
    element?.scrollTo({ top: 0 });
  }, [area, page]);
}
