// The translation layer: Strapi v3 attributes to CMS field types, and v3 entries to the
// untagged values the CMS accepts.
//
// The two systems do not have the same vocabulary, so every decision that is not a straight
// rename lives here, and every field this layer cannot express is reported rather than dropped
// silently (see `unmapped` and the migration report).

/** The kinds of field this layer knows how to carry over. */
export const FieldKind = {
  Scalar: 'scalar',
  Image: 'image',
  Component: 'component',
  DynamicZone: 'dynamiczone',
  /** A reference to another collection's items, or to a single page. */
  Relation: 'relation',
};

/**
 * The empty value the CMS expects for a field type.
 *
 * A field is omitted rather than guessed at when nothing can be said about it, but an editor
 * opening the item should still see every field, so a mapped field always gets a value of the
 * right shape.
 */
export function emptyValue(fieldType) {
  if (typeof fieldType === 'string') {
    switch (fieldType) {
      case 'Text':
      case 'Slug':
      case 'Markdown':
        return '';
      case 'Number':
      case 'Date':
      case 'DateTime':
      case 'Image':
        return null;
      case 'Boolean':
        return false;
      default:
        return null;
    }
  }
  const [variant] = Object.keys(fieldType);
  if (variant === 'Array' || variant === 'TextEnum' || variant === 'Relation') return [];
  if (variant === 'CompositeField') return null;
  return null;
}

/** A CMS field definition, matching `FieldSchema`'s wire shape. */
function fieldSchema(name, fieldType, { required = false, unique = false } = {}) {
  const field = {
    name,
    field_type: fieldType,
    required,
    width: 12,
    height: 1,
  };
  // `unique`, `show_in_list` and `is_title` are omitted when false, exactly as the CMS omits them.
  if (unique) field.unique = true;
  return field;
}

function textOptions(attribute) {
  // v3 keeps character limits in `maxLength` / `minLength`; `max` / `min` are the *numeric*
  // bounds for number attributes and must not be mistaken for lengths.
  const options = { max_length: null, min_length: null };
  if (Number.isInteger(attribute.maxLength)) options.max_length = attribute.maxLength;
  if (Number.isInteger(attribute.minLength)) options.min_length = attribute.minLength;
  return options;
}

const plainText = () => ({ Text: { max_length: null, min_length: null } });

/**
 * Whether an attribute is a media field from v3's upload plugin.
 *
 * Two spellings arrive here. A project's `settings.json` writes the relation to the upload
 * plugin's `file` model (`{ "model": "file", "plugin": "upload" }`), while the admin
 * content-type-builder *normalises* it (`{ "type": "media", "multiple": true }`).
 */
function isUploadAttribute(attribute) {
  if (attribute.type === 'media') return true;
  const target = attribute.model ?? attribute.collection;
  return target === 'file' || attribute.plugin === 'upload';
}

function isUploadMultiple(attribute) {
  if (attribute.type === 'media') return attribute.multiple === true;
  return Boolean(attribute.collection);
}

/**
 * Whether an attribute is a relation to another content type.
 *
 * Again two spellings: `settings.json` carries `model`/`collection`, and the content-type-builder
 * normalises to `nature`/`target` with no `type` at all. A typed attribute that is not a
 * relation (a scalar, a component, a dynamic zone) is never one, so `type` decides first.
 */
export function isRelationAttribute(attribute) {
  if (attribute.type && attribute.type !== 'relation') return false;
  if (attribute.nature || attribute.target) return true;
  return Boolean(attribute.model || attribute.collection);
}

/**
 * The content-type uid a relation attribute points at, whichever spelling wrote it.
 *
 * Two spellings name the same thing: a project's `settings.json` writes the bare model name
 * (`post`), while the admin content-type-builder answers a full uid
 * (`application::post.post`, `plugins::users-permissions.user`). The CMS is asked for the name,
 * so the namespace and the leading segment are stripped here.
 */
function relationTargetUid(attribute) {
  const raw = attribute.model ?? attribute.collection ?? attribute.target ?? null;
  if (typeof raw !== 'string' || raw === '') return null;
  const withoutNamespace = raw.includes('::') ? raw.slice(raw.indexOf('::') + 2) : raw;
  const parts = withoutNamespace.split('.');
  return parts[parts.length - 1];
}

/**
 * Whether a relation holds a **set** rather than one reference.
 *
 * `settings.json` says it with `collection`, the content-type-builder with `nature`: `oneToMany`,
 * `manyToMany` and `manyWay` all arrive as an array in the API, `manyToOne`, `oneToOne` and
 * `oneWay` as a single object.
 */
