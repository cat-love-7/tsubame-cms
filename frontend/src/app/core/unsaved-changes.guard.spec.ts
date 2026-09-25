import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { TranslocoService, provideTransloco } from '@jsverse/transloco';

import { FALLBACK_LANGUAGE, SUPPORTED_LANGUAGES } from 'app/core/i18n/language';
import { BundledTranslocoLoader } from 'app/core/i18n/transloco.loader';

import { HasUnsavedChanges, unsavedChangesGuard } from './unsaved-changes.guard';

function screen(hasUnsavedChanges: boolean): HasUnsavedChanges {
  return { hasUnsavedChanges: () => hasUnsavedChanges };
}

/** The guard asks the screen, and then the person - in the language the CMS is showing. */
function run(component: HasUnsavedChanges): boolean {
  return TestBed.runInInjectionContext(
    () => unsavedChangesGuard(component, {} as never, {} as never, {} as never) as boolean,
  );
}

describe('unsavedChangesGuard', () => {
  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        provideRouter([]),
        provideTransloco({
          config: {
            availableLangs: [...SUPPORTED_LANGUAGES],
            defaultLang: FALLBACK_LANGUAGE,
            fallbackLang: FALLBACK_LANGUAGE,
            reRenderOnLangChange: true,
            prodMode: false,
          },
          loader: BundledTranslocoLoader,
        }),
      ],
    });
  });

  it('lets a screen with nothing to lose go', () => {
    const confirmSpy = vi.spyOn(window, 'confirm');
    expect(run(screen(false))).toBe(true);
    expect(confirmSpy).not.toHaveBeenCalled();
  });

  it('asks before leaving a screen that has unsaved edits', () => {
    // Leaving is the person's decision, so both answers are the guard's answers.
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    expect(run(screen(true))).toBe(false);

    vi.spyOn(window, 'confirm').mockReturnValue(true);
    expect(run(screen(true))).toBe(true);
  });

  it('asks the question through the catalog', () => {
    // The question is a key the catalogs hold (`core/i18n/keys.spec.ts` fails on one that is not),
    // so the guard reads it rather than carrying a sentence of its own.
    const translate = vi.spyOn(TestBed.inject(TranslocoService), 'translate');
    vi.spyOn(window, 'confirm').mockReturnValue(false);

    run(screen(true));

    expect(translate).toHaveBeenCalledWith('content.leaveUnsaved');
  });
});
