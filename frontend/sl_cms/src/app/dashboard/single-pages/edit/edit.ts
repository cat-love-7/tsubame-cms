import { Component, DestroyRef, HostListener, computed, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, RouterLink } from '@angular/router';
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
import { ContentValue } from 'app/models/values/single-page';

import { FieldValue, withDefaults } from 'app/models/values/fields';
import { SinglePagesService } from 'app/services/schema/single-pages.service';
import { ItemStatusBadge } from 'app/shared/item-status/item-status';
import { copyToClipboard, previewSiteUrl } from 'app/shared/share-link';
import { RelationReferences } from 'app/shared/relation-references/relation-references';
import { ValueField } from 'app/shared/value-field/value-field';

/** The page a request was started for (see the collection item editor). */
interface StartedPage {
  name: string;
  generation: number;
}

/**
 * Edit the content of one single page.
 *
 * A single page has exactly one item, so there is no list and no create/delete here —
 * only the form. It shares `ValueField` and the layout grid with the collection editor,
 * and publishing is a separate act from saving just as it is for a collection item.
 */
@Component({
  selector: 'app-single-page-edit',
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
export class SinglePageEdit implements HasUnsavedChanges {
  private route = inject(ActivatedRoute);
  /** When this screen goes away, so does everything it still has in flight (see the constructor). */
  private destroyRef = inject(DestroyRef);
  private pages = inject(SinglePagesService);
  private capabilities = inject(CapabilitiesService);
  private dates = inject(DateTimeFormat);
  /** A read-only account sees the form but cannot change it. */
  public auth = inject(AuthService);

  /**
   * The page being edited.
   *
   * A signal, and read from the parameter stream rather than once: the sidebar switches pages
   * without leaving this route, and the router reuses the component, so a parameter read at
   * construction would leave the previous page on screen.
   */
  public pageName = signal('');
  /** Signals, for the reason given in the collection item editor: both arrive from
   * asynchronous loads that would otherwise trip the dev-mode change check. */
  public schema = signal<CollectionSchema>([]);
  public values = signal<ContentValue>({});
  /** The failure to show, as a key or as the server's own words. */
  public error = signal<Message | null>(null);
  /** The field the last refusal was about, so the form can mark that one input. */
  public problemField = signal<string | null>(null);
  public metadata = signal<ItemMetadata | null>(null);
  public published = computed(() => this.metadata()?.status === 'published');
  /** A published page with an unpublished working copy: the site is behind the editor. */
  public hasDraft = computed(() => this.metadata()?.has_draft ?? false);

  /** What the form held when it was last in step with the server (see the collection editor). */
  private saved = signal('');
  /**
   * Whether the page's content has arrived (see the collection item editor): saving before it has
   * would write the empty form over what nobody touched, so the form waits for it.
   */
  public loaded = signal(false);
  /** Whether the form holds edits that have never been saved. */
  public unsavedChanges = computed(
    () => this.loaded() && fingerprint(this.values()) !== this.saved(),
  );
  /**
   * Guards against a response for a page that is no longer the one on screen: the sidebar
   * switches pages without leaving this route, so a slow answer used to arrive after the switch.
   */
  private loadToken = 0;
  /** What this account may do *with this page*, overrides included. */
  public canEdit = computed(() => this.auth.canEditIn('single_pages', this.pageName()));
  public canPublish = computed(() => this.auth.canPublishIn('single_pages', this.pageName()));
  /** The shareable preview link, once one has been minted. */
  public previewUrl = signal('');
  /** What happened to the preview link: copied, or made but not copied. */
  public notice = signal<Message | null>(null);
  public cellStyle = fieldCellStyle;

  /** Per-field problems reported by the value fields; saving is refused while any remain. */
  private fieldErrors: { [field: string]: Message } = {};

  constructor() {
    // A destroyed screen is nobody's screen: the answers still on their way belong to a load that
    // no longer exists, so the generation moves on and every guard that compares it - `stillOn`,
    // and the token checks in the loads below - answers "no". Without this, a save and publish that
    // landed after the reader had gone elsewhere still released the page.
    this.destroyRef.onDestroy(() => {
      this.loadToken += 1;
    });
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((params) => {
      const name = params.get('name') ?? '';
      if (name !== this.pageName()) {
        this.load(name);
      }
    });
  }

  /** Everything the screen shows belongs to one page, so switching starts from nothing. */
  private load(name: string) {
    const token = ++this.loadToken;
    this.loaded.set(false);
    this.pageName.set(name);
    this.schema.set([]);
    this.values.set({});
    this.metadata.set(null);
    this.error.set(null);
    this.notice.set(null);
    this.previewUrl.set('');
    this.problemField.set(null);
    this.fieldErrors = {};

    this.pages
      .getPageSchema(name)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (schema) => {
          if (token !== this.loadToken) {
            return;
          }
          this.schema.set(schema);
          this.loadItem(schema, token);
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadSchema', e));
          }
        },
      });

    this.loadMetadata(token);
  }

  private loadItem(schema: CollectionSchema, token: number) {
    this.pages
      .getPageItem(this.pageName())
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (values) => {
          if (token !== this.loadToken) {
            return;
          }
          const filled = withDefaults(schema, values);
          this.values.set(filled);
          this.saved.set(fingerprint(filled));
          // Only now can the form be edited and saved: before this, what it holds is not the page.
          this.loaded.set(true);
        },
        error: (e) => {
          if (token === this.loadToken) {
            this.error.set(failure('content.failedToLoadContent', e));
          }
        },
      });
  }

  /**
   * Publish, or release the changes waiting on a published page.
   *
   * Publishing is the copy on the server, so publishing a published page again is exactly
   * "make the site match the editor" - no need to take the page down first.
   */
  publish() {
    this.setPublished(true);
  }

  /** Take the page off the site. Its working copy is kept. */
  unpublish() {
    this.setPublished(false);
  }

  private setPublished(published: boolean, started = this.start()) {
    const name = started.name;
    const request = published ? this.pages.publishPage(name) : this.pages.unpublishPage(name);

    request.pipe(takeUntilDestroyed(this.destroyRef)).subscribe({
      next: (metadata) => {
        // The answer belongs to the page that was on screen when the button was pressed.
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

  setValue(field: FieldSchema, value: FieldValue) {
    // Replaced rather than mutated: the template reads the signal.
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

  /** Mint a link that shows this working copy to someone without an account, and copy it
   * (see the collection item editor for the reasoning). */
  sharePreview() {
    // The page this is about, captured now: the sidebar switches pages without leaving the route,
    // so the screen may be showing a different one by the time the link comes back.
    const started = this.start();
    this.error.set(null);
    this.notice.set(null);
    // The API's own preview answer is JSON, so without a preview site there is nothing readable
    // to hand a reviewer (see the collection item editor).
    const site = this.capabilities.previewSiteUrl();
    if (site === null) {
      this.previewUrl.set('');
      this.error.set(t('content.previewSiteNotConfigured'));
      return;
    }
    this.pages
      .createPreviewLink(started.name)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: (link) => {
          // The clipboard write is asynchronous and nothing waits for it (see the collection item
          // editor).
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
   * Put a minted link on the clipboard, and report how that went (see the collection item
   * editor).
   */
  private async copyPreviewLink(link: PreviewLink, started: StartedPage, site: string) {
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

  /** Ask again after a load that failed (see the collection item editor). */
  retry() {
    this.load(this.pageName());
  }

  /**
   * Save the working copy, and stay here.
   *
   * A single page is one item with a publish control beside it, so leaving the screen after a
   * save only meant finding the page again to publish it. Saving used to navigate to the schema
   * list, which has no publish control at all. The metadata is re-read so the status and the
   * publish controls appear as soon as the page exists on the server.
   */
  save() {
    this.saveThen();
  }

  /** Save and take the page live in one act: the two steps an editor always does in sequence. */
  saveAndPublish() {
    this.saveThen(() => this.setPublished(true));
  }

  private saveThen(then?: (name: string) => void) {
    const problems = Object.values(this.fieldErrors);
    if (problems.length > 0) {
      this.error.set(problems[0]);
      return;
    }

    this.error.set(null);
    this.notice.set(null);
    this.problemField.set(null);
    const values = { ...this.values() };
    // Captured before the request: everything after this point is about the page the button was
    // pressed for, whatever the sidebar shows by the time the answer arrives.
    const start = this.start();
    const name = start.name;
    this.pages
      .updatePageItem(name, values)
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe({
        next: () => {
          // Everything below describes *this* form - what it holds, what it was, what it is told -
          // so none of it may be written once the screen is on another page, or on the same page
          // opened afresh.
          if (!this.stillOn(start)) {
            return;
          }
          this.error.set(null);
          this.notice.set(t('common.saved'));
          // The form and the server agree again, so leaving no longer needs asking about.
          this.saved.set(fingerprint(values));
          // A page that has never been saved has no status on screen yet; this is what puts the
          // badge and the publish controls there without a reload.
          this.loadMetadata(this.loadToken);
          then?.(name);
        },
        error: (e) => {
          if (!this.stillOn(start)) {
            return;
          }
          this.problemField.set(fieldOf(e));
          this.error.set(failure('content.saveFailed', e));
        },
      });
  }

  private loadMetadata(token: number) {
    this.pages
      .getPageMetadata(this.pageName())
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
   * The page an act is about, and the load it belongs to, captured when the act starts.
   *
   * A single page has no id, so the name is the whole address; the load generation is what tells
   * "the same page, still as it was" from "the same page, opened again since" - a save that
   * answers after a reload must not decide that the reloaded form has been saved.
   */
  private start(): StartedPage {
    return { name: this.pageName(), generation: this.loadToken };
  }

  /**
   * Whether the screen is still on the page a slow answer was about, as it was then.
   *
   * A screen that has been destroyed is not that screen: the generation moves on when it goes (see
   * the constructor), so nothing it asked for is ever answered into it.
   */
  private stillOn(start: StartedPage): boolean {
    return this.pageName() === start.name && this.loadToken === start.generation;
  }

  /**
   * Whether the form holds edits that would be lost by leaving.
   *
   * Asked by `unsavedChangesGuard`, and by the screen to decide what the publish button means.
   */
  hasUnsavedChanges(): boolean {
    return this.unsavedChanges();
  }

  /** Warn before a reload or a closed tab, which no route guard can see. */
  @HostListener('window:beforeunload', ['$event'])
  warnBeforeLeaving(event: BeforeUnloadEvent) {
    if (this.unsavedChanges()) {
      event.preventDefault();
    }
  }
}
