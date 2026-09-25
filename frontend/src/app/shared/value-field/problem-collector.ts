import { Message } from 'app/core/i18n/message';

/**
 * The problems the sub-editors of one field report, so one clearing does not clear another's.
 *
 * A composite reports the problems of its sub-fields, an array the problems of its elements, and
 * the editor above shows the first of them and refuses to save while any remain. Collecting them
 * by key is what makes that possible: naming the others too would only lengthen the message, and
 * one editor clearing its own problem must not clear the problem of its neighbour.
 */
export class ProblemCollector {
  private problems: { [key: string]: Message } = {};

  /** Where the first problem left goes, on every change. */
  constructor(private readonly report: (problem: Message | null) => void) {}

  /** Note, or clear, the problem a sub-editor reports under `key`. */
  set(key: string, problem: Message | null) {
    if (problem) {
      this.problems[key] = problem;
    } else {
      delete this.problems[key];
    }
    this.emit();
  }

  /** Forget every problem whose key starts with `prefix` (the positions that no longer exist). */
  clear(prefix: string) {
    for (const key of Object.keys(this.problems)) {
      if (key.startsWith(prefix)) {
        delete this.problems[key];
      }
    }
    this.emit();
  }

  private emit() {
    this.report(Object.values(this.problems)[0] ?? null);
  }
}
