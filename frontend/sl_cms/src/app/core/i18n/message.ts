import { ChangeDetectorRef, OnDestroy, Pipe, PipeTransform, inject } from '@angular/core';
import { Subscription, switchMap } from 'rxjs';

import { TranslocoService } from '@jsverse/transloco';

import { errorKey, errorMessage, isStatusError } from '../http-error';

/**
 * A piece of text for the screen to show.
 *
 * Kept in this shape rather than as rendered text so that a message already on screen follows a
 * language change: `key` is looked up in the catalogs when it is drawn, with `params` filled in,
 * and `text` is shown as it came — which is what a server error this client has no code for
 * deserves. `cat doc/i18n.md`.
 */
export type Message = { key: string; params?: Record<string, unknown> } | { text: string };

/** Our own wording, `key`, with `params` filled in when the message is rendered. */
export function t(key: string, params?: Record<string, unknown>): Message {
  return params ? { key, params } : { key };
}

/**
 * What to show for a failure that happened while doing something named by `siteKey`.
 *
 * A situational code is the whole reason, so its own wording is shown. Otherwise the server's
 * message — the sentence that says which input was wrong, which item was missing — is kept, and
 * only the screen's own context is translated around it.
 */
export function failure(siteKey: string, error: unknown): Message {
  const key = errorKey(error);
  if (key && !isStatusError(error)) {
    return { key };
  }
  return { key: siteKey, params: { message: errorMessage(error) } };
}

/**
 * What to show for a failure with no useful context in front of it.
 *
 * The wording of a known code, or the server's message verbatim when the code is one this client
 * does not know: the newest code cannot be a reason to show nothing.
 */
export function apiMessage(error: unknown): Message {
  const key = errorKey(error);
  return key ? { key } : { text: errorMessage(error) };
}

/**
 * Renders a [`Message`].
 *
 * Impure, and it loads the catalog for the active language itself: a screen whose only translated
 * text is the message would otherwise draw the key, because it is the `transloco` pipe next to it
 * that normally asks for the file (the load is cached, so asking again costs nothing). It also
 * listens to language changes for the same reason Transloco's own pipe does — an OnPush view
 * holding an error has to be checked again, or the message stays in the old language.
 */
@Pipe({ name: 'message', pure: false })
export class MessagePipe implements PipeTransform, OnDestroy {
  private transloco = inject(TranslocoService);
  private cdr = inject(ChangeDetectorRef);
  private subscription: Subscription;

  constructor() {
    this.subscription = this.transloco.langChanges$
      .pipe(switchMap((language) => this.transloco.load(language)))
      .subscribe(() => this.cdr.markForCheck());
  }

  transform(message: Message | null | undefined): string {
    if (!message) {
      return '';
    }
    return 'text' in message
      ? message.text
      : this.transloco.translate(message.key, message.params);
  }

  ngOnDestroy(): void {
    this.subscription.unsubscribe();
  }
}
