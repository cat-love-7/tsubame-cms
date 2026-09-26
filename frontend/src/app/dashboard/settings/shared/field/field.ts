import {
  Component,
  EventEmitter,
  Input,
  OnChanges,
  OnInit,
  Output,
  SimpleChanges,
  inject,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { TranslocoPipe } from '@jsverse/transloco';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipsModule } from '@angular/material/chips';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatIconModule } from '@angular/material/icon';
import { MatInputModule } from '@angular/material/input';
import { MatSelectModule } from '@angular/material/select';
import {
  ArrayItemTypeOptions,
  DefaultFieldLayout,
  FieldDefaults,
  FieldSchema,
  FieldType,
  FieldTypeStringPipe,
  IsArrayFieldSchemaPipe,
  IsCompositeFieldSchemaPipe,
  IsEnumFieldSchemaPipe,
  IsMarkdownFieldSchema,
  IsTextFieldSchema,
  RelationArrayItemType,
  RelationOptions,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isMarkdownFieldSchema,
  isRelationFieldSchema,
  isSlugFieldSchema,
  isTextFieldSchema,
  newFieldType,
  reconcileArrayItemTypes,
} from 'app/models/schema/fields';
import { EnumField } from '../enum-field/enum-field';
import { CompositeField } from '../composite-field/composite-field';
import { TextField } from '../text-field/text-field';
import { FieldWidthPresets } from 'app/core/field-layout';
import { CompositeFieldsService } from 'app/services/schema/composite-fields.service';
import { CollectionsService } from 'app/services/schema/collections.service';
import { SinglePagesService } from 'app/services/schema/single-pages.service';

@Component({
  selector: 'app-field',
  imports: [
    FormsModule,
    MatFormFieldModule,
    MatInputModule,
    MatSelectModule,
    MatCheckboxModule,
    MatChipsModule,
    MatIconModule,
    IsTextFieldSchema,
    TranslocoPipe,
    IsMarkdownFieldSchema,
    IsCompositeFieldSchemaPipe,
    IsArrayFieldSchemaPipe,
    IsEnumFieldSchemaPipe,
    FieldTypeStringPipe,
    EnumField,
    CompositeField,
    TextField,
  ],
  templateUrl: './field.html',
  styleUrl: './field.scss',
})
export class Field implements OnInit, OnChanges {
  private compositeFields = inject(CompositeFieldsService);
  private collections = inject(CollectionsService);
  private singlePages = inject(SinglePagesService);

  @Input() field: FieldSchema = {
    name: '',
    // A copy, not the shared default: this editor writes into the type.
    field_type: newFieldType('Text'),
    required: false,
    ...DefaultFieldLayout,
  };
  /**
   * Whether this screen may offer `unique` at all.
   *
   * A collection holds many items, so a value can be compared across them; a single page holds
   * one and a composite definition's fields are embedded in the item that uses them, so the
   * server refuses it there and the control would only ever fail.
   */
  @Input() uniqueAllowed = true;
  /**
   * Whether this screen may offer `show_in_list` at all.
   *
   * Only a collection has a list of its items; a single page holds one item and a composite
   * definition's values live inside whatever item uses them, so there is nothing to list.
   */
  @Input() listAllowed = false;
  /**
   * Whether this screen may offer `is_title` at all.
   *
   * A collection's items and a page are the things a relation can point at, so those are the
   * schemas that can name one; a composite definition is embedded in whatever item uses it, so a
   * reference never names it on its own.
   */
  @Input() titleAllowed = false;
  /**
   * Every field of the schema this one belongs to.
   *
   * A slug may name one of them as the source its value is generated from, and the picker can only
   * offer what the schema holds.
   */
  @Input() siblingFields: FieldSchema[] = [];
  @Output() fieldChange = new EventEmitter<FieldSchema>();

  /**
   * The composite definitions an array may hold, by id.
   *
   * Asked for only when the field is an array: the control that needs it is the only one that
   * would show it, and a schema's text fields have no reason to fetch the list.
   */
  public compositeIds: string[] = [];