function isManyRelation(attribute) {
  if (attribute.collection) return true;
  return ['oneToMany', 'manyToMany', 'manyWay'].includes(attribute.nature);
}

/**
 * The name the other side uses for this relation.
 *
 * `settings.json` writes it as `via`; the content-type-builder normalises it to `targetAttribute`.
 */
function inverseNameOf(attribute) {
  const name = attribute.via ?? attribute.targetAttribute ?? null;
  return typeof name === 'string' && name !== '' ? name : null;
}

/** Whether `attribute` points back at the content type `owner` describes. */
function pointsAtOwner(attribute, owner) {
  const uid = relationTargetUid(attribute);
  if (!uid || !owner) return false;
  const wanted = uid.toLowerCase();
  return [owner.apiId, owner.name].filter(Boolean).some((name) => String(name).toLowerCase() === wanted);
}

/**
 * The two fields that make up one mutual relation, or `null` when there is no inverse.
 *
 * Strapi declares a link on **both** content types; this CMS defines it on one side only and
 * answers the other from an index. Whichever side is picked, the pair is the same two fields, so
 * naming them in one place is what lets the automatic choice, an operator's choice
 * (`--relation-owner`) and the check that they do not contradict each other all agree.
 *
 * `via`/`targetAttribute` that names a field which does not point back (a stale or hand-written
 * schema) is not an inverse at all, and neither is a relation on a target this run is not
 * migrating.
 *
 * @returns {{ mine: string, theirs: string, peer: object }|null}
 */
export function mutualRelationPair(attribute, fieldName, owner, ctx) {
  const targetUid = relationTargetUid(attribute);
  const target = targetUid ? ctx.contentTypeFor(targetUid) : null;
  if (!target) return null;

  const peerName = inverseNameOf(attribute) ?? peerNaming(attribute, fieldName, owner, target);
  if (!peerName) return null;

  const peer = target.attributes?.[peerName];
  if (!peer || !isRelationAttribute(peer) || !pointsAtOwner(peer, owner)) return null;

  return {
    mine: `${owner?.apiId ?? ''}.${fieldName}`,
    theirs: `${target.apiId}.${peerName}`,
    peer,
  };
}

/**
 * The field on `target` that declares *this* field as its inverse.
 *
 * A one-to-many is written from both ends, but only one of them carries `via`: the "many" side
 * (`{ "model": "category" }`) says nothing about the other end, and it is the side that holds the
 * values. Without this search that side looks one-way, which would leave an operator's
 * `--relation-owner` for it silently ignored - and migrating both sides would then store the link
 * twice, which is the one thing the single-side rule exists to prevent.
 */
function peerNaming(attribute, fieldName, owner, target) {
  for (const [name, candidate] of Object.entries(target.attributes ?? {})) {
    if (!isRelationAttribute(candidate)) continue;
    if (inverseNameOf(candidate) !== fieldName) continue;
    if (!pointsAtOwner(candidate, owner)) continue;
    return name;
  }
  return null;
}

/**
 * Whether this side of a Strapi relation is the one to migrate.
 *
 * One side is adopted and the other is reported, because the CMS stores a link once and derives
 * the other direction from its index. The rules cover the shapes Strapi writes:
 *
 *   * a one-way relation is the only side there is - adopt it;
 *   * an operator's choice (`--relation-owner`) wins over everything below - the flag exists
 *     because which way round a relation is read is a question about the site, not about Strapi;
 *   * when the two sides differ in cardinality (the `oneToMany` / `manyToOne` pair), the **single**
 *     reference is kept. That is the shape `doc/relations-design.md` describes ("put the single
 *     reference on the many side to express a one-to-many"), and it means the collection of
 *     children is derived rather than stored;
 *   * when both sides hold a set (a many-to-many; the two spellings of a `manyWay`) one is picked
 *     by name - the same choice every run, which is what a re-runnable migration needs.
 */
