import {
  Component,
  EventEmitter,
  Input,
  OnChanges,
  Output,
  SimpleChanges,
  TemplateRef,
  inject,
  signal,
} from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { TranslocoPipe } from '@jsverse/transloco';

import { fieldCellStyle } from 'app/core/field-layout';
import { Message, failure } from 'app/core/i18n/message';
import { FieldSchema, isCompositeFieldSchema } from 'app/models/schema/fields';
import { FieldValue, withDefaults } from 'app/models/values/fields';
import { ContentValue } from 'app/models/values/single-page';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
import { ProblemCollector } from 'app/shared/value-field/problem-collector';

/**
 * What one sub-field of a composite needs.
 *
 * The widget lays the sub-fields out; what each one is edited with belongs to the field above,
 * which is the component that knows how to render any field - and which this widget cannot name,
 * since that component renders this one for a composite. So it is handed a template, and the
 * context carries the cell's layout and the callbacks the sub-field editor binds to.
 */
export interface CompositeSubFieldContext {
  /** The sub-field's own schema. */
  schema: FieldSchema;
  /** What the sub-field holds now. */
  value: FieldValue;
  /** Where the cell sits in the definition's 12-column grid. */
  style: Record<string, string>;
  /** A sub-field editor emits the value it edited. */
  changed: (value: FieldValue) => void;
  /** A sub-field editor reports the problem it holds, or null. */
  problem: (problem: Message | null) => void;
}

/**
 * A composite field: the sub-fields of the definition it names, laid out the way the definition
 * was drawn.
 *
 * The definition is fetched by id, so a composite is defined once and used by any number of
 * collections and pages. Reads wrap the sub-values as `{id, values}`; what a write takes, and
 * what this widget emits, is the bare object of sub-values.
 *
 * Each sub-field is edited by the field above, which renders one editor per sub-field through the
 * template it handed down: that is what makes composites nest to any depth without the two
 * components importing each other.
 */
@Component({
  selector: 'app-composite-field',
  imports: [NgTemplateOutlet, TranslocoPipe],
  templateUrl: './composite-field.html',
  styleUrl: './composite-field.scss',
})
export class CompositeField implements OnChanges {
  private compositeFields = inject(CompositeFieldsService);

  @Input({ required: true }) field!: FieldSchema;
  @Input() value: FieldValue = null;
  /** Renders the sub-fields read-only, for the schema editor's preview. */
  @Input() disabled = false;
  /** How one sub-field is edited and laid out; see [`CompositeSubFieldContext`]. */
  @Input({ required: true }) subFieldTemplate!: TemplateRef<CompositeSubFieldContext>;
  @Output() valueChange = new EventEmitter<FieldValue>();
  @Output() errorChange = new EventEmitter<Message | null>();

  /** The definition's sub-schema, or null while it is being fetched or when it is not defined. */
  public compositeSchema = signal<FieldSchema[] | null>(null);
  public compositeId = signal('');
  public compositeValues: ContentValue = {};

  /** The last value this component emitted, so its own output is not mistaken for new input. */
  private lastEmitted: FieldValue = null;

  /** Problems reported by sub-fields, so one clearing does not clear another's. */
  private problems = new ProblemCollector((problem) => this.errorChange.emit(problem));

  ngOnChanges(changes: SimpleChanges) {
    if (changes['field']) {
      this.loadCompositeSchema();
    } else if (changes['value']) {
      this.syncCompositeValues();
    }
  }

  /** The context one sub-field editor is rendered with. */
  subFieldContext(schema: FieldSchema): CompositeSubFieldContext {
    return {
      schema,
      value: this.compositeValues[schema.name],
      style: fieldCellStyle(schema),
      changed: (value: FieldValue) => this.setCompositeValue(schema, value),
      problem: (problem: Message | null) => this.forwardCompositeError(schema, problem),
    };
  }

  setCompositeValue(subField: FieldSchema, value: FieldValue) {
    this.compositeValues[subField.name] = value;
    // Emit the bare object of sub-values: that is what a composite write accepts. The
    // `{id, values}` wrapper only appears on reads.
    this.update({ ...this.compositeValues });
  }

  forwardCompositeError(subField: FieldSchema, problem: Message | null) {
    // One at a time: the parent shows the first problem and refuses to save until none remain,
    // so naming the others too would only lengthen the message.
    this.problems.set(subField.name, problem);
  }

  private update(value: FieldValue) {
    this.lastEmitted = value;
    this.valueChange.emit(value);
  }

  private loadCompositeSchema() {
    const type = this.field.field_type;
    if (!isCompositeFieldSchema(type)) {
      return;
    }
    const id = String(type.CompositeField.id);
    this.compositeId.set(id);
    this.compositeFields.getAllCompositeFields().subscribe({
      next: (all) => {
        const schema = all[id] ?? null;
        this.compositeSchema.set(schema);
        this.compositeValues = schema ? withDefaults(schema, this.innerCompositeValue(schema)) : {};
      },
      error: (e) => this.errorChange.emit(failure('content.failedToLoadComposite', e, { id })),
    });
  }

  private syncCompositeValues() {
    const schema = this.compositeSchema();
    if (!schema || this.value === this.lastEmitted) {
      return;
    }
    this.compositeValues = withDefaults(schema, this.innerCompositeValue(schema));
  }

  /**
   * A composite's own sub-values.
   *
   * Reads wrap them as `{id, values}`, so the wrapper is unwrapped here — except when the
   * composite genuinely declares a sub-field called `values`, which mirrors what the
   * server does.
   */
  private innerCompositeValue(schema: FieldSchema[]): ContentValue {
    const value = this.value;
    if (value === null || typeof value !== 'object' || Array.isArray(value)) {
      return {};
    }
    const record = value;
    const declaresValues = schema.some((field) => field.name === 'values');
    const wrapped = record['values'];
    if (
      !declaresValues &&
      wrapped !== null &&
      typeof wrapped === 'object' &&
      !Array.isArray(wrapped)
    ) {
      return wrapped;
    }
    return record;
  }
}
