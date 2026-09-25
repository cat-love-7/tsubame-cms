import { Component, Input, computed, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe } from '@jsverse/transloco';

import { Message, MessagePipe } from 'app/core/i18n/message';

/** How long a confirmation stays before it fades, in milliseconds. */
const NOTICE_LIFETIME_MS = 5000;

/**
 * The message that follows an action, at the top of the screen.
 *
 * A screen's own messages are rendered where the screen is, which is no use when the screen is a
 * long form and the action was taken at its bottom: measured on a collection's schema, saving from
 * the bottom left the message 1888 pixels above the viewport. This floats instead, so it is read
 * wherever the reader is looking.
 *
 * Two kinds of message, treated differently:
 *
 * - **A confirmation** (`notice`) is transient: it fades after a few seconds, and goes when it is
 *   clicked. Nothing is lost by missing it.
 * - **A failure** (`error`) stays until it is dismissed, and is *not* the only place the failure is
 *   said: the screen keeps its own error block, which is where the way out (a retry, a corrected
 *   input) lives. That block is `role="alert"` and is announced whether or not it is on screen, so
 *   this copy of it is hidden from assistive technology rather than read out twice.
 *
 * Dismissing is local: a screen's own signal is what decides whether there *is* a failure, and
 * clearing it here would take away the error block and its retry button.
 */
@Component({
  selector: 'app-notice-toast',
  // A class on the host, so a screen (and the end-to-end suite) can address the message without
  // knowing how it is drawn.
  host: { class: 'notice-toast' },
  imports: [MatButtonModule, MatIconModule, MatTooltipModule, MessagePipe, TranslocoPipe],
  templateUrl: './notice-toast.html',
  styleUrl: './notice-toast.scss',
})
export class NoticeToast {
  /** What the action confirmed. Setting it puts the message up again, even a repeated one. */
  @Input() set notice(message: Message | null) {
    this.showing.set(message);
    clearTimeout(this.fade);
    const notice = message;
    if (notice !== null) {
      this.fade = setTimeout(() => {
        // Only while it is still this message: a newer one has the timer of its own.
        if (this.showing() === notice) {
          this.showing.set(null);
        }
      }, NOTICE_LIFETIME_MS);
    }
  }

  /** What failed. Shown instead of the confirmation, and never fades on its own. */
  @Input() set error(message: Message | null) {
    this.failed.set(message);
  }

  /** What the screen reported, as it arrived. */
  private readonly failed = signal<Message | null>(null);
  /** The confirmation on screen, which its own timer takes away. */
  private readonly showing = signal<Message | null>(null);
  /** The message the reader has put away. */
  private readonly dismissed = signal<Message | null>(null);
  private fade?: ReturnType<typeof setTimeout>;

  /** The failure, until it is dismissed. */
  public readonly failure = computed(() => {
    const error = this.failed();
    return error !== null && error !== this.dismissed() ? error : null;
  });

  /** The confirmation, until it fades or is dismissed. */
  public readonly confirmation = computed(() => {
    const notice = this.showing();
    return notice !== null && notice !== this.dismissed() ? notice : null;
  });

  /** What to draw: a failure outranks a confirmation, being the one that needs acting on. */
  public readonly message = computed(() => this.failure() ?? this.confirmation());

  /** Whether what is on screen is a failure, which is what decides how it is announced. */
  public readonly isFailure = computed(() => this.failure() !== null);

  /** Put the message away. The screen's own state is untouched (see the class comment). */
  dismiss() {
    clearTimeout(this.fade);
    this.dismissed.set(this.message());
  }
}