export function shouldAdoptRelation(attribute, fieldName, owner, ctx) {
  const pair = mutualRelationPair(attribute, fieldName, owner, ctx);
  if (pair === null) return { adopt: true };
  const { mine, theirs, peer } = pair;

  const chosen = ctx.relationOwners;
  if (chosen?.has(mine) && !chosen.has(theirs)) return { adopt: true };
  if (chosen?.has(theirs) && !chosen.has(mine)) {
    return { adopt: false, reason: `${theirs} was chosen to own it (--relation-owner)` };
  }

  const mineIsMany = isManyRelation(attribute);
  const peerIsMany = isManyRelation(peer);
  if (mineIsMany !== peerIsMany) {
    return mineIsMany
      ? {
          adopt: false,
          reason: `the inverse of ${theirs}, which owns the relation (it holds the single reference)`,
        }
      : { adopt: true };
  }
  // Both hold a set: adopt one, deterministically.
  return mine <= theirs
    ? { adopt: true }
    : { adopt: false, reason: `the other side of a many-to-many (${theirs}) is migrated instead` };
}

/**
 * Every relation a run would meet, and which side it keeps by default.
 *
 * `--relation-owner` takes a field name, so this is how an operator finds the names: the same walk
 * the planner does, reported instead of applied. Relations are grouped by what happens to them:
 *
 *   * `mutual` - both content types declare it, so one side is kept and the other is derived from
 *     the index. `flip` is the flag that would keep the other side instead;
 *   * `oneWay` - the only side there is;
 *   * `dropped` - the target is not one of the content types this run migrates (a plugin's table,
 *     or a type excluded with `--only` / `--skip`).
 *
 * Components are walked too, because a relation inside one is migrated now; there is no inverse to
 * find there (relations point at content types, never at a component), so those are one-way.
 */
export function relationReport(contentTypes, components, ctx) {
  const mutual = [];
  const oneWay = [];
  const dropped = [];
  const seenPairs = new Set();

  const visit = (owner, attributes, where) => {
    for (const [name, attribute] of Object.entries(attributes ?? {})) {
      // Media is a reference too, but not one `--relation-owner` can name: an image is an image,
      // and the planner handles it before relations are ever considered.
      if (isUploadAttribute(attribute)) continue;
      if (!isRelationAttribute(attribute)) continue;
      const field = `${owner.apiId}.${name}`;

      const pair = mutualRelationPair(attribute, name, owner, ctx);
      if (pair) {
        const signature = [pair.mine, pair.theirs].sort().join('|');
        if (seenPairs.has(signature)) continue;
        seenPairs.add(signature);
        const decision = shouldAdoptRelation(attribute, name, owner, ctx);
        mutual.push({
          mine: pair.mine,
          theirs: pair.theirs,
          mineMany: isManyRelation(attribute),
          theirsMany: isManyRelation(pair.peer),
          owner: decision.adopt ? pair.mine : pair.theirs,
          other: decision.adopt ? pair.theirs : pair.mine,
        });
        continue;
      }

      const targetUid = relationTargetUid(attribute);
      const target = targetUid ? ctx.contentTypeFor(targetUid) : null;
      if (!target) {
        dropped.push({ field, target: targetUid, where });
        continue;
      }
      oneWay.push({
        field,
        target: target.cmsName,
        // A single page holds one item, so the CMS folds a stray `collection:` - the report says
        // what will really be written rather than what Strapi declared.
        hasMany: target.kind === 'singleType' ? false : isManyRelation(attribute),
        where,
      });
    }
  };

  for (const contentType of contentTypes) {
    visit({ apiId: contentType.apiId, name: contentType.name }, contentType.attributes, 'content');
  }
  for (const component of components) {
    visit({ apiId: component.id, name: component.id }, component.attributes, 'composite');
  }
  return { mutual, oneWay, dropped };
}

/**
 * Check the operator's `--relation-owner` choices against the relations that actually exist.
 *
 * A name that matches nothing is a typo that would silently leave the automatic choice in place,
 * and naming both sides of one relation is a contradiction the CMS cannot satisfy (it stores the
 * link once) - both are reported rather than quietly resolved.
 *
 * @returns {string[]} one message per problem, empty when the choices are usable
 */
export function relationOwnerProblems(contentTypes, chosen, ctx) {
  if (!chosen || chosen.size === 0) return [];

  const known = new Map();
  for (const contentType of contentTypes) {
    for (const [name, attribute] of Object.entries(contentType.attributes ?? {})) {
      if (isRelationAttribute(attribute)) known.set(`${contentType.apiId}.${name}`, { contentType, attribute });
    }
  }

  const problems = [];
  for (const key of chosen) {
    if (!known.has(key)) {
      problems.push(
        `--relation-owner names '${key}', which is not a relation field of a content type being migrated`,
      );
    }
  }

  const reported = new Set();
  for (const [key, { contentType, attribute }] of known) {
    if (!chosen.has(key)) continue;
    const owner = { apiId: contentType.apiId, name: contentType.name };
    const pair = mutualRelationPair(attribute, key.slice(key.lastIndexOf('.') + 1), owner, ctx);
    if (!pair || !chosen.has(pair.theirs)) continue;
    const signature = [pair.mine, pair.theirs].sort().join('|');
    if (reported.has(signature)) continue;
    reported.add(signature);
    problems.push(
      `--relation-owner names both sides of one relation: '${pair.mine}' and '${pair.theirs}' ` +
        '(the CMS stores a link once)',
    );
  }
  return problems;
}

