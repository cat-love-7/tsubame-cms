import { Component, HostListener, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { fingerprint } from 'app/core/value-changes';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure, fieldOf, t } from 'app/core/i18n/message';
import { ItemMetadata } from 'app/models/item-status';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { CollectionValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { absoluteApiUrl, copyToClipboard } from 'app/shared/share-link';
import { ValueField } from 'app/shared/value-field/value-field';

/**
 * Create/edit one collection item.
 *
 * The form is driven by the collection schema: the shared `ValueField` renders the widget
 * each field type needs and owns its own input state. Values are sent **without type
 * tags**, and fields this editor cannot edit yet (composite fields) keep whatever the
 * server sent so saving never silently discards them.
 *
 * Publishing is separate from saving: an item may be edited any number of times while it
 * stays a draft, and only publishing makes it visible to the public content API.
 */
@Component({
  selector: 'app-item-edit',
  imports: [ItemStatusBadge, MatButtonModule, MessagePipe, RouterLink, TranslocoPipe, ValueField],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class Edit implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private collectionsService = inject(CollectionsService);
  private dates = inject(DateTimeFormat);
  /** A read-only account sees the form but cannot change it. */
  public auth = inject(AuthService);

  /**
   * The collection and the item being edited.
   *
   * Signals, read from the parameter stream: opening another item of the same collection, or
   * another collection, reuses this component, so parameters read once would leave the previous
   * item's values in the form.
   */
  public collectionName = signal('');
  private itemId = signal<number | null>(null);
  /** The id the address names, which stops matching the item once a new one has been created. */
  private routeItemId = signal<number | null>(null);
  public isNew = computed(() => this.itemId() === null);

  /**
   * The schema and the values being edited, as signals.
   *
   * They arrive from two asynchronous loads, and a plain field assigned from a response
   * can land while Angular is checking the view: dev mode then reported
   * `ExpressionChangedAfterItHasBeenCheckedError` for `schema.length === 0` every time an
   * item was opened. A signal tells Angular the view changed instead of tripping over it.
   */
  public schema = signal<CollectionSchema>([]);
  public values = signal<CollectionValue>({});
  /** The failure to show, as a key or as the server's own words. */
  public error = signal<Message | null>(null);
  /** The field the last refusal was about, so the form can mark that one input. */
  public problemField = signal<string | null>(null);
  /** Draft/published state; `null` for an item that has not been saved yet. */
  public metadata = signal<ItemMetadata | null>(null);
  public published = computed(() => this.metadata()?.status === 'published');
  /** A published item with an unpublished working copy: the site is behind the editor. */
  public hasDraft = computed(() => this.metadata()?.has_draft ?? false);
  /** What this account may do *with this collection*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('collections', this.collectionName()));
  public canPublish = computed(() => this.auth.canPublishIn('collections', this.collectionName()));
  /** The shareable preview link, once one has been minted. */
  public previewUrl = signal('');
  /** What happened to the preview link: copied, or made but not copied. */
  public notice = signal<Message | null>(null);
  /** Places each field on the shared 12-column grid, mirroring the schema editor. */
  public cellStyle = fieldCellStyle;

  /**
   * What the form held when it was last in step with the server.
   *
   * Compared by rendering rather than by identity: the widgets rebuild their values as they are
   * edited, so a form nobody touched would still be a different object.
   */
  private saved = signal('');
  /** Whether the form holds edits that have never been saved. */
  public unsavedChanges = computed(() => fingerprint(this.values()) !== this.saved());

  /**
   * Guards against a response that belongs to the item that was open when it was asked for.
   *
   * Opening another item reuses this component: a slow answer for the previous one used to land
   * in the new form, which showed one item's content under another's address - and saving that
   * wrote one item's content over the other.
   */
  private loadToken = 0;

  /**
   * Per-field input problems reported by the value fields (invalid JSON, failed upload).
   * Saving is refused while any remain, so bad input is neither sent nor replaced by a
   * stale value without the user noticing.
   */
  private fieldErrors: { [field: string]: Message } = {};

  constructor() {
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      const id = params.get('id') === null ? null : Number(params.get('id'));
      this.routeItemId.set(id);
      if (name !== this.collectionName() || id !== this.itemId()) {
        this.load(name, id);
      }
    });
  }

  /** Everything the form shows belongs to one item, so a switch starts from nothing. */
  private load(name: string, id: number | null) {
    const token = ++this.loadToken;
    this.collectionName.set(name);
    this.itemId.set(id);
    this.schema.set([]);
    this.values.set({});
    this.metadata.set(null);
    this.error.set(null);
    this.notice.set(null);
    this.previewUrl.set('');
    this.problemField.set(null);
    this.fieldErrors = {};

    this.collectionsService.getCollectionSchema(name).subscribe({
      next: (schema) => {
        if (token !== this.loadToken) {
          return;
        }
        this.schema.set(schema);
        if (id === null) {
          const empty = withDefaults(schema, {});
          this.values.set(empty);
          this.saved.set(fingerprint(empty));
        } else {
          this.loadItem(schema, id, token);
          this.loadMetadata(id, token);
        }
      },
      error: (e) => {
        if (token === this.loadToken) {
          this.error.set(failure('content.failedToLoadSchema', e));
        }
      },
    });
  }

  private loadItem(schema: CollectionSchema, id: number, token: number) {
    this.collectionsService.getCollectionItem(this.collectionName(), id).subscribe({
      next: (values) => {
        if (token !== this.loadToken) {
          return;
        }
        const filled = withDefaults(schema, values);
        this.values.set(filled);
        this.saved.set(fingerprint(filled));
      },
      error: (e) => {
        if (token === this.loadToken) {
          this.error.set(failure('content.failedToLoadItem', e));
        }
      },
    });
  }

  private loadMetadata(id: number, token: number) {
    this.collectionsService.getItemMetadata(this.collectionName(), id).subscribe({
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

  /**
   * Publish, or release the changes waiting on a published item.
   *
   * The server treats publishing as the copy: it replaces the published item with the working
   * copy. Doing it again on a published item is therefore exactly "make the site match the
   * editor" - there is no need to take the item down first.
   */
  publish() {
    this.setPublished(true);
  }

  /**
   * Save the form, then publish what was saved.
   *
   * Publishing copies the *stored* working copy, so pressing it with edits still in the form
   * would put the previous version on the site while the screen showed the new one. The button
   * becomes this whenever the form has unsaved edits (see the template), which is the one order
   * that cannot be wrong.
   */
  saveAndPublish() {
    this.saveThen(() => {
      this.addressTheNewItem();
      this.setPublished(true);
    });
  }

  /**
   * Point the address at an item that was created on the create screen.
   *
   * Reloading `/create` would offer a blank form for an item that already exists (pressing save
   * again would then create a second one). The route id is compared with the item's, so this only
   * fires for the screen that created something.
   */
  private addressTheNewItem() {
    const id = this.itemId();
    if (id === null || this.routeItemId() !== null) {
      return;
    }
    this.router.navigate(['/collections', this.collectionName(), 'edit', id], { replaceUrl: true });
  }

  /** Take the item off the site. Its working copy is kept. */
  unpublish() {
    this.setPublished(false);
  }

  private setPublished(published: boolean) {
    const id = this.itemId();
    if (id === null) {
      return;
    }

    const request = published
      ? this.collectionsService.publishItem(this.collectionName(), id)
      : this.collectionsService.unpublishItem(this.collectionName(), id);

    request.subscribe({
      next: (metadata) => {
        // The answer is about the item that was on screen when the button was pressed; by now
        // the editor may be showing another one.
        if (this.itemId() !== id) {
          return;
        }
        this.error.set(null);
        this.metadata.set(metadata);
      },
      error: (e) => {
        if (this.itemId() === id) {
          this.error.set(failure('content.failedToChangePublished', e));
        }
      },
    });
  }

  setValue(field: FieldSchema, value: FieldValue) {
    // Replaced rather than mutated: the template reads the signal, and changing the object
    // inside it would not tell Angular anything happened.
    this.values.update((values) => ({ ...values, [field.name]: value }));
  }

  setFieldError(field: FieldSchema, message: Message | null) {
    if (message) {
      this.fieldErrors[field.name] = message;
      // A problem the widget itself found (a text outside its lengths, unparseable JSON) marks
      // its input exactly like a refusal from the server does.
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
   * Mint a link that shows this working copy to someone without an account, and copy it.
   *
   * Saving first is not required: the link always shows what is stored, which is what a
   * reviewer should be looking at anyway.
   */
  sharePreview() {
    if (this.itemId() === null) {
      return;
    }
    this.error.set(null);
    this.notice.set(null);
    this.collectionsService.createPreviewLink(this.collectionName(), this.itemId() as number).subscribe({
      next: async (link) => {
        const url = absoluteApiUrl(link.path);
        this.previewUrl.set(url);
        const copied = await copyToClipboard(url);
        const expires = this.dates.format(link.expires_at);
        this.notice.set(
          copied
            ? t('content.previewCopied', { expires })
            : t('content.previewNotCopied', { expires }),
        );
      },
      error: (e) => this.error.set(failure('content.failedToCreatePreviewLink', e)),
    });
  }

  /** Save the working copy, and go back to the list. */
  save() {
    this.saveThen(() => this.goBackToList());
  }

  /**
   * Send the form, and do something with the saved item.
   *
   * The follow-up only runs when the server accepted the save: publishing what a refused save
   * left behind would be worse than doing nothing.
   */
  private saveThen(then: () => void) {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set(null);
    this.notice.set(null);
    this.problemField.set(null);
    const values: CollectionValue = { ...this.values() };

    // Subscribe per branch: the create and update calls return different observable
    // types, which cannot be unioned into a single `subscribe` call.
    const id = this.itemId();
    if (id === null) {
      this.collectionsService.createCollectionItem(this.collectionName(), values).subscribe({
        next: (created) => {
          this.editsSaved(values);
          // A new item is saved as a working copy; publishing it needs its id.
          this.itemId.set(created);
          then();
        },
        error: (e) => this.refuse(e),
      });
    } else {
      this.collectionsService.updateCollectionItem(this.collectionName(), id, values).subscribe({
        next: () => {
          this.editsSaved(values);
          then();
        },
        error: (e) => this.refuse(e),
      });
    }
  }

  /** The form and the server agree again. */
  private editsSaved(values: CollectionValue) {
    this.saved.set(fingerprint(values));
  }

  /**
   * Whether the form holds edits that would be lost by leaving.
   *
   * `CanDeactivate` asks this (see `unsavedChangesGuard`), and the screen asks it to decide
   * whether publishing is one act or two.
   */
  hasUnsavedChanges(): boolean {
    return this.unsavedChanges();
  }

  /**
   * Warn before a reload or a closed tab, which no route guard can see.
   *
   * The browser shows its own wording; all a page can do is ask it to.
   */
  @HostListener('window:beforeunload', ['$event'])
  warnBeforeLeaving(event: BeforeUnloadEvent) {
    if (this.unsavedChanges()) {
      event.preventDefault();
    }
  }

  /** A save the server refused: remember which field it was about, and mark it. */
  private refuse(error: unknown) {
    this.problemField.set(fieldOf(error));
    this.error.set(failure('content.saveFailed', error));
  }

  private goBackToList() {
    this.router.navigate(['/collections', this.collectionName()]);
  }
}