  /**
   * The three halves of an array's item types.
   *
   * Cached rather than computed in the template: a method that returned a fresh array would
   * hand the multi-select a new value on every change detection pass, and the binding would
   * never settle. Relations are cached too, because each one carries a target and an inverse
   * name that are edited in a control of their own.
   */
  public scalarTypes: FieldType[] = [];
  public compositeTypeIds: string[] = [];
  public relationItems: RelationOptions[] = [];
  /** What the item-type multi-select shows as selected: the scalars, plus one marker for relations. */
  public selectedItemTypes: FieldType[] = [];

  /**
   * The names a relation may point at, by target kind.
   *
   * A relation can point at any collection or any single page, so both lists are needed; they are
   * fetched only when a field is a relation, like the composite ids an array needs.
   */
  public collectionNames: string[] = [];
  public pageNames: string[] = [];

  private compositeIdsRequested = false;
  private relationTargetsRequested = false;

  ngOnInit() {
    this.refreshItemTypes();
    this.loadCompositeIds();
    this.loadRelationTargets();
  }
  public typeOptions = Object.keys(FieldDefaults);
  public arrayItemTypeOptions = ArrayItemTypeOptions;
  public widthPresets = FieldWidthPresets;
  public relationTargetKinds = [
    { label: 'content.collection', value: 'collection' as const },
    { label: 'content.singlePage', value: 'single_page' as const },
  ];

  /** Whether this field is a slug, whose only option is where to generate the value from. */
  isSlug(): boolean {
    return isSlugFieldSchema(this.field.field_type);
  }

  /** The fields a slug could be generated from: text and markdown, and never itself. */
  slugSourceOptions(): FieldSchema[] {
    return this.siblingFields.filter(
      (candidate) =>
        candidate.name !== this.field.name &&
        (isTextFieldSchema(candidate.field_type) || isMarkdownFieldSchema(candidate.field_type)),
    );
  }

  /** The source the schema currently names, or `''` for none. */
  currentGenerateFrom(): string {
    const type = this.field.field_type;
    return isSlugFieldSchema(type) ? (type.Slug.generate_from ?? '') : '';
  }

  setGenerateFrom(name: string) {
    const type = this.field.field_type;
    if (!isSlugFieldSchema(type)) {
      return;
    }
    // Undefined rather than an empty string: "no suggestion" is the absence of an option, and the
    // server drops it from the wire.
    type.Slug.generate_from = name === '' ? undefined : name;
    this.fieldChange.emit(this.field);
  }

  /** Typing a raw column count is not intuitive; the presets cover the common fractions. */
  public setWidth(width: number) {
    this.field.width = width;
    this.fieldChange.emit(this.field);
  }

  /** Whether this field is a relation, whose options are its target and its cardinality. */
  isRelation(): boolean {
    return isRelationFieldSchema(this.field.field_type);
  }

  /** The relation's options, or null when this field is not a relation. */
  private relation() {
    const type = this.field.field_type;
    return isRelationFieldSchema(type) ? type.Relation : null;
  }

  relationTargetKind(): 'collection' | 'single_page' {
    return this.relation()?.target.kind ?? 'collection';
  }

  relationTargetName(): string {
    return this.relation()?.target.name ?? '';
  }

  /** The names this relation could point at, for the target it has chosen. */
  relationTargetNames(): string[] {
    return this.relationTargetKind() === 'single_page' ? this.pageNames : this.collectionNames;
  }

  relationInverseName(): string {
    return this.relation()?.inverse_name ?? '';
  }

  /**
   * Switching the kind of target clears the name: a collection and a page may share one, but they
   * are different targets, and keeping the name would silently point the relation somewhere else.
   */
  setRelationTargetKind(kind: 'collection' | 'single_page') {
    const options = this.relation();
    if (!options || options.target.kind === kind) {
      return;
    }
    options.target = { kind, name: '' };
    this.fieldChange.emit(this.field);
  }

  setRelationTargetName(name: string) {
    const options = this.relation();
    if (!options) {
      return;
    }
    options.target = { kind: options.target.kind, name };
    this.fieldChange.emit(this.field);
  }

  setRelationInverseName(name: string) {
    const options = this.relation();
    if (!options) {
      return;
    }
    options.inverse_name = name;
    this.fieldChange.emit(this.field);
  }

