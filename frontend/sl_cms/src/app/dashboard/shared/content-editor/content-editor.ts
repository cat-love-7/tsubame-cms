import { DestroyRef, computed, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Observable } from 'rxjs';

import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, failure, fieldOf, t } from 'app/core/i18n/message';
import { fingerprint } from 'app/core/value-changes';
import { ItemMetadata } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { FieldValue, FieldValues, withDefaults } from 'app/models/values/fields';
import { copyToClipboard, previewSiteUrl } from 'app/shared/share-link';
import { TranslocoService } from '@jsverse/transloco';

/**
 * The content an editor is looking at, as the API reads and writes it: field name to value.
 *
 * A collection item and a single page hold exactly the same shape - the difference is where it is
 * stored and what it is called - which is what lets the two editors be one editor.
 */
export type EditorValues = FieldValues;

/**
 * Everything one screen's content is, as the editor asks for it.
 *
 * This is the seam between the item editor and the page editor: everything above it - the load
 * generation, the save/publish sequence, the comparison with what is live, discarding the working
 * copy, the preview link - is written once, in [`ContentEditor`], and does not know which of the
 * two it is driving.
 */
export interface ContentSource {
  /**
   * The content this act is about, as one string that changes when the screen moves on.
   *
   * A collection item is `collection/id` and a single page is its name; the editor only compares
   * it, so what it is made of is the source's business.
   */
  address(): string;
  /** Whether the content is on the server yet: a new item is created by the first save. */
  exists(): boolean;
  /**
   * Create the content, answering the id it was given (see [`ContentSource.created`]).
   *
   * Required of a source that can answer "not on the server yet" from [`ContentSource.exists`]: a
   * screen that has a first save has to say what it does with it.
   */
  create?(values: EditorValues): Observable<number>;
  /**
   * Adopt the id a create answered: the content exists now, and its address changes from here.
   *
   * A collection item is named by an id that only exists after the first save; a page is named by
   * its own name and never calls this.
   */
  created?(id: number): void;
  /** Write the working copy of content that exists. */
  update(values: EditorValues): Observable<unknown>;
  loadSchema(): Observable<CollectionSchema>;
  /** The working copy: what the editor saves into. */
  loadValues(): Observable<EditorValues>;
  /** Draft/published state, or `null` for content that is not on the server yet. */
  loadMetadata(): Observable<ItemMetadata | null>;
  /** What the site serves for this content right now. */
  loadPublishedValues(): Observable<EditorValues>;
  /** Whether the schema allows preview links at all. */
  loadPreviewAllowed(): Observable<boolean>;
  setPublished(published: boolean): Observable<ItemMetadata>;
  discardDraft(): Observable<void>;
  createPreviewLink(): Observable<PreviewLink>;
}

/** The content an act was started for, and the load it belongs to. */
interface StartedContent {
  address: string;
  generation: number;
}

/** What the editor needs from the screen it is driving. */
export interface ContentEditorDeps {
  i18n: TranslocoService;
  capabilities: CapabilitiesService;
  dates: DateTimeFormat;
  destroyRef: DestroyRef;
}

/**
 * The state and the acts of one content editor.
 *
 * The item editor and the single-page editor were the same screen written twice - 66% of their
 * lines were identical, and every change had to be made in both (the comparison with what is live,
 * discarding the working copy, the preview link, the messages). This is that screen's logic, once,
 * over a [`ContentSource`] that says which resource it is.
 *
 * The signals are the screen's: a component re-exposes them (as its own fields) so its template and
 * its specs read the same way they did before the extraction.
 */
export class ContentEditor {
  /** The collection or page being edited; the source says which. */
  public readonly name = signal('');
  public readonly schema = signal<CollectionSchema>([]);
  public readonly values = signal<EditorValues>({});
  /** The failure to show, as a key or as the server's own words. */
  public readonly error = signal<Message | null>(null);
  /** The field the last refusal was about, so the form can mark that one input. */
  public readonly problemField = signal<string | null>(null);
  public readonly metadata = signal<ItemMetadata | null>(null);
  public readonly published = computed(() => this.metadata()?.status === 'published');
  /** Published content with changes waiting: the site is behind the editor. */
  public readonly hasDraft = computed(() => this.metadata()?.has_draft ?? false);
  /** What the site serves, once an editor has asked to see it, and whether that is on screen. */
  public readonly publishedValues = signal<EditorValues | null>(null);
  public readonly comparing = signal(false);
  /** Whether the schema allows preview links, or `null` while that is not known yet. */
  public readonly previewAllowed = signal<boolean | null>(null);
  /** The shareable preview link, once one has been minted. */
  public readonly previewUrl = signal('');
  /** What just happened (saved, a link copied), as a key or the server's own words. */
  public readonly notice = signal<Message | null>(null);
  /**
   * Whether the content has arrived.
   *
   * Until it has, the form holds nothing and the server holds the content: saving now (or typing
   * into a form with nothing in it) would write the empty form over fields nobody touched.
   */
  public readonly loaded = signal(false);

