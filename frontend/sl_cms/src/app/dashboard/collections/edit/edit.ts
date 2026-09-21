import { Component, DestroyRef, HostListener, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { MatButtonModule } from '@angular/material/button';
import { TranslocoPipe } from '@jsverse/transloco';

import { AuthService } from 'app/core/auth/auth.service';
import { CapabilitiesService } from 'app/core/capabilities/capabilities.service';
import { fieldCellStyle } from 'app/core/field-layout';
import { HasUnsavedChanges } from 'app/core/unsaved-changes.guard';
import { fingerprint } from 'app/core/value-changes';
import { DateTimeFormat } from 'app/core/i18n/date-format';
import { Message, MessagePipe, failure, fieldOf, t } from 'app/core/i18n/message';
import { ItemMetadata } from 'app/models/item-status';
import { PreviewLink } from 'app/models/links';
import { CollectionSchema } from 'app/models/schema/collection';
import { FieldSchema } from 'app/models/schema/fields';
import { CollectionValue } from 'app/models/values/collection';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { CollectionsService } from 'app/services/schema/collections.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { copyToClipboard, previewSiteUrl } from 'app/shared/share-link';
import { RelationReferences } from 'app/shared/relation-references/relation-references';
import { ValueField } from 'app/shared/value-field/value-field';

/**
 * The item a request was started for.
 *
 * The sidebar and the item list reuse this screen, so an answer can arrive while a different item
 * is on it; carrying what the request started from is what lets that answer be dropped.
 */
interface StartedItem {
  name: string;
  id: number | null;
  generation: number;
}

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
  selector: 'app-collection-item-edit',
  imports: [
    ItemStatusBadge,
    MatButtonModule,
    MessagePipe,
    RelationReferences,
    RouterLink,
    TranslocoPipe,
    ValueField,
  ],
  templateUrl: './edit.html',
  styleUrl: './edit.scss',
})
export class CollectionItemEdit implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  /** When this screen goes away, so does everything it still has in flight (see the constructor). */
  private destroyRef = inject(DestroyRef);
  private collectionsService = inject(CollectionsService);
  private capabilities = inject(CapabilitiesService);
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

  /** The item the references panel is about: what this screen is editing, once it exists. */
  public referencedItemId = computed(() => this.itemId());

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
  /**
   * Whether the item's content has arrived.
   *
   * Until it has, the form holds nothing and the server holds the item: saving now (or typing into
   * a form with nothing in it) would write the empty form over fields nobody touched, so the form
   * is not rendered until this is true. It stays false when the load failed, which is what the
   * retry button is for.
   */
  public loaded = signal(false);
  /** Whether the form holds edits that have never been saved. */
  public unsavedChanges = computed(
    () => this.loaded() && fingerprint(this.values()) !== this.saved(),
  );

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
    // A destroyed screen is nobody's screen: the answers still on their way belong to a load that
    // no longer exists, so the generation moves on and every guard that compares it - `stillOn`,
    // and the token checks in the loads below - answers "no". Without this, a save that landed
    // after the reader had gone elsewhere still passed the guard, and took them back to the list
    // the save was pressed from.
    this.destroyRef.onDestroy(() => {
      this.loadToken += 1;
    });
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
    this.loaded.set(false);
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

    this.collectionsService
      .getCollectionSchema(name)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (schema) => {
          if (token !== this.loadToken) {
            return;
          }
          this.schema.set(schema);
          if (id === null) {
            // A new item starts from the schema's defaults, and there is nothing else to wait for.
            const empty = withDefaults(schema, {});
            this.values.set(empty);
            this.saved.set(fingerprint(empty));
            this.loaded.set(true);
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
    this.collectionsService
      .getCollectionItem(this.collectionName(), id)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (values) => {
          if (token !== this.loadToken) {
            return;
          }
          const filled = withDefaults(schema, values);
          this.values.set(filled);
          this.saved.set(fingerprint(filled));
          // Only now is there a form to edit and save: before this, what the screen holds is not
          // the item.
          this.loaded.set(true);
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadItem', e));
          }
        },
      });
  }

  private loadMetadata(id: number, token: number) {
    this.collectionsService
      .getItemMetadata(this.collectionName(), id)
      .pipe(takeUntilDestroyed(this.destroyRef))
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
    this.saveThen((target) => {
      // Publish, *then* move the address: the publish is the act the button was pressed for, and
      // the navigation below is free to replace this screen - a route change may destroy it, and a
      // request that has not been sent by then is never sent.
      this.setPublished(true, target);
      this.addressTheNewItem(target);
    });
  }

  /**
   * Point the address at an item that was created on the create screen.
   *
   * Reloading `/create` would offer a blank form for an item that already exists (pressing save
   * again would then create a second one). The route id is compared with the item's, so this only
   * fires for the screen that created something.
   */
  private addressTheNewItem(target: { name: string; id: number }) {
    if (this.routeItemId() !== null) {
      return;
    }
    void this.router.navigate(['/collections', target.name, 'edit', target.id], {
      replaceUrl: true,
    });
  }

  /** Take the item off the site. Its working copy is kept. */
  unpublish() {
    this.setPublished(false);
  }

  private setPublished(published: boolean, target = this.target()) {
    if (target === null) {
      return;
    }

    const request = published
      ? this.collectionsService.publishItem(target.name, target.id)
      : this.collectionsService.unpublishItem(target.name, target.id);

    request.pipe(takeUntilDestroyed(this.destroyRef)).subscribe({
      next: (metadata) => {
        // The answer is about the item that was on screen when the button was pressed; by now
        // the editor may be showing another one.
        if (!this.stillOn(target)) {
          return;
        }
        this.error.set(null);
        this.metadata.set(metadata);
      },
      error: (e) => {
        if (this.stillOn(target)) {
          // Publishing is where required fields are asked about, so a refusal names one: mark the
          // input, exactly as a refused save does.
          this.problemField.set(fieldOf(e));
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
    // The item this is about, captured now: the screen may be showing a different one by the time
    // the link comes back (the sidebar and the item list reuse this component), and a link for the
    // item the reader has left is not one to copy under the name of the one on screen.
    const started = this.start();
    const id = started.id;
    if (id === null) {
      return;
    }
    this.error.set(null);
    this.notice.set(null);
    // The API's own preview answer is JSON, so without a preview site there is nothing readable
    // to hand a reviewer. Say that rather than copy a link nobody can use - see the capabilities
    // answer, which is where a deployment says whether it has one.
    const site = this.capabilities.previewSiteUrl();
    if (site === null) {
      this.previewUrl.set('');
      this.error.set(t('content.previewSiteNotConfigured'));
      return;
    }
    this.collectionsService
      .createPreviewLink(started.name, id)
      .pipe(takeUntilDestroyed(this.destroyRef))
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

  /**
   * Put a minted link on the clipboard, and report how that went.
   *
   * Copying is asynchronous, so the screen can move on while the clipboard is written: what is
   * said about the link has to be about the item it is for.
   */
  private async copyPreviewLink(link: PreviewLink, started: StartedItem, site: string) {
    if (!this.stillOn(started)) {
      return;
    }
    const url = previewSiteUrl(link.path, site);
    this.previewUrl.set(url);
    const copied = await copyToClipboard(url);
    if (!this.stillOn(started)) {
      return;
    }
    const expires = this.dates.format(link.expires_at);
    this.notice.set(
      copied ? t('content.previewCopied', { expires }) : t('content.previewNotCopied', { expires }),
    );
  }

  /** Ask again after a load that failed: the form stays off the screen until something arrives,
   *  and this is the only thing on it that can make that happen. */
  retry() {
    this.load(this.collectionName(), this.routeItemId());
  }

  /** Save the working copy, and go back to the list. */
  save() {
    this.saveThen((target) => this.goBackToList(target.name));
  }

  /**
   * Send the form, and do something with the saved item.
   *
   * The follow-up only runs when the server accepted the save: publishing what a refused save
   * left behind would be worse than doing nothing.
   */
  private saveThen(then: (target: { name: string; id: number; generation: number }) => void) {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set(null);
    this.notice.set(null);
    this.problemField.set(null);
    const values: CollectionValue = { ...this.values() };
    // Captured now, before the request: everything after this point is about the item the button
    // was pressed for, whatever the screen shows by the time the answer arrives.
    const started = this.start();
    const name = started.name;
    const id = started.id;

    // Subscribe per branch: the create and update calls return different observable
    // types, which cannot be unioned into a single `subscribe` call.
    if (id === null) {
      this.collectionsService
        .createCollectionItem(name, values)
        .pipe(takeUntilDestroyed(this.destroyRef))
        .subscribe({
          next: (created) => {
            // The form, its saved baseline and the address all describe the create screen this was
            // pressed on: writing any of them once the reader has gone elsewhere would attribute a
            // save to whatever they are looking at now.
            if (!this.stillOn(started)) {
              return;
            }
            this.editsSaved(values);
            // A new item is saved as a working copy; publishing it needs its id.
            this.itemId.set(created);
            then({ name, id: created, generation: started.generation });
          },
          error: (e) => {
            if (this.stillOn(started)) {
              this.refuse(e);
            }
          },
        });
    } else {
      this.collectionsService
        .updateCollectionItem(name, id, values)
        .pipe(takeUntilDestroyed(this.destroyRef))
        .subscribe({
          next: () => {
            if (!this.stillOn(started)) {
              return;
            }
            this.editsSaved(values);
            then({ name, id, generation: started.generation });
          },
          error: (e) => {
            if (this.stillOn(started)) {
              this.refuse(e);
            }
          },
        });
    }
  }

  /** The form and the server agree again. */
  private editsSaved(values: CollectionValue) {
    this.saved.set(fingerprint(values));
  }

  /**
   * The item an act is about, captured when the act starts.
   *
   * A save or publish answers later than it is asked, and by then the screen may be showing
   * another item - or another collection, which is worse: the id would name a different item
   * there. Everything that talks to the server after an await reads this, not the signals.
   */
  private target(): { name: string; id: number; generation: number } | null {
    const id = this.itemId();
    return id === null ? null : { name: this.collectionName(), id, generation: this.loadToken };
  }

  /**
   * The item an act is about, and the load it belongs to, captured when the act starts.
   *
   * The id says *which* item; the generation says which visit to it. A save that answers after the
   * same item was opened again - or after the reader left and came back - must not decide that the
   * form on screen has been saved, and must not report a refusal about it.
   */
  private start(): StartedItem {
    return { name: this.collectionName(), id: this.itemId(), generation: this.loadToken };
  }

  /**
   * Whether the screen is still on the item a slow answer was about, as it was then.
   *
   * A screen that has been destroyed is not that screen: the generation moves on when it goes (see
   * the constructor), so nothing it asked for is ever answered into it.
   */
  private stillOn(target: StartedItem): boolean {
    return (
      this.collectionName() === target.name &&
      this.itemId() === target.id &&
      this.loadToken === target.generation
    );
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

  private goBackToList(name: string) {
    void this.router.navigate(['/collections', name]);
  }
}