/**
 * Map one v3 attribute to a CMS field.
 *
 * Returns either `{ skip: true, reason }` or a description the value converter can follow:
 * `{ kind, fieldType, componentId?, target?, hasMany?, multiple?, options? }`.
 *
 * @param {object} attribute  One entry of `settings.json`'s `attributes`.
 * @param {object} ctx
 * @param {(uid: string) => object|null} ctx.contentTypeFor  A migrated content type by uid.
 * @param {(uid: string) => string|null} ctx.componentIdOf  A v3 component uid, as a CMS id.
 * @param {'skip'|'text'|'relation'} ctx.relationsMode  What to do with a relation.
 * @param {{apiId: string, name: string}|undefined} owner  The content type holding this attribute.
 * @param {string} [fieldName]  The attribute's name, needed to recognise a mutual relation.
 */
export function mapAttribute(attribute, ctx, owner, fieldName = '') {
  if (isUploadAttribute(attribute)) {
    const multiple = isUploadMultiple(attribute);
    return {
      kind: FieldKind.Image,
      multiple,
      fieldType: multiple ? { Array: ['Image'] } : 'Image',
    };
  }

  if (isRelationAttribute(attribute)) {
    return mapRelation(attribute, ctx, owner, fieldName);
  }

  switch (attribute.type) {
    case 'string':
    case 'text':
      return {
        kind: FieldKind.Scalar,
        fieldType: { Text: textOptions(attribute) },
        unique: attribute.unique === true,
      };
    case 'email':
      return { kind: FieldKind.Scalar, fieldType: { Text: textOptions(attribute) } };
    case 'richtext':
      return { kind: FieldKind.Scalar, fieldType: { Markdown: textOptions(attribute) } };
    case 'uid':
      // A v3 uid and a CMS slug are both "a URL-safe value derived from a title" - the same
      // idea, and the CMS normalises it on write.
      return { kind: FieldKind.Scalar, fieldType: { Slug: {} } };
    case 'password':
      return { skip: true, reason: 'password (credentials are not content)' };
    case 'integer':
    case 'biginteger':
    case 'float':
    case 'decimal':
      return { kind: FieldKind.Scalar, fieldType: 'Number' };
    case 'boolean':
      return { kind: FieldKind.Scalar, fieldType: 'Boolean' };
    case 'date':
      return { kind: FieldKind.Scalar, fieldType: 'Date' };
    case 'datetime':
    case 'timestamp':
      return { kind: FieldKind.Scalar, fieldType: 'DateTime' };
    case 'time':
      // This CMS has no time-of-day type; the value is carried as text so it is not lost.
      return { kind: FieldKind.Scalar, fieldType: plainText() };
    case 'enumeration':
      return {
        kind: FieldKind.Scalar,
        fieldType: { TextEnum: (attribute.enum ?? []).map(String) },
        options: (attribute.enum ?? []).map(String),
      };
    case 'json':
      // No JSON field type exists, so the value is carried as text holding its JSON encoding.
      return { kind: FieldKind.Scalar, fieldType: plainText(), jsonText: true };
    case 'component': {
      const componentId = ctx.componentIdOf(attribute.component);
      if (!componentId) {
        return { skip: true, reason: `component '${attribute.component}' was not found` };
      }
      const multiple = attribute.repeatable === true;
      return {
        kind: FieldKind.Component,
        componentId,
        multiple,
        fieldType: multiple ? { Array: [{ CompositeField: { id: componentId } }] } : { CompositeField: { id: componentId } },
      };
    }
    case 'dynamiczone': {
      const componentIds = (attribute.components ?? []).map(ctx.componentIdOf).filter(Boolean);
      if (componentIds.length === 0) {
        return { skip: true, reason: 'dynamic zone with no known component' };
      }
      return {
        kind: FieldKind.DynamicZone,
        componentIds,
        multiple: true,
        fieldType: { Array: componentIds.map((id) => ({ CompositeField: { id } })) },
      };
    }
    default:
      return { skip: true, reason: `unsupported attribute type '${attribute.type ?? 'relation'}'` };
  }
}