  /**
   * What the form held when it was last in step with the server.
   *
   * Compared by rendering rather than by identity: the widgets rebuild their values as they are
   * edited, so a form nobody touched would still be a different object.
   */
  private saved = signal('');
  /**
   * Guards against an answer for content the reader has left.
   *
   * The sidebar switches collections and pages without leaving the route, so a slow answer used to
   * arrive after the switch and be written over what is on screen now.
   */
  private loadToken = 0;
  /** Per-field problems reported by the value widgets; saving is refused while any remain. */
  private fieldErrors: { [field: string]: Message } = {};

  constructor(
    private readonly source: ContentSource,
    private readonly deps: ContentEditorDeps,
  ) {}

  /** The form holds edits that have never been saved. */
  public readonly unsavedChanges = computed(
    () => this.loaded() && fingerprint(this.values()) !== this.saved(),
  );

  /**
   * The fields this form would change, each with what is live and what is not.
   *
   * Compared as renderings ([`fingerprint`]), because the widgets rebuild their values as they are
   * edited: two values that differ only in key order are not a change. The published copy goes
   * through `withDefaults` first, so a field the form filled in with its default is not counted as
   * a difference against a copy stored before that default existed.
   */
  public readonly changedFields = computed(() => {
    const published = this.publishedValues();
    if (published === null) {
      return [];
    }
    const schema = this.schema();
    const live = withDefaults(schema, published);
    const draft = this.values();
    return schema
      .filter((field) => fingerprint(draft[field.name]) !== fingerprint(live[field.name]))
      .map((field) => ({
        field,
        published: live[field.name] ?? null,
        draft: draft[field.name] ?? null,
      }));
  });

