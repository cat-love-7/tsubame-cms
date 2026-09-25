import { Component, EventEmitter, Input, Output, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButtonModule } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatTooltipModule } from '@angular/material/tooltip';
import { TranslocoPipe, TranslocoService } from '@jsverse/transloco';

import { Message } from 'app/core/i18n/message';
import { FieldSchema, isMarkdownFieldSchema } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { ImageEntry } from 'app/repositories/media/images.repository';
import { ImagesService } from 'app/services/media/images.service';
import { LibraryPicker } from 'app/shared/library-picker/library-picker';
import { absoluteApiUrl } from 'app/shared/share-link';

/** One of the buttons that writes syntax, and what it wraps or prefixes. */
interface MarkdownAction {
  /** The catalog key of the button's name, which is also what a screen reader reads. */
  label: string;
  icon: string;
  /** Wrapped around the selection (bold, italic, link, image). */
  wrap?: [string, string];
  /** The key of the text a wrap starts with when nothing is selected. */
  placeholder?: string;
  /** Put at the start of every line the selection touches. */
  prefix?: string;
}

/**
 * A Markdown box, with buttons for the syntax.
 *
 * Markdown is what a site renders, and its syntax is what an editor has to remember - which is why
 * the box carries a toolbar: an editor who does not write Markdown should not have to look up which
 * bracket is which. Each button acts on the selection, and the parser is the site's, not ours.
 *
 * The image button uses the same library picker the image fields do, and inserts the **durable**
 * link (`/api/images/by-id/{id}`, as an absolute address because the site that renders the Markdown
 * may be another origin) - which is what survives the image being replaced.
 */
@Component({
  selector: 'app-markdown-field',
  imports: [
    FormsModule,
    LibraryPicker,
    MatButtonModule,
    MatFormFieldModule,
    MatIconModule,
    MatInputModule,
    MatTooltipModule,
    TranslocoPipe,
  ],
  templateUrl: './markdown-field.html',
  styleUrl: './markdown-field.scss',
})
export class MarkdownField {
  private images = inject(ImagesService);
  /** The wording of the placeholders and prompts the buttons put up. */
  private i18n = inject(TranslocoService);

  @Input({ required: true }) field!: FieldSchema;
  @Input() value: FieldValue = null;
  /** Renders the box read-only, for the schema editor's preview. */
  @Input() disabled = false;
  @Input() labelId = '';
  @Output() valueChange = new EventEmitter<FieldValue>();
  @Output() errorChange = new EventEmitter<Message | null>();

  /** Whether the library picker is open, for the image button. */
  public pickerOpen = signal(false);
  /**
   * Where a chosen image goes: the box and the range that was selected when the button was pressed.
   *
   * The picker is an overlay over the form, so the range cannot change while it is open, and an
   * image is inserted where the caret was rather than replacing the field's value.
   */
  private imageTarget: { input: HTMLTextAreaElement; start: number; end: number } | null = null;

  /**
   * What each button does.
   *
   * A table rather than eight blocks of template: the buttons differ only in what they wrap or
   * prefix, and a ninth is one line here.
   */
  public readonly actions: MarkdownAction[] = [
    {
      label: 'content.mdBold',
      icon: 'format_bold',
      wrap: ['**', '**'],
      placeholder: 'content.mdBoldPlaceholder',
    },
    {
      label: 'content.mdItalic',
      icon: 'format_italic',
      wrap: ['*', '*'],
      placeholder: 'content.mdItalicPlaceholder',
    },
    { label: 'content.mdHeading', icon: 'title', prefix: '## ' },
    { label: 'content.mdLink', icon: 'link' },
    { label: 'content.mdImage', icon: 'image' },
    { label: 'content.mdBulletList', icon: 'format_list_bulleted', prefix: '- ' },
    { label: 'content.mdNumberedList', icon: 'format_list_numbered', prefix: '1. ' },
    { label: 'content.mdQuote', icon: 'format_quote', prefix: '> ' },
  ];

  /** The text in the box, as the buttons see it. */
  private text(): string {
    return typeof this.value === 'string' ? this.value : '';
  }

  /** The lengths the schema set, which the box reports as hints. */
  public maxLength(): number | null {
    const type = this.field.field_type;
    return isMarkdownFieldSchema(type) ? (type.Markdown.max_length ?? null) : null;
  }

  public minLength(): number | null {
    const type = this.field.field_type;
    return isMarkdownFieldSchema(type) ? (type.Markdown.min_length ?? null) : null;
  }

  /**
   * How many lines the box shows, from the height the schema asked for.
   *
   * `height` is a layout minimum in row units of 72px (see `field-layout`), and a line of text is
   * about 24px in this theme - so one unit is three lines. The floor is what a Markdown box always
   * was: never a one-line box.
   */
  public rows(): number {
    return Math.max(6, Math.max(1, this.field.height) * 3);
  }