/** A relation, as a real reference, a text stand-in, or a reported omission. */
function mapRelation(attribute, ctx, owner, fieldName) {
  const targetUid = relationTargetUid(attribute);
  const multiple = isManyRelation(attribute);

  if (ctx.relationsMode !== 'relation') {
    if (ctx.relationsMode !== 'text') {
      return {
        skip: true,
        reason:
          `relation to '${targetUid}' (this CMS can hold it: re-run with --relations=relation; ` +
          '--relations=text keeps the ids as text instead)',
      };
    }
    return {
      kind: FieldKind.Relation,
      text: true,
      multiple,
      fieldType: multiple ? { Array: [plainText()] } : plainText(),
    };
  }

  const adopt = shouldAdoptRelation(attribute, fieldName, owner, ctx);
  if (!adopt.adopt) return { skip: true, reason: adopt.reason };

  const target = targetUid ? ctx.contentTypeFor(targetUid) : null;
  if (!target) {
    return {
      skip: true,
      reason: `relation target '${targetUid}' is not among the content types being migrated`,
    };
  }

  const kind = target.kind === 'singleType' ? 'single_page' : 'collection';
  // A single page is one item whose identity is its name, so it can only ever hold one reference;
  // the CMS refuses `has_many` against a page, so a stray `collection:` is folded rather than
  // sent to be rejected.
  const hasMany = kind === 'single_page' ? false : multiple;
  const targetRef = { kind, name: target.cmsName };
  const options = { target: targetRef, has_many: hasMany };
  // The other side's name for this relation is exactly what `inverse_name` is for: a heading on
  // the target's screen, and the name a future `?populate=` uses.
  const inverse = inverseNameOf(attribute);
  if (inverse) options.inverse_name = inverse;

  return {
    kind: FieldKind.Relation,
    target: targetRef,
    hasMany,
    fieldType: { Relation: options },
  };
}

/**
 * Turn one content type's attributes into a CMS schema plus a plan the value converter follows.
 *
 * @returns {{ fields: object[], schema: object[], unmapped: {name: string, reason: string}[] }}
 */
export function planContentType(attributes, ctx, owner) {
  const fields = [];
  const unmapped = [];
  for (const [name, attribute] of Object.entries(attributes ?? {})) {
    const mapped = mapAttribute(attribute, ctx, owner, name);
    if (mapped.skip) {
      unmapped.push({ name, reason: mapped.reason });
      continue;
    }
    const schemaField = fieldSchema(name, mapped.fieldType, {
      required: attribute.required === true,
      unique: mapped.unique === true && typeof mapped.fieldType !== 'string' && 'Text' in mapped.fieldType,
    });
    fields.push({ name, ...mapped, schemaField, attribute });
  }
  return { fields, schema: fields.map((field) => field.schemaField), unmapped };
}

/** Whether a field type can be the one line a reference to the item shows as its name. */
export function isTitleEligible(fieldType) {
  const variant = typeof fieldType === 'string' ? fieldType : Object.keys(fieldType)[0];
  return ['Text', 'Slug', 'Markdown', 'Number', 'Boolean', 'Date', 'DateTime', 'TextEnum'].includes(variant);
}

/**
 * Which field names an item, so a reference to it shows a name instead of an id.
 *
 * Strapi v3 has no such marker, so this looks for the names a content type almost always uses
 * for its label and falls back to the first field that reads as one line. Only a collection has
 * one: a single page's identity is its name, and a composite's values live inside the item that
 * uses them.
 *
 * @returns {string|null} the field's name, or null when nothing fits
 */
export function chooseTitleField(schema) {
  const eligible = schema.filter((field) => isTitleEligible(field.field_type));
  const preferred = ['title', 'name', 'label', 'heading', 'headline', 'subject', 'slug'];
  for (const wanted of preferred) {
    const hit = eligible.find((field) => field.name.toLowerCase() === wanted);
    if (hit) return hit.name;
  }
  return eligible[0]?.name ?? null;
}

