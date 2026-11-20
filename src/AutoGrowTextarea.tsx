import { useCallback, useLayoutEffect, useRef, type ComponentPropsWithRef, type Ref } from "react";

import { fitTextareaHeight } from "./autoGrow";

type Props = Omit<ComponentPropsWithRef<"textarea">, "value"> & { value: string };

function assignRef<T>(ref: Ref<T> | undefined, value: T | null): void {
  if (typeof ref === "function") ref(value);
  else if (ref !== null && ref !== undefined) ref.current = value;
}

/**
 * A text field that grows with its text up to the CSS `max-height` and scrolls beyond it. Dragging
 * its corner (CSS `resize: vertical`) sets a height it keeps, until the text needs more room.
 */
export function AutoGrowTextarea({ ref, value, onPointerDown, ...rest }: Props) {
  const field = useRef<HTMLTextAreaElement | null>(null);
  /** Height the person dragged the field to; 0 while untouched. */
  const floor = useRef(0);
  /** Height set by the last fit, to tell a drag apart from a click into the text. */
  const fitted = useRef(0);

  const fit = useCallback(() => {
    const element = field.current;
    if (element === null) return;
    fitTextareaHeight(element, floor.current);
    // The rendered height, after CSS min-height and max-height.
    fitted.current = element.offsetHeight;
  }, []);

  useLayoutEffect(fit, [fit, value]);

  // A new width rewraps the text (window, dialog, a field that was hidden and is shown now).
  useLayoutEffect(() => {
    const element = field.current;
    if (element === null || typeof ResizeObserver === "undefined") return;
    let width = element.clientWidth;
    const observer = new ResizeObserver(() => {
      if (element.clientWidth === width) return;
      width = element.clientWidth;
      fit();
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [fit]);

  const setRef = useCallback(
    (element: HTMLTextAreaElement | null) => {
      field.current = element;
      assignRef(ref, element);
    },
    [ref],
  );

  return (
    <textarea
      {...rest}
      ref={setRef}
      value={value}
      onPointerDown={(event) => {
        onPointerDown?.(event);
        const element = event.currentTarget;
        window.addEventListener(
          "pointerup",
          () => {
            const height = element.offsetHeight;
            if (height === fitted.current) return;
            floor.current = height;
            fitted.current = height;
          },
          { once: true },
        );
      }}
    />
  );
}
