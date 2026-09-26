import { provideHttpClient } from '@angular/common/http';
import { By } from '@angular/platform-browser';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { MatDialog } from '@angular/material/dialog';
import { of } from 'rxjs';

import { FieldSchema } from 'app/models/schema/fields';
import { FieldValue } from 'app/models/values/fields';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
import { Message, t } from 'app/core/i18n/message';
import { ImagesService } from 'app/services/media/images.service';
import { TypedFixture } from 'app/core/testing/fixture';
import { COMPOSITE_DEFINITIONS, StubImagesService, field } from 'app/core/testing/fields';
import { CompositeField } from 'app/shared/composite-field/composite-field';
import { ImageField } from 'app/shared/image-field/image-field';
import { RelationField } from 'app/shared/relation-field/relation-field';
import { ValueField } from './value-field';

describe('ValueField', () => {
  let fixture: TypedFixture<ValueField>;
  let images: StubImagesService;

  beforeEach(async () => {
    images = new StubImagesService();
    await TestBed.configureTestingModule({
      imports: [ValueField],
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        // A stub rather than the HTTP-backed service, so the composite definitions are
        // available synchronously.
        {
          provide: CompositeFieldsService,
          useValue: { getAllCompositeFields: () => of(COMPOSITE_DEFINITIONS) },
        },
        { provide: ImagesService, useValue: images },
      ],
    }).compileComponents();
  });

  /** Uses `setInput` so the inputs are in place before the first change detection. */
  // A dialog is attached to the document rather than to the fixture, so one left open is a `.thumb`
  // the next spec finds.
  afterEach(() => {
    TestBed.inject(MatDialog).closeAll();
  });

  function create(fieldSchema: FieldSchema, value: FieldValue = null, compact = false): ValueField {
    fixture = TestBed.createComponent(ValueField);
    fixture.componentRef.setInput('field', fieldSchema);
    fixture.componentRef.setInput('value', value);
    fixture.componentRef.setInput('compact', compact);
    fixture.detectChanges();
    return fixture.componentInstance;
  }

  function query(selector: string): Element | null {
    return fixture.nativeElement.querySelector(selector);
  }

  // The label is a `span` above the widget, not a Material `mat-label` inside it, so a screen
  // reader has nothing to go on without this: every box read as "edit text", and the boolean read
  // as "Yes" - the label's own text was never part of the control.
  it('names each control with the field label', () => {
    // The element that carries the name is the one a screen reader lands on: the native input
    // itself, and the checkbox's inner `<input>` (Material puts the name there, not on the host).
    const labelled: [FieldSchema, string][] = [
      [field('title', { Text: {} }), 'input'],
      [field('body', { Markdown: {} }), 'textarea'],
      [field('published', 'Boolean'), 'mat-checkbox input[type="checkbox"]'],
    ];
    for (const [schema, control] of labelled) {
      create(schema);
      const label = query('.field-label') as HTMLElement;
      expect(label.id, `${schema.name}: the label has an id`).toBeTruthy();
      expect(label.textContent).toContain(schema.name);
      const element = query(control) as HTMLElement;
      expect(element, `${schema.name}: ${control}`).toBeTruthy();
      expect(element.getAttribute('aria-labelledby'), `${schema.name}: ${control}`).toBe(label.id);
    }

    // A select keeps its own aria-labelledby (it points at the value it shows), so the name goes
    // in through the input the component publishes for it.
    create(field('kind', { TextEnum: ['a', 'b'] }));
    expect((query('mat-select') as HTMLElement).getAttribute('aria-label')).toBe('kind');
  });

  // Two widgets for the same field name (a composite that repeats one) must not share an id: the
  // browser and the screen reader both resolve an id to the first element that has it.
  it('gives each widget its own label id', () => {
    create(field('title', { Text: {} }));
    const first = (query('.field-label') as HTMLElement).id;
    create(field('title', { Text: {} }));
    const second = (query('.field-label') as HTMLElement).id;
    expect(first).not.toBe(second);
  });

  it('renders the control the field type calls for', () => {
    create(field('text', { Text: {} }));
    expect(query('input[matinput]')).toBeTruthy();

    create(field('body', { Markdown: {} }));
    expect(query('textarea')).toBeTruthy();

    create(field('count', 'Number'));
    expect(query('input[type="number"]')).toBeTruthy();

    create(field('live', 'Boolean'));
    expect(query('mat-checkbox')).toBeTruthy();

    create(field('published', 'Date'));
    expect(query('input[type="date"]')).toBeTruthy();

    create(field('at', 'DateTime'));
    expect(query('input[type="datetime-local"]')).toBeTruthy();

    create(field('tags', { TextEnum: ['a', 'b'] }));
    expect(query('mat-select')).toBeTruthy();

    create(field('numbers', { Array: ['Number'] }));
    expect(query('textarea')).toBeTruthy();
    expect(query('mat-select')).toBeFalsy();
  });

  it('explains that an undefined composite field cannot be edited rather than dropping it', () => {
    create(field('missing', { CompositeField: { id: 'nope' } }));
    expect(fixture.nativeElement.textContent).toContain('is not defined');
  });

  it('renders a composite field as its sub-fields, nested through this same component', () => {
    create(field('seo', { CompositeField: { id: 'seo' } }));

    // The sub-field is rendered by another ValueField instance: the recursion works.
    expect(fixture.nativeElement.querySelector('app-value-field')).toBeTruthy();
    expect(fixture.nativeElement.textContent).toContain('description');
  });

  it('lays a composite’s sub-fields out with the widths and heights of the definition', () => {
    create(field('page', { CompositeField: { id: 'layout' } }), {
      id: 'layout',
      values: { headline: 'Hello', aside: 'Sidebar' },
    });
    fixture.detectChanges();

    const cells = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('fieldset.composite .field-cell'),
    );
    expect(cells.length).toBe(2);
    expect(cells[0].style.gridColumn).toBe('span 8');
    expect(cells[0].style.minHeight).toBe('calc(var(--field-row-unit, 72px) * 2)');
    expect(cells[1].style.gridColumn).toBe('span 4');
  });

  it('emits the bare object for a composite value, unwrapping the read wrapper', () => {
    // Reads wrap a composite as `{id, values}`; the wrapper must not leak into sub-fields
    // or back out onto the wire.
    const component = create(field('seo', { CompositeField: { id: 'seo' } }), {
      id: 'seo',
      values: { description: 'meta' },
    });
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));

    // The sub-fields are the composite widget's, which is what holds them; what this component
    // does is pass the fixed value on (the card above is the spec for that).
    const composite = fixture.debugElement.query(By.directive(CompositeField))
      ?.componentInstance as CompositeField;
    composite.setCompositeValue(field('description', { Text: {} }), 'changed');

    expect(emitted).toEqual([{ description: 'changed' }]);
  });

  // The height a schema asks for is a layout minimum (72px units), and a line of text is about
  // 24px: a box that ignored it stayed six rows tall however the schema was drawn.
  it('sizes a text box from the height the schema asked for', () => {
    create(field('body', { Markdown: {} }, { height: 1 }));
    expect((query('textarea') as HTMLTextAreaElement).getAttribute('rows')).toBe('6');

    create(field('body', { Markdown: {} }, { height: 5 }));
    expect((query('textarea') as HTMLTextAreaElement).getAttribute('rows')).toBe('15');

    // A multi-line text field: three lines at the smallest, and the height on top of that.
    create(field('lede', { Text: { multiline: true } }, { height: 1 }));
    expect((query('textarea') as HTMLTextAreaElement).getAttribute('rows')).toBe('3');

    create(field('lede', { Text: { multiline: true } }, { height: 4 }));
    expect((query('textarea') as HTMLTextAreaElement).getAttribute('rows')).toBe('12');
  });

  // A title is one line and a paragraph is a box; the type cannot tell which it is, so the schema
  // says, and a field without the option stays the one-line input it always was.
  it('draws a multi-line text field as a box, and a plain one as a line', () => {
    create(field('title', { Text: {} }));
    expect(query('input[matinput]')).toBeTruthy();
    expect(query('textarea')).toBeNull();

    create(field('lede', { Text: { multiline: true } }));
    expect(query('textarea')).toBeTruthy();
    expect(query('input[matinput]')).toBeNull();
  });

  // The buttons write the syntax an editor who does not know Markdown would otherwise have to
  // remember. Each acts on what is selected in the box.
  describe('the Markdown toolbar', () => {
    /**
     * A Markdown box with a value in it, and the textarea it drew.
     *
     * The text and the selection are put into the element the way the browser has them: `NgModel`
     * does not write the model into the DOM in this environment, and a range only exists on an
     * element that has text.
     */
    function markdown(value: string, selection?: [number, number]) {
      const component = create(field('body', { Markdown: {} }), value);
      const textarea = query('textarea') as HTMLTextAreaElement;
      const emitted: FieldValue[] = [];
      component.valueChange.subscribe((next) => emitted.push(next));
      textarea.value = value;
      if (selection) {
        textarea.setSelectionRange(selection[0], selection[1]);
      }
      return { component, textarea, emitted, pressed: (label: string) => press(label) };
    }

    /** Press the toolbar button with this accessible name. */
    function press(label: string): void {
      const button = Array.from(
        fixture.nativeElement.querySelectorAll<HTMLButtonElement>('.markdown-toolbar button'),
      ).find((candidate) => candidate.getAttribute('aria-label') === label);
      expect(button, `no ${label} button`).toBeTruthy();
      button?.click();
    }

    it('offers the buttons for Markdown only', () => {
      create(field('body', { Markdown: {} }));
      expect(fixture.nativeElement.querySelectorAll('.markdown-toolbar button').length).toBe(8);

      // A single-line text field has no Markdown to write.
      create(field('title', { Text: {} }));
      expect(fixture.nativeElement.querySelector('.markdown-toolbar')).toBeNull();

      // Read-only (the schema editor's preview) offers nothing to press.
      fixture.componentRef.setInput('disabled', true);
      fixture.componentRef.setInput('field', field('body', { Markdown: {} }));
      fixture.detectChanges();
      expect(fixture.nativeElement.querySelector('.markdown-toolbar')).toBeNull();
    });

    it('wraps what is selected', () => {
      const { emitted, pressed } = markdown('a bold word', [2, 6]);

      pressed('Bold');

      expect(emitted).toEqual(['a **bold** word']);
    });

    it('leaves a placeholder selected when nothing is', () => {
      const { emitted, pressed } = markdown('say ');

      pressed('Italic');

      // The placeholder is there to be typed over, which is what the selection is for.
      expect(emitted).toEqual(['say *italic text*']);
    });

    it('puts a link around the selection, asking for the address', () => {
      const prompt = vi.spyOn(window, 'prompt').mockReturnValue('https://example.test/');
      const { emitted, pressed } = markdown('the docs', [0, 8]);

      pressed('Link');

      expect(prompt).toHaveBeenCalled();
      expect(emitted).toEqual(['[the docs](https://example.test/)']);
      prompt.mockRestore();
    });

    it('leaves the text alone when the link prompt is cancelled', () => {
      const prompt = vi.spyOn(window, 'prompt').mockReturnValue(null);
      const { emitted, pressed } = markdown('the docs', [0, 8]);

      pressed('Link');

      expect(emitted).toEqual([]);
      prompt.mockRestore();
    });

    it('prefixes every line the selection touches', () => {
      const { emitted, pressed } = markdown('one\ntwo\nthree', [0, 7]);

      pressed('Bulleted list');

      expect(emitted).toEqual(['- one\n- two\nthree']);
    });

    // Pressing it twice should not make "- - one": an editor fixing a list should not have to
    // undo what the button just did.
    it('does not prefix a line that already has it', () => {
      const { emitted, pressed } = markdown('- one');

      pressed('Bulleted list');

      expect(emitted).toEqual(['- one']);
    });

    // The image comes from the same library the image fields use, and goes in as the durable link
    // the library's own copy button hands out.
    it('inserts a library image at the caret', async () => {
      const { emitted, pressed } = markdown('before after', [7, 7]);

      pressed('Image');
      // The picker is a dialog now: its wall is attached to the document, not to the field.
      await fixture.whenStable();
      const thumbnail = document.querySelector('.thumb') as HTMLButtonElement;
      expect(thumbnail, 'the library picker').toBeTruthy();
      thumbnail.click();
      await fixture.whenStable();

      expect(emitted.length).toBe(1);
      // A Markdown value is a string on the wire; the cast is what says so.
      const inserted = emitted[0] as string;
      // The durable link, as the address a browser can reach it at: the same one the library's
      // copy button hands out.
      expect(inserted).toContain('![logo.png](');
      expect(inserted).toMatch(/\/api\/images\/by-id\/3\)/);
      // Where the caret was, not at the end.
      expect(inserted.startsWith('before ![logo.png](')).toBe(true);
      expect(inserted.endsWith(')after')).toBe(true);
    });
  });

  it('emits a new value when the input changes', () => {
    const component = create(field('text', { Text: {} }), 'old');
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));

    component.update('new');

    expect(emitted).toEqual(['new']);
    expect(component.value).toBe('new');
  });

  // The schema's lengths used to be the schema editor's business only: the content editor let a
  // value past them and the server refused the save after the fact.
  it('states the lengths the schema set, and counts what has been typed', async () => {
    create(field('title', { Text: { max_length: 5, min_length: 2 } }), 'abc');
    await fixture.whenStable();
    fixture.detectChanges();

    const input = query('input[matinput]') as HTMLInputElement;
    expect(input.getAttribute('maxlength')).toBe('5');
    expect(input.getAttribute('minlength')).toBe('2');
    expect(fixture.nativeElement.textContent).toContain('2–5 characters');
    expect(fixture.nativeElement.textContent).toContain('3 / 5');
  });

  it('reports a value outside the lengths before the save, and clears it again', () => {
    const component = create(field('title', { Text: { max_length: 5, min_length: 2 } }), 'abc');
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));

    component.update('abcdef');
    expect(problems[0]).toEqual(t('content.valueTooLong', { field: 'title', max: 5 }));

    component.update('a');
    expect(problems[1]).toEqual(t('content.valueTooShort', { field: 'title', min: 2 }));

    component.update('ab');
    expect(problems[2]).toBeNull();
  });

  // Characters, not bytes: five Japanese characters are five, which is what the server counts
  // too, and what a schema that says 5 has to mean.
  it('counts characters rather than bytes', () => {
    const component = create(field('title', { Text: { max_length: 5 } }), '');
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));

    component.update('あいうえお');
    expect(problems).toEqual([null]);

    component.update('あいうえおか');
    expect(problems[1]).toEqual(t('content.valueTooLong', { field: 'title', max: 5 }));
  });

  // A slug is a type of its own: the rule belongs to it, and the editor can fill it from another
  // field. What is stored is the server's normalisation of it.
  it('edits a slug as a text input, and says what a slug is', async () => {
    create(field('address', { Slug: {} }), 'hello-world');
    await fixture.whenStable();
    fixture.detectChanges();

    const input = query('input[matinput]') as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.value).toBe('hello-world');
    expect(fixture.nativeElement.textContent).toContain('Lower-case letters, digits and hyphens');
    // Unique by being a slug, so the label says so without the schema asking.
    expect(fixture.nativeElement.querySelector('.unique-mark')).toBeTruthy();
  });

  it('rewrites what was typed into canonical form when the input is left', () => {
    const component = create(field('address', { Slug: {} }), '');
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    component.update('  Hello, World!  ');
    // Nothing is rewritten while it is being typed: only on the way out.
    expect(values).toEqual(['  Hello, World!  ']);

    component.canonicaliseSlug();
    expect(values[1]).toBe('hello-world');
  });

  it('offers to take the slug from the field the schema names', () => {
    const component = create(field('address', { Slug: { generate_from: 'title' } }), '');
    component.siblings = { title: 'Hello World' };
    const values: FieldValue[] = [];
    component.valueChange.subscribe((value) => values.push(value));

    expect(component.canGenerateSlug()).toBe(true);
    component.generateSlug();
    expect(values).toEqual(['hello-world']);

    // Nothing to generate from, or nothing that would change: no button worth pressing.
    component.siblings = { title: '' };
    expect(component.canGenerateSlug()).toBe(false);
    component.siblings = { title: 'Hello World' };
    component.update('hello-world');
    expect(component.canGenerateSlug()).toBe(false);
  });

  it('has no generate option when the schema names no source', () => {
    const component = create(field('address', { Slug: {} }), '');
    component.siblings = { title: 'Hello World' };

    expect(component.slugSource()).toBeNull();
    expect(component.canGenerateSlug()).toBe(false);
  });

  // The same two answers the server gives, before the form is sent.
  it('reports a slug that nothing a URL could use, or one that is too long', () => {
    const component = create(field('address', { Slug: {} }), '');
    const problems: (Message | null)[] = [];
    component.errorChange.subscribe((problem) => problems.push(problem));

    component.update('日本語');
    expect(problems[0]).toEqual(t('errors.invalid_slug', { field: 'address' }));

    component.update('a'.repeat(201));
    expect(problems[1]).toEqual(t('content.valueTooLong', { field: 'address', max: 200 }));

    // Empty is allowed (unless the field is required), and the cap is the type's.
    component.update('');
    expect(problems[2]).toBeNull();
    component.update('a'.repeat(200));
    expect(problems[3]).toBeNull();
  });

  it('says a field has to be unique, which only the server can check', () => {
    create(field('slug', { Text: {} }, { unique: true }));
    expect(query('.unique-mark')?.textContent?.trim()).toBe('unique');

    create(field('title', { Text: {} }));
    expect(query('.unique-mark')).toBeFalsy();
  });

  // Several references inside a composite are an array of relations, so the sub-field is edited by
  // the relation widget in its many mode: choosing there is choosing for the composite that holds
  // it.
  it('edits an array of relations that lives inside a composite', async () => {
    const http = TestBed.inject(HttpTestingController);
    const component = create(field('cta', { CompositeField: { id: 'cta' } }), {
      id: 'cta',
      values: { author: [{ target: 'authors', item: 1 }] },
    });
    const emitted: FieldValue[] = [];
    component.valueChange.subscribe((value) => emitted.push(value));
    await fixture.whenStable();

    // The nested instance holds what the composite holds, and the target's schema names it.
    http
      .expectOne((request) => request.url === '/api/models/collections/authors/items/titles')
      .flush({ 1: 'Ada' });
    await fixture.whenStable();
    fixture.detectChanges();

    const nested = fixture.debugElement.queryAll(By.directive(ValueField)).at(-1)
      ?.componentInstance as ValueField;
    expect(nested.kind()).toBe('RelationArray');
    const relation = fixture.debugElement.queryAll(By.directive(RelationField)).at(-1)
      ?.componentInstance as RelationField;
    expect(relation.value).toEqual([{ target: 'authors', item: 1 }]);
    expect(relation.multiple).toBe(true);
    expect(query('fieldset.composite button.relation-add')).toBeTruthy();
    const chips = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>(
        'fieldset.composite mat-chip-row .reference-label',
      ),
      (chip: HTMLElement) => chip.textContent?.trim(),
    );
    expect(chips).toEqual(['Ada']);

    // A list, so it grows; the emitted value is the whole composite, not the nested field alone.
    relation.toggleReference({ target: 'authors', item: 2 });
    expect(emitted.at(-1)).toEqual({
      author: [
        { target: 'authors', item: 1 },
        { target: 'authors', item: 2 },
      ],
    });
  });

  // The relation widget has specs of its own; what matters here is that this component renders it
  // and passes the value on, the way it does for every other kind.
  it('hands a relation value to the relation field', () => {
    const component = create(
      field('author', { Relation: { target: { kind: 'collection', name: 'authors' } } }),
      { target: 'authors', item: 1 },
    );
    const relation = fixture.debugElement.query(By.directive(RelationField))
      ?.componentInstance as RelationField;

    expect(relation?.value).toEqual({ target: 'authors', item: 1 });
    expect(relation?.multiple).toBe(false);
    expect(relation?.targets).toEqual([{ kind: 'collection', name: 'authors' }]);

    relation?.valueChange.emit({ target: 'authors', item: 2 });
    expect(component.value).toEqual({ target: 'authors', item: 2 });
  });

  // Several references are an array whose item types are all relations, and they are edited with
  // the same widget in its many mode rather than as JSON.
  it('renders an array of relations as the relation field in its many mode', () => {
    const component = create(
      field('related', {
        Array: [
          { Relation: { target: { kind: 'collection', name: 'authors' } } },
          { Relation: { target: { kind: 'single_page', name: 'home' } } },
        ],
      }),
      [{ target: 'authors', item: 1 }, { target: 'home' }],
    );
    const relation = fixture.debugElement.query(By.directive(RelationField))
      ?.componentInstance as RelationField;

    expect(component.kind()).toBe('RelationArray');
    expect(relation?.multiple).toBe(true);
    expect(relation?.targets).toEqual([
      { kind: 'collection', name: 'authors' },
      { kind: 'single_page', name: 'home' },
    ]);
    expect(relation?.value).toEqual([{ target: 'authors', item: 1 }, { target: 'home' }]);

    relation?.valueChange.emit([{ target: 'home' }]);
    expect(component.value).toEqual([{ target: 'home' }]);
  });

  it('converts date-times between local input and RFC 3339', () => {
    const component = create(field('at', 'DateTime'), '2024-03-01T10:00:00.000Z');

    // The exact local rendering depends on the test machine's timezone, so assert the
    // round trip rather than a literal string.
    const local = component.toDateTimeLocal();
    expect(local).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/);

    component.setDateTime('2024-03-01T10:00');
    expect(component.value).toBe(new Date('2024-03-01T10:00').toISOString());

    component.setDateTime('');
    expect(component.value).toBeNull();
  });

  it('hands an image value to the image field, which owns the controls for it', () => {
    // The single image widget has a spec of its own; what matters here is that this component
    // renders it and passes the value on, the way it does for every other kind.
    const component = create(field('photo', 'Image'), { id: 3, url: '/images/logo.png' });
    const image = fixture.debugElement.query(By.directive(ImageField))
      ?.componentInstance as ImageField;

    expect(image).toBeTruthy();
    expect(image?.value).toEqual({ id: 3, url: '/images/logo.png' });

    image?.valueChange.emit({ id: 4, url: '/images/photo.png' });
    expect(component.value).toEqual({ id: 4, url: '/images/photo.png' });
  });

  // The comparison of a draft against what is published reads the value; it is not a second form.
  // A box holding the height the Schema asked for is blank space over one sentence.
  it('shows a multi-line value as text when it is only being read', () => {
    create(field('body', { Text: { multiline: true } }, { height: 6 }), 'one line', true);

    expect(query('textarea')).toBeNull();
    expect(query('.value-text')?.textContent?.trim()).toBe('one line');
  });

  it('keeps the box while the value can be edited', () => {
    create(field('body', { Text: { multiline: true } }, { height: 6 }), 'one line');

    expect(query('textarea')).not.toBeNull();
    expect(query('.value-text')).toBeNull();
  });

  // The comparison reads the two versions; it is not a second form. A field whose Schema asks for a
  // tall box used to be drawn at that height on both sides, which is blank space over a sentence.
  it('reads the tall fields of a composite as text too', () => {
    create(
      field('block', { CompositeField: { id: 'article' } }),
      { lede: 'one line', body: '# heading' },
      true,
    );

    expect(fixture.nativeElement.querySelectorAll('fieldset.composite textarea').length).toBe(0);
    const texts = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('fieldset.composite .value-text'),
      (element) => element.textContent?.trim(),
    );
    expect(texts).toEqual(['one line', '# heading']);
  });

  it('drops the layout minimum from a composite it is only reading', () => {
    create(field('block', { CompositeField: { id: 'layout' } }), { headline: 'x' }, true);

    const cells = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('fieldset.composite .field-cell'),
    );
    expect(cells.length).toBeGreaterThan(0);
    // The definition asks for heights, so without this the cells would hold them.
    expect(cells.every((cell) => cell.style.minHeight === '')).toBe(true);
  });

  it('keeps the layout minimum in a composite that is being edited', () => {
    create(field('block', { CompositeField: { id: 'layout' } }), { headline: 'x' });

    const cells = Array.from(
      fixture.nativeElement.querySelectorAll<HTMLElement>('fieldset.composite .field-cell'),
    );
    expect(cells.some((cell) => cell.style.minHeight !== '')).toBe(true);
  });

  it('edits an image array that lives inside a composite', async () => {
    const component = create(field('block', { CompositeField: { id: 'gallery' } }), {
      images: [{ id: 3, url: '/images/logo.png' }],
    });
    fixture.detectChanges();

    // Sub-fields are edited by this same component, so the nested array gets the same UI.
    const nested = fixture.nativeElement.querySelectorAll('fieldset.composite .array-item');
    expect(nested.length).toBe(1);

    (query('fieldset.composite button.array-add') as HTMLButtonElement).click();
    await fixture.whenStable();
    // One picker serves the whole form, so the wall is in the document rather than inside the field.
    (document.querySelectorAll('.thumb')[1] as HTMLButtonElement).click();
    await fixture.whenStable();
    (document.querySelector('button.array-add-selected') as HTMLButtonElement).click();
    await fixture.whenStable();
    fixture.detectChanges();

    expect(component.value).toEqual({
      images: [
        { id: 3, url: '/images/logo.png' },
        { id: 4, url: '/images/photo.png' },
      ],
    });
  });
});