  /**
   * Everything the screen shows belongs to one piece of content, so a switch starts from nothing.
   *
   * The screen calls this when its address changes; the loads it starts are the source's.
   */
  load(name: string) {
    const token = ++this.loadToken;
    this.loaded.set(false);
    this.name.set(name);
    this.schema.set([]);
    this.values.set({});
    this.metadata.set(null);
    this.error.set(null);
    this.notice.set(null);
    this.publishedValues.set(null);
    this.comparing.set(false);
    this.previewAllowed.set(null);
    this.previewUrl.set('');
    this.problemField.set(null);
    this.fieldErrors = {};

    // Asked for on every visit rather than cached: an administrator can turn it off while a
    // reviewer holds the screen open, and the refusal would then arrive from the server with
    // nothing on screen to explain it.
    this.source
      .loadPreviewAllowed()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: (allowed) => {
          if (token === this.loadToken) {
            this.previewAllowed.set(allowed);
          }
        },
        // Left unknown, which keeps the button offering what the server can still refuse.
        error: () => undefined,
      });

    this.source
      .loadSchema()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: (schema) => {
          if (token !== this.loadToken) {
            return;
          }
          this.schema.set(schema);
          this.loadValues(schema, token);
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadSchema', e));
          }
        },
      });

    this.loadMetadata(token);
  }

  /** Ask again after a load that failed. */
  reload() {
    this.load(this.name());
  }

  private loadValues(schema: CollectionSchema, token: number) {
    this.source
      .loadValues()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: (values) => {
          if (token !== this.loadToken) {
            return;
          }
          const filled = withDefaults(schema, values);
          this.values.set(filled);
          this.saved.set(fingerprint(filled));
          // Only now can the form be edited and saved: before this, what it holds is not the
          // content.
          this.loaded.set(true);
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadContent', e));
          }
        },
      });
  }

  private loadMetadata(token: number) {
    this.source
      .loadMetadata()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: (metadata) => {
          if (token === this.loadToken) {
            this.metadata.set(metadata);
          }
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadPublishedState', e));
          }
        },
      });
  }

  setValue(field: FieldSchema, value: FieldValue) {
    // Replaced rather than mutated: the template reads the signal.
    this.values.update((values) => ({ ...values, [field.name]: value }));
  }

  setFieldError(field: FieldSchema, message: Message | null) {
    if (message) {
      this.fieldErrors[field.name] = message;
      // A problem the widget itself found (a text outside its lengths, unparseable JSON) marks its
      // input exactly like a refusal from the server does.
      this.problemField.set(field.name);
    } else {
      delete this.fieldErrors[field.name];
      if (this.problemField() === field.name) {
        this.problemField.set(null);
      }
    }
  }

  /**
   * Whether this cell is the one a refusal named.
   *
   * The server names the input as a path (`title`, `tags[2]`, `seo.description`), so a refusal
   * about something inside a composite still marks the composite's cell rather than nothing.
   */
  isProblem(field: FieldSchema): boolean {
    const problem = this.problemField();
    if (problem === null) {
      return false;
    }
    return (
      problem === field.name ||
      problem.startsWith(`${field.name}.`) ||
      problem.startsWith(`${field.name}[`)
    );
  }

  /**
   * Save the working copy, and then do whatever the screen means by "saved".
   *
   * `then` is the screen's own follow-up - the item editor goes back to its list, and the page
   * editor stays where it is - and it takes no arguments: what the screen needs is in its own
   * signals, and what the editor knows is an *address* (`collection/id`), which is not a name
   * anything outside can navigate to.
   */
  save(then?: () => void) {
    this.saveThen(then);
  }

  /** Save and take the content live in one act: the two steps an editor always does in sequence. */
  saveAndPublish(then?: () => void) {
    this.saveThen(() => {
      // Publish what was just saved, which for a new item is the item the create answered rather
      // than the address this screen had a moment ago. Publish, *then* let the screen move: the
      // publish is the act the button was pressed for, and a navigation is free to replace the
      // screen - a route change may destroy it, and a request that has not been sent by then is
      // never sent.
      this.setPublished(true);
      then?.();
    });
  }

  private saveThen(then?: () => void) {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set(null);
    this.notice.set(null);
    this.problemField.set(null);
    const values: EditorValues = { ...this.values() };
    // Captured before the request: everything after this point is about the content the button was
    // pressed for, whatever the screen shows by the time the answer arrives.
    const started = this.start();

    // Everything below describes *this* form - what it holds, what it was, what it is told - so
    // none of it may be written once the screen is on other content, or on the same content opened
    // afresh.
    const done = () => {
      if (!this.stillOn(started)) {
        return;
      }
      this.editsSaved(values);
      then?.();
    };

    const create = this.source.create;
    if (!this.source.exists() && create) {
      // A new item: the create answers the id it was given, and the screen adopts it - which is
      // what makes the next save an update, and what lets the publish below name the item that was
      // just created.
      create
        .call(this.source, values)
        .pipe(takeUntilDestroyed(this.deps.destroyRef))
        .subscribe({
          next: (created) => {
            if (!this.stillOn(started)) {
              return;
            }
            // The screen adopts the id *after* the guard: the address this act was started for
            // names "new", and adopting the id changes it, so checking afterwards would always say
            // the reader had moved on.
            this.source.created?.(created);
            this.editsSaved(values);
            then?.();
          },
          error: (e) => {
            if (this.stillOn(started)) {
              this.refuse(e);
            }
          },
        });
      return;
    }

    this.source
      .update(values)
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: () => done(),
        error: (e) => {
          if (this.stillOn(started)) {
            this.refuse(e);
          }
        },
      });
  }

  /** The form and the server agree again, so leaving no longer needs asking about. */
  private editsSaved(values: EditorValues) {
    this.error.set(null);
    this.notice.set(t('common.saved'));
    this.saved.set(fingerprint(values));
    // Content that has never been saved has no status on screen yet; this is what puts the badge
    // and the publish controls there without a reload.
    this.loadMetadata(this.loadToken);
  }

  private refuse(error: unknown) {
    this.problemField.set(fieldOf(error));
    this.error.set(failure('content.saveFailed', error));
  }

  /**
   * Publish, or release the changes waiting on published content.
   *
   * Publishing is the copy on the server, so publishing something published again is exactly "make
   * the site match the editor" - no need to take it down first.
   */
  publish() {
    this.setPublished(true);
  }

  /** Take the content off the site. Its working copy is kept. */
  unpublish() {
    this.setPublished(false);
  }

  private setPublished(published: boolean, started = this.start()) {
    const request = this.source.setPublished(published);
    request.pipe(takeUntilDestroyed(this.deps.destroyRef)).subscribe({
      next: (metadata) => {
        // The answer belongs to the content that was on screen when the button was pressed.
        if (!this.stillOn(started)) {
          return;
        }
        this.error.set(null);
        this.metadata.set(metadata);
      },
      error: (e) => {
        if (this.stillOn(started)) {
          // Publishing is where required fields are asked about, so a refusal names one: mark the
          // input, exactly as a refused save does.
          this.problemField.set(fieldOf(e));
          this.error.set(failure('content.failedToChangePublished', e));
        }
      },
    });
  }

  /**
   * Show what the site is serving next to what is in the form.
   *
   * The form holds the working copy, so without this the one thing an editor cannot see anywhere in
   * the CMS is the content their changes are replacing.
   */
  compareWithPublished() {
    const started = this.start();
    if (this.comparing()) {
      this.comparing.set(false);
      return;
    }
    this.error.set(null);
    this.source
      .loadPublishedValues()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: (values) => {
          if (!this.stillOn(started)) {
            return;
          }
          this.publishedValues.set(values);
          this.comparing.set(true);
        },
        error: (e) => {
          if (this.stillOn(started)) {
            this.error.set(failure('content.failedToLoadPublished', e));
          }
        },
      });
  }

  /**
   * Throw the saved changes away: the content goes back to what the site is serving.
   *
   * Not publishing and not unpublishing: the site is not touched at all, and what is lost is only
   * the work nobody has seen.
   */
  discardChanges() {
    const started = this.start();
    if (!confirm(this.deps.i18n.translate('content.discardChangesConfirm'))) {
      return;
    }
    this.error.set(null);
    this.notice.set(null);
    this.source
      .discardDraft()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: () => {
          if (!this.stillOn(started)) {
            return;
          }
          // The notice is set *after* the load, which starts by clearing whatever the last act
          // said.
          this.load(this.name());
          this.notice.set(t('content.changesDiscarded'));
        },
        error: (e) => {
          if (this.stillOn(started)) {
            this.error.set(failure('content.failedToDiscardChanges', e));
          }
        },
      });
  }

  /**
   * Mint a link that shows this working copy to someone without an account, and copy it.
   *
   * Saving first is not required: the link always shows what is stored, which is what a reviewer
   * should be looking at anyway.
   */
  sharePreview() {
    // The content this is about, captured now: the screen may be showing other content by the time
    // the link comes back, and a link for what the reader has left is not one to copy under the
    // name of what is on screen.
    const started = this.start();
    this.error.set(null);
    this.notice.set(null);
    // The API's own preview answer is JSON, so without a preview site there is nothing readable to
    // hand a reviewer. Say that rather than copy a link nobody can use - see the capabilities
    // answer, which is where a deployment says whether it has one.
    const site = this.deps.capabilities.previewSiteUrl();
    if (site === null) {
      this.previewUrl.set('');
      this.error.set(t('content.previewSiteNotConfigured'));
      return;
    }
    this.source
      .createPreviewLink()
      .pipe(takeUntilDestroyed(this.deps.destroyRef))
      .subscribe({
        next: (link) => {
          // The clipboard write is asynchronous and nothing waits for it; the method that does it
          // says so by returning a promise this handler deliberately drops.
          void this.copyPreviewLink(link, started, site);
        },
        error: (e) => {
          if (this.stillOn(started)) {
            this.error.set(failure('content.failedToCreatePreviewLink', e));
          }
        },
      });
  }

  private async copyPreviewLink(link: PreviewLink, started: StartedContent, site: string) {
    if (!this.stillOn(started)) {
      return;
    }
    const url = previewSiteUrl(link.path, site);
    this.previewUrl.set(url);
    const copied = await copyToClipboard(url);
    if (!this.stillOn(started)) {
      return;
    }
    const expires = this.deps.dates.format(link.expires_at);
    this.notice.set(
      copied
        ? t('content.previewCopied', { expires })
        : t('content.previewNotCopied', { expires }),
    );
  }

  /**
   * Whether the form holds edits that would be lost by leaving.
   *
   * Asked by `unsavedChangesGuard`, and by the screen to decide what the publish button means.
   */
  hasUnsavedChanges(): boolean {
    return this.unsavedChanges();
  }

  /** Let go of everything in flight: a destroyed screen is nobody's screen. */
  destroy() {
    // The generation moves on, so every guard that compares it answers "no", and answers still on
    // their way are dropped. Without this, a save and publish that landed after the reader had gone
    // elsewhere still released the content.
    this.loadToken += 1;
  }

  /** The content an act is about, and the load it belongs to, captured when the act starts. */
  private start(): StartedContent {
    return { address: this.source.address(), generation: this.loadToken };
  }

  /** Whether the screen is still on the content a slow answer was about, as it was then. */
  private stillOn(started: StartedContent): boolean {
    return this.source.address() === started.address && this.loadToken === started.generation;
  }
}