  public onFieldTypeChange(value: keyof typeof FieldDefaults) {
    // A copy of the default: this editor writes its limits and item types into the type.
    this.field.field_type = newFieldType(value);
    this.refreshItemTypes();
    // Switching *to* an array is how most arrays are made, and the input object does not
    // change identity when the type does, so `ngOnChanges` never sees it.
    this.loadCompositeIds();
    this.loadRelationTargets();
    this.fieldChange.emit(this.field);
  }

  ngOnChanges(changes: SimpleChanges) {
    if (changes['field']) {
      this.refreshItemTypes();
      this.loadCompositeIds();
      this.loadRelationTargets();
    }
  }

  /**
   * Ask for the definitions an array's item types may name, once.
   *
   * The service caches the list, so a schema with several array fields asks for it once; a
   * field that is not an array never asks at all.
   */
  private loadCompositeIds() {
    if (this.compositeIdsRequested || !isArrayFieldSchema(this.field.field_type)) {
      return;
    }
    this.compositeIdsRequested = true;
    this.compositeFields.getAllCompositeFields().subscribe({
      next: (definitions) => (this.compositeIds = Object.keys(definitions)),
      // Without the list the picker is empty; the rest of the editor still works, and a
      // schema that already references a composite keeps that reference.
      error: () => (this.compositeIds = []),
    });
  }

  /**
   * Ask for the collections and single pages a relation may point at, once.
   *
   * Both lists are needed because a relation can point at either; a field that is not a relation
   * never asks. Without them the target picker is empty, and a relation that already names a target
   * keeps it.
   */
  private loadRelationTargets() {
    // An array may hold relation item types too, so its editor needs the lists even though the
    // field itself is not a relation.
    if (
      this.relationTargetsRequested ||
      !(isRelationFieldSchema(this.field.field_type) || isArrayFieldSchema(this.field.field_type))
    ) {
      return;
    }
    this.relationTargetsRequested = true;
    this.collections.getAllCollectionNames().subscribe({
      next: (names) => (this.collectionNames = names),
      error: () => (this.collectionNames = []),
    });
    this.singlePages.listPageNames().subscribe({
      next: (names) => (this.pageNames = names),
      error: () => (this.pageNames = []),
    });
  }

  private refreshItemTypes() {
    const items = this.arrayItemTypes();
    this.scalarTypes = items.filter((item) => typeof item === 'string');
    this.compositeTypeIds = items
      .filter(isCompositeFieldSchema)
      .map((item) => item.CompositeField.id);
    this.relationItems = items.filter(isRelationFieldSchema).map((item) => item.Relation);
    // One marker stands for every relation item type: the multi-select is a set, and what each
    // relation item type is configured with is chosen underneath it.
    this.selectedItemTypes =
      this.relationItems.length > 0
        ? [...this.scalarTypes, RelationArrayItemType]
        : [...this.scalarTypes];
  }

  private arrayItemTypes(): FieldType[] {
    const fieldType = this.field.field_type;
    return isArrayFieldSchema(fieldType) ? fieldType.Array : [];
  }

  /**
   * Whether two item types are the same choice in the multi-select.
   *
   * A relation item type is a configured object rather than a name, and any of them is the one
   * `Relation` choice, so they all compare equal to the marker.
   */
  public compareItemTypes(first: FieldType, second: FieldType): boolean {
    if (isRelationFieldSchema(first) && isRelationFieldSchema(second)) {
      return true;
    }
    return first === second;
  }