/** Normalise a v3 date to `YYYY-MM-DD`, or `null` with a complaint. */
function toDate(raw, problems, path) {
  if (typeof raw !== 'string') {
    problems.push(`${path}: '${raw}' is not a date`);
    return null;
  }
  const match = /^(\d{4}-\d{2}-\d{2})/.exec(raw);
  if (!match) {
    problems.push(`${path}: '${raw}' is not an ISO date`);
    return null;
  }
  return match[1];
}

/** Normalise a v3 date-time to RFC 3339, which is what the CMS parses. */
export function toDateTime(raw, problems = [], path = '') {
  if (typeof raw !== 'string') {
    if (path) problems.push(`${path}: '${raw}' is not a date-time`);
    return null;
  }
  // Strapi emits `2021-01-01T00:00:00.000Z`; a date-only or offset-less value is repaired
  // rather than dropped, because "midnight UTC" is the only sensible reading of it.
  let value = raw.trim();
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) value = `${value}T00:00:00Z`;
  else if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?$/.test(value)) value = `${value}Z`;
  if (!/^\d{4}-\d{2}-\d{2}T.*(Z|[+-]\d{2}:\d{2})$/.test(value)) {
    if (path) problems.push(`${path}: '${raw}' is not an RFC 3339 date-time`);
    return null;
  }
  return value;
}

function toStringValue(raw) {
  if (raw === null || raw === undefined) return '';
  if (typeof raw === 'string') return raw;
  if (typeof raw === 'number' || typeof raw === 'boolean') return String(raw);
  return JSON.stringify(raw);
}

/**
 * `/uploads/<file>`, with an optional origin in front of it.
 *
 * That is how a Strapi body refers to an upload - relatively in the common case, absolutely when
 * a provider stores one. The file name is what the upload plugin generated: letters, digits,
 * `_`, `.`, `-` (and `%` from an encoded character).
 */
