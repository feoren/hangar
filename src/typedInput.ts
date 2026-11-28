// Fans each line the user submits out to everything in a session that reads typed input. One
// tracker for all of them: two trackers fed from different call sites drift apart as soon as one
// input path (deleting a selection, Enter from a button) reaches only one of them.

import { promptWorthShowing } from "./lastPrompt";
import { PromptLineTracker } from "./promptLine";

/** Everything that reads the lines a session's user submits. */
export interface TypedLineConsumers {
  /** Every line, in shells too: the handover and the timeline judge it. */
  line(line: string, agent: boolean): void;
  /**
   * Agent sessions only: the lines of one chunk worth showing in the last prompt bar, cleaned
   * (`promptWorthShowing`). Called once per chunk and only when there is at least one.
   */
  lastPrompts(texts: readonly string[]): void;
}

export class TypedInput {
  private readonly tracker = new PromptLineTracker();

  constructor(private readonly consumers: TypedLineConsumers) {}

  /** Input as it reached the program: typed keys, pasted text, keys the app sent for the user. */
  feed(data: string, agent: boolean): void {
    const lines = this.tracker.feed(data);
    if (lines.length === 0) return;
    for (const line of lines) this.consumers.line(line, agent);
    // A shell's command line is no prompt to come back to.
    if (!agent) return;
    const texts = lines.map(promptWorthShowing).filter((text) => text !== null);
    if (texts.length > 0) this.consumers.lastPrompts(texts);
  }
}