  /**
   * Array item types are untyped on the wire, so `Number` and `Image` cannot be combined
   * (an image id is a number). The binding is one-way, so `field.field_type.Array` is
   * still the previous selection here. The composite and relation item types are carried over:
   * they are chosen in their own controls, and dropping them here would lose them silently.
   */
  public onScalarItemTypesChange(selected: FieldType[]) {
    const fieldType = this.field.field_type;
    if (!isArrayFieldSchema(fieldType)) {
      return;
    }
    // Read the previous selection from the field, not from the cached binding: the cache is
    // for the control, and this decision is about what the field holds right now.
    const previous = fieldType.Array.filter((item) => typeof item === 'string');
    const selectedScalars = selected.filter((item) => typeof item === 'string');
    const composites = fieldType.Array.filter(isCompositeFieldSchema);
    const choseRelations = selected.some(isRelationFieldSchema);
    const relations = fieldType.Array.filter(isRelationFieldSchema).map((item) => item.Relation);
    // Choosing Relation starts one item type; more targets are added in the relation control, and
    // unchoosing it drops them all.
    const kept =
      choseRelations && relations.length > 0
        ? relations
        : choseRelations
          ? [{ target: { kind: 'collection', name: '' } } as RelationOptions]
          : [];
    fieldType.Array = [
      ...reconcileArrayItemTypes(previous, selectedScalars),
      ...composites,
      ...kept.map((options) => ({ Relation: options })),
    ];
    this.refreshItemTypes();
    this.fieldChange.emit(this.field);
  }

  /** Declare which composite definitions this array holds. */
  public onCompositeItemTypesChange(ids: string[]) {
    const fieldType = this.field.field_type;
    if (!isArrayFieldSchema(fieldType)) {
      return;
    }
    const scalars = fieldType.Array.filter((item) => typeof item === 'string');
    const relations = fieldType.Array.filter(isRelationFieldSchema);
    fieldType.Array = [...scalars, ...ids.map((id) => ({ CompositeField: { id } })), ...relations];
    this.refreshItemTypes();
    this.fieldChange.emit(this.field);
  }

  /** Add a relation item type, for an array that already holds at least one. */
  public addRelationItemType() {
    const options: RelationOptions = { target: { kind: 'collection', name: '' } };
    this.relationItems = [...this.relationItems, options];
    this.writeRelationItems();
  }

  /** Take one relation item type away. */
  public removeRelationItemType(index: number) {
    this.relationItems = this.relationItems.filter((_, other) => other !== index);
    this.writeRelationItems();
  }

  /** The kind of a relation item type's target. */
  public arrayRelationTargetKind(index: number): 'collection' | 'single_page' {
    return this.relationItems[index]?.target.kind ?? 'collection';
  }

  /**
   * The names one relation item type could point at.
   *
   * A target already claimed by another item type is left out: an array may name each target once,
   * and the server refuses a schema that names one twice.
   */
  public arrayRelationTargetNames(index: number): string[] {
    const item = this.relationItems[index];
    if (item === undefined) {
      return [];
    }
    const names = item.target.kind === 'single_page' ? this.pageNames : this.collectionNames;
    const taken = this.relationItems
      .filter((_, other) => other !== index)
      .filter((other) => other.target.kind === item.target.kind)
      .map((other) => other.target.name);
    return names.filter((name) => !taken.includes(name) || name === item.target.name);
  }

  /** Switching the kind of a relation item type clears its name, which is a different target. */
  public setArrayRelationTargetKind(index: number, kind: 'collection' | 'single_page') {
    const item = this.relationItems[index];
    if (item === undefined || item.target.kind === kind) {
      return;
    }
    item.target = { kind, name: '' };
    this.writeRelationItems();
  }

  public setArrayRelationTargetName(index: number, name: string) {
    const item = this.relationItems[index];
    if (item === undefined) {
      return;
    }
    item.target = { kind: item.target.kind, name };
    this.writeRelationItems();
  }

  public arrayRelationInverseName(index: number): string {
    return this.relationItems[index]?.inverse_name ?? '';
  }

  public setArrayRelationInverseName(index: number, name: string) {
    const item = this.relationItems[index];
    if (item === undefined) {
      return;
    }
    item.inverse_name = name;
    this.writeRelationItems();
  }

  /** Put the relation item types back into the array, after their own control changed one. */
  private writeRelationItems() {
    const fieldType = this.field.field_type;
    if (!isArrayFieldSchema(fieldType)) {
      return;
    }
    const others = fieldType.Array.filter((item) => !isRelationFieldSchema(item));
    fieldType.Array = [...others, ...this.relationItems.map((options) => ({ Relation: options }))];
    this.refreshItemTypes();
    this.fieldChange.emit(this.field);
  }
}