const UPLOAD_LINK = /(?:https?:\/\/[^\s)"'<>]*?)?\/uploads\/([A-Za-z0-9_.%\-]+)/g;

/**
 * Repoint the `/uploads/…` links a body carries at the CMS's own, stable image address.
 *
 * The bytes are uploaded again under a name the CMS generates, so the URL the body was written
 * with stops existing. `GET /api/images/by-id/{id}` is what `doc/content-api.md` says a
 * hand-written link (in a Markdown body, say) should use: it resolves to whatever that image
 * serves now, so replacing the image later does not break the body that points at it.
 *
 * A link whose file was not migrated is left exactly as it was, and reported: inventing an
 * address for it would be worse than a broken link that still says where it came from.
 */
function rewriteUploadLinks(ctx, text, path, problems) {
  if (typeof ctx.uploadImageId !== 'function' || text === '') return text;
  return text.replace(UPLOAD_LINK, (match, file) => {
    const cmsId = ctx.uploadImageId(file);
    if (cmsId === null || cmsId === undefined) {
      problems.push(`${path}: '/uploads/${file}' was not migrated, so that link was left alone`);
      return match;
    }
    return `/api/images/by-id/${cmsId}`;
  });
}

/**
 * Convert one field's value, following the kind the plan recorded.
 *
 * `path` is the dotted location (`seo.description`, `blocks[2].title`) so a refusal names the
 * place a reader has to look, exactly as the CMS's own `field` refusals do.
 */
function convertField(field, raw, ctx, problems, path) {
  switch (field.kind) {
    case FieldKind.Image: {
      const ids = field.multiple ? asArray(raw) : [raw];
      const mapped = ids
        .map((file) => ctx.imageIdOf(file))
        .filter((id) => id !== null && id !== undefined);
      if (ids.length > 0 && mapped.length < ids.length) {
        problems.push(`${path}: ${ids.length - mapped.length} media reference(s) were not migrated`);
      }
      return field.multiple ? mapped : (mapped[0] ?? null);
    }
    case FieldKind.Relation:
      return field.text ? convertRelationAsText(field, raw) : convertRelation(field, raw, ctx, problems, path);
    case FieldKind.Component:
      return convertComponentValue(field.componentId, field.multiple ? asArray(raw) : [raw], field.multiple, ctx, problems, path);
    case FieldKind.DynamicZone:
      return convertDynamicZone(raw, ctx, problems, path);
    default:
      return convertScalar(field, raw, ctx, problems, path);
  }
}

function asArray(value) {
  if (value === null || value === undefined) return [];
  return Array.isArray(value) ? value : [value];
}

/** The id of a related entry, whether Strapi sent the object or just its id. */
function relationId(entry) {
  if (entry === null || entry === undefined) return null;
  if (typeof entry === 'object') return entry.id ?? null;
  return entry;
}

/** A relation kept only as the ids it held, for a project not ready for real references. */
function convertRelationAsText(field, raw) {
  const texts = asArray(raw)
    .map((entry) => relationId(entry))
    .filter((value) => value !== null)
    .map(String);
  return field.multiple ? texts : (texts[0] ?? '');
}

/**
 * A relation, as the CMS stores it: a set of `{ target, item }` references.
 *
 * The ids are the CMS's own, which only exist once the referenced items have been created - the
 * whole reason a relation migration runs in two passes. Before that pass `relationsReady` is
 * false and the field is written empty, rather than reporting every target as missing.
 */
function convertRelation(field, raw, ctx, problems, path) {
  if (!ctx.relationsReady) return [];
  const target = field.target;
  const references = [];
  for (const entry of asArray(raw)) {
    if (entry === null || entry === undefined) continue;
    if (target.kind === 'single_page') {
      // A page has no id: whether the page was migrated is the whole answer, and the value names
      // it rather than numbering it.
      if (!ctx.pageMigrated(target.name)) {
        problems.push(`${path}: references the single page '${target.name}', which was not migrated`);
        continue;
      }
      references.push({ target: target.name });
      continue;
    }
    const strapiId = typeof entry === 'object' ? entry.id : entry;
    const cmsId = strapiId === null || strapiId === undefined ? null : ctx.itemIdOf(target.name, strapiId);
    if (cmsId === null || cmsId === undefined) {
      problems.push(`${path}: references ${target.name} #${strapiId}, which was not migrated`);
      continue;
    }
    references.push({ target: target.name, item: cmsId });
  }
  // The CMS refuses more than one reference in a single-valued field, so the first is kept and
  // the rest reported rather than losing the whole item to a refusal.
  if (!field.hasMany && references.length > 1) {
    problems.push(`${path}: holds one reference but ${references.length} were given; kept the first`);
    return references.slice(0, 1);
  }
  return references;
}

function convertScalar(field, raw, ctx, problems, path) {
  const fieldType = field.fieldType;
  const variant = typeof fieldType === 'string' ? fieldType : Object.keys(fieldType)[0];
  if (field.jsonText) {
    if (raw === null || raw === undefined) return '';
    return typeof raw === 'string' ? raw : JSON.stringify(raw);
  }
  switch (variant) {
    case 'Text':
    case 'Markdown':
      // A body commonly links its images by hand (`![x](/uploads/abc.png)`, `<img src="/uploads/…">`).
      // Those bytes are uploaded again under a generated name, so the link is repointed at the
      // CMS's stable by-id address, which survives a later replacement of the image.
      return rewriteUploadLinks(ctx, toStringValue(raw), path, problems);
    case 'Slug':
      return toStringValue(raw);
    case 'Number': {
      if (raw === null || raw === undefined || raw === '') return null;
      const value = typeof raw === 'number' ? raw : Number(raw);
      if (!Number.isFinite(value)) {
        problems.push(`${path}: '${raw}' is not a number`);
        return null;
      }
      return value;
    }
    case 'Boolean':
      return typeof raw === 'boolean' ? raw : Boolean(raw);
    case 'Date':
      return raw === null || raw === undefined ? null : toDate(raw, problems, path);
    case 'DateTime':
      return raw === null || raw === undefined ? null : toDateTime(raw, problems, path);
    case 'TextEnum': {
      const allowed = new Set(field.options ?? []);
      const wanted = Array.isArray(raw) ? raw : raw === null || raw === undefined ? [] : [raw];
      const kept = [];
      for (const value of wanted) {
        const text = String(value);
        if (allowed.size === 0 || allowed.has(text)) kept.push(text);
        else problems.push(`${path}: '${text}' is not one of the enumeration's options`);
      }
      return kept;
    }
    default:
      return emptyValue(fieldType);
  }
}

/** A Strapi component row, as a CMS composite value. */
function convertComponentValue(componentId, rows, multiple, ctx, problems, path) {
  const plan = ctx.componentPlan(componentId);
  const converted = rows.map((row, index) => {
    const at = multiple ? `${path}[${index}]` : path;
    if (row === null || row === undefined) return null;
    if (typeof row !== 'object') {
      problems.push(`${at}: expected a component object`);
      return null;
    }
    const { values } = convertEntry(row, plan, ctx, at);
    return compositeValue(componentId, values, ctx);
  });
  if (multiple) return converted.filter((value) => value !== null);
  return converted[0] ?? null;
}

/**
 * The value shape the CMS round-trips for a composite: `{ "id", "values" }`.
 *
 * Writes accept this wrapper back (that is what makes "load, change one field, save" work), and
 * for an array element the `id` is also what tells the parser which of several declared
 * composite types the element is. The one shape it cannot express is a composite that declares a
 * sub-field literally named `values` - then the wrapper is not unwrapped and `id` would be an
 * unknown field, so the bare object is sent instead and a warning records it.
 */
function compositeValue(componentId, values, ctx) {
  if (ctx.componentDeclaresValues(componentId)) {
    ctx.warn(`composite '${componentId}' declares a 'values' sub-field; the element id cannot be carried`);
    return values;
  }
  return { id: componentId, values };
}

/** A v3 dynamic zone, as a CMS array of composites. */
function convertDynamicZone(raw, ctx, problems, path) {
  const elements = asArray(raw);
  const converted = [];
  elements.forEach((element, index) => {
    const at = `${path}[${index}]`;
    if (element === null || typeof element !== 'object') {
      problems.push(`${at}: expected a dynamic zone element`);
      return;
    }
    const strapiComponent = element.__component;
    const componentId = ctx.componentIdOf(strapiComponent);
    if (!componentId) {
      problems.push(`${at}: component '${strapiComponent}' was not migrated`);
      return;
    }
    const plan = ctx.componentPlan(componentId);
    const { values } = convertEntry(element, plan, ctx, at);
    converted.push(compositeValue(componentId, values, ctx));
  });
  return converted;
}

/**
 * Convert one Strapi entry into the object of untagged values the CMS accepts.
 *
 * Every planned field is present, so the editor sees the whole item; `problems` lists anything
 * that could not be carried over, and `missingRequired` lists a required field that ended up
 * empty - which the CMS would refuse, so the caller reports the item as skipped instead.
 */
export function convertEntry(entry, plan, ctx, pathPrefix = '') {
  const values = {};
  const problems = [];
  const missingRequired = [];
  for (const field of plan.fields) {
    const path = pathPrefix ? `${pathPrefix}.${field.name}` : field.name;
    // Before the relation pass the schema does not carry the relation fields yet - a required one
    // cannot be saved empty, so it is added only after the items exist - which means the value
    // must leave the key out entirely rather than send an empty set the schema would refuse as an
    // unknown field. `required` is not judged for it either, for the same reason.
    if (field.kind === FieldKind.Relation && !field.text && !ctx.relationsReady) continue;
    const value = convertField(field, entry?.[field.name], ctx, problems, path);
    values[field.name] = value;
    if (field.schemaField.required && isEmpty(field.fieldType, value)) {
      missingRequired.push(path);
    }
  }
  return { values, problems, missingRequired };
}

/**
 * The dates an entry carried in Strapi, ready for the CMS's metadata endpoint.
 *
 * v3 uses snake_case for timestamps (`created_at`), and a MongoDB-backed project answers
 * `createdAt`; both are read. The CMS refuses an update before the creation, so an inconsistent
 * pair is trimmed to the creation date rather than dropped: the item still gets the date that
 * matters, and nothing is lost by writing the earlier one twice.
 */
export function entryDates(entry, { published = true } = {}) {
  const pick = (...names) => {
    for (const name of names) {
      const value = entry?.[name];
      if (typeof value === 'string' && value.trim() !== '') return value;
    }
    return null;
  };
  const dates = {};
  const created = toDateTime(pick('created_at', 'createdAt'));
  const updated = toDateTime(pick('updated_at', 'updatedAt'));
  const publishedAt = toDateTime(pick('published_at', 'publishedAt'));
  if (created) dates.created_at = created;
  if (updated) dates.updated_at = created && updated < created ? created : updated;
  if (published && publishedAt) dates.published_at = publishedAt;
  return dates;
}

function isEmpty(fieldType, value) {
  if (value === undefined || value === null) return true;
  if (typeof fieldType === 'string') {
    if (fieldType === 'Boolean') return typeof value !== 'boolean';
    if (fieldType === 'Number') return typeof value !== 'number';
    if (fieldType === 'Image') return false;
    return value === '';
  }
  const variant = Object.keys(fieldType)[0];
  if (variant === 'Array' || variant === 'TextEnum' || variant === 'Relation') return value.length === 0;
  if (variant === 'CompositeField') return value === null;
  return isEmpty(variant, value);
}
