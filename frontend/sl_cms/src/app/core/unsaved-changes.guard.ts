import { inject } from '@angular/core';
import { CanDeactivateFn } from '@angular/router';
import { TranslocoService } from '@jsverse/transloco';

/**
 * A screen that holds edits its own save button has not sent yet.
 *
 * Asking is the screen's business (it knows what "changed" means for its form), so the guard only
 * asks it and, when the answer is yes, asks the person as well.
 */
export interface HasUnsavedChanges {
  hasUnsavedChanges(): boolean;
}

/**
 * Ask before leaving a screen with unsaved edits.
 *
 * `window.confirm` rather than a dialog: it is the one prompt a browser will not let the page
 * replace or style away, and it is the same question the `beforeunload` handler raises on a
 * reload. Answering "no" keeps the editor and everything in it.
 */
export const unsavedChangesGuard: CanDeactivateFn<HasUnsavedChanges> = (component) => {
  if (!component.hasUnsavedChanges?.()) {
    return true;
  }
  const i18n = inject(TranslocoService);
  return confirm(i18n.translate('content.leaveUnsaved'));
};