  public textLength(): number {
    return [...this.text()].length;
  }

  /** The hint about the lengths, when the schema set any. */
  public lengthHintKey(): string | null {
    const min = this.minLength();
    const max = this.maxLength();
    if (min !== null && max !== null) {
      return 'content.lengthBetween';
    }
    if (max !== null) {
      return 'content.lengthAtMost';
    }
    return min !== null ? 'content.lengthAtLeast' : null;
  }

  /** Press a toolbar button: what it does depends on what it is. */
  press(action: MarkdownAction, input: HTMLTextAreaElement) {
    if (action.prefix !== undefined) {
      this.prefixLines(input, action.prefix);
      return;
    }
    if (action.icon === 'link') {
      this.insertLink(input);
      return;
    }
    if (action.icon === 'image') {
      this.pickImage(input);
      return;
    }
    const [before, after] = action.wrap ?? ['', ''];
    this.wrap(input, before, after, action.placeholder ?? '');
  }

  /** The box changed: the value travels to the field that owns it. */
  update(value: FieldValue) {
    this.valueChange.emit(value);
  }

  /**
   * Put `before` and `after` around the selection, or around a placeholder when nothing is
   * selected.
   *
   * What was inserted (or the placeholder) is left selected, so pressing a button and typing
   * replaces the placeholder rather than landing after it.
   */
  private wrap(input: HTMLTextAreaElement, before: string, after: string, placeholderKey: string) {
    const value = this.text();
    const start = input.selectionStart;
    const end = input.selectionEnd;
    const chosen = value.slice(start, end) || this.i18n.translate(placeholderKey);
    this.replace(input, { start, end }, `${before}${chosen}${after}`, {
      start: start + before.length,
      end: start + before.length + chosen.length,
    });
  }

  /**
   * Put `prefix` at the start of every line the selection touches.
   *
   * Whole lines, so highlighting the middle of a paragraph and pressing "list" makes a list of that
   * paragraph rather than of its middle. A line that already has the prefix is left alone, which is
   * what makes the button safe to press twice.
   */
  private prefixLines(input: HTMLTextAreaElement, prefix: string) {
    const value = this.text();
    const start = input.selectionStart;
    const end = input.selectionEnd;
    const from = value.lastIndexOf('\n', Math.max(0, start - 1)) + 1;
    const to = value.indexOf('\n', end);
    const lineEnd = to === -1 ? value.length : to;
    const lines = value.slice(from, lineEnd).split('\n');
    const prefixed = lines
      .map((line) => (line.startsWith(prefix) ? line : prefix + line))
      .join('\n');
    this.replace(input, { start: from, end: lineEnd }, prefixed, {
      start: from,
      end: from + prefixed.length,
    });
  }

  /** The link button: ask for the address, then wrap the selection (or a placeholder) with it. */
  private insertLink(input: HTMLTextAreaElement) {
    const url = window.prompt(this.i18n.translate('content.mdLinkPrompt'), 'https://');
    const address = url?.trim() ?? '';
    if (address === '') {
      return;
    }
    this.wrap(input, '[', `](${address})`, 'content.mdLinkPlaceholder');
  }

  /** The image button: pick from the library, and put the durable link where the caret is. */
  private pickImage(input: HTMLTextAreaElement) {
    this.imageTarget = { input, start: input.selectionStart, end: input.selectionEnd };
    this.pickerOpen.set(true);
  }

  /** The image the picker chose: the durable link, where the caret was. */
  insertImage(image: ImageEntry) {
    const target = this.imageTarget;
    this.imageTarget = null;
    this.pickerOpen.set(false);
    if (target === null) {
      return;
    }
    // The durable link, as an absolute address: Markdown is rendered by the site, which may be
    // somewhere else entirely, and the id is what survives a replacement (see `docs/content-api.md`).
    // It is the same link the library's copy button hands out.
    const url = absoluteApiUrl(this.images.imageLink(image.id));
    this.replace(
      target.input,
      { start: target.start, end: target.end },
      `![${image.original_filename}](${url})`,
    );
    this.errorChange.emit(null);
  }

  /**
   * Replace `range` with `text`, and put the selection where the button meant it to be.
   *
   * The value travels to the parent and comes back through the binding, which writes the whole box
   * and leaves the caret at the end; the range is set after that, once the framework has written
   * it. Without a `selected` range the caret goes after what was inserted.
   */
  private replace(
    input: HTMLTextAreaElement,
    range: { start: number; end: number },
    text: string,
    selected?: { start: number; end: number },
  ) {
    const value = this.text();
    this.update(`${value.slice(0, range.start)}${text}${value.slice(range.end)}`);
    const from = selected?.start ?? range.start + text.length;
    const to = selected?.end ?? from;
    setTimeout(() => {
      input.focus();
      input.setSelectionRange(from, to);
    });
  }
}
