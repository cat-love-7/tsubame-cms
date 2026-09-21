// The decisions that do not need a network or a running server: how a name is made safe, what
// order components have to be created in, whether an entry is published, and how a media file's
// extension is worked out. Kept apart from the orchestration so they can be tested directly.

import path from 'node:path';

import { sanitizeComponentId } from './definitions.mjs';

/** A collection or page name that survives being a URL path segment. */
export function sanitizeName(name) {
  return String(name).replace(/[/\\?#%]/g, '-');
}

/** Whether an attribute declares a single component. */
export function isComponentReference(attribute) {
  return attribute?.type === 'component' && typeof attribute.component === 'string';
}

/**
 * Whether an attribute is a component field **without** the array around it.
 *
 * The distinction decides whether a reference can close a cycle the CMS refuses: a repeatable
 * component is stored as an array, and the CMS's cycle check stops at arrays.
 */
function isDirectComponentReference(attribute) {
  return isComponentReference(attribute) && attribute.repeatable !== true;
}

/** Every component id an attribute refers to, whichever spelling it uses. */
export function referencedComponentIds(attribute) {
  if (isComponentReference(attribute)) return [sanitizeComponentId(attribute.component)];
  if (attribute?.type === 'dynamiczone') {
    return (attribute.components ?? []).map(sanitizeComponentId);
  }
  return [];
}

/**
 * Plan the order the component definitions have to be written in.
 *
 * A definition cannot name a composite that does not exist yet, so a reference is an ordering
 * edge. Three shapes have to be told apart:
 *
 *   * a component that reaches **itself** through an array needs no special treatment at all -
 *     the CMS counts the definition being saved as already existing, and its cycle check stops at
 *     arrays (the editor draws an array's elements from the stored value, so an empty array draws
 *     nothing);
 *   * a **pair** that reaches each other through an array is storable too, but neither can be
 *     written first *in full*. It is bootstrapped: write one with the not-yet-existing reference
 *     left out, write the other in full, then write the first in full again;
 *   * a **direct** cycle - every step a plain component field, no array in between - is refused
 *     outright (`validate_no_composite_cycles` walks exactly those edges), and a direct
 *     self-reference with it. Such a component can never be written, so it is reported and every
 *     field that names it is dropped. Strapi does not offer the shape either: v3 has no check
 *     that rejects the model files, but its `populateComponent` recurses through components with
 *     no visited set, so an API that served one would not terminate.
 *
 * @param {{id: string, attributes: object}[]} components
 * @returns {{ order: string[], bootstrap: string[], unmigratable: string[] }}
 *   `order` is written in full in dependency order; each id in `bootstrap` is written with its
 *   unavailable references dropped and then written again in full; `unmigratable` is never
 *   written, and the caller drops the fields that named it.
 */
export function orderComponents(components) {
  const ids = new Set(components.map((component) => component.id));
  const directReferences = new Map();
  const allReferences = new Map();
  for (const component of components) {
    const direct = new Set();
    const all = new Set();
    for (const attribute of Object.values(component.attributes)) {
      for (const id of referencedComponentIds(attribute)) {
        if (!ids.has(id)) continue;
        all.add(id);
        // A plain component field is a direct edge (a self-reference here is a cycle the CMS
        // refuses); a repeatable component, an array or a dynamic zone is not, because the cycle
        // check stops at arrays.
        if (isDirectComponentReference(attribute)) direct.add(id);
      }
    }
    directReferences.set(component.id, direct);
    // A self-reference through an array orders nothing and is allowed, so it is not an edge.
    allReferences.set(component.id, new Set([...all].filter((id) => id !== component.id)));
  }

  const unmigratable = new Set(
    [...ids].filter((id) => reachesItself(directReferences, id)),
  );

  // An edge into a component that can never be written is not an ordering edge either: the field
  // that makes it is dropped when the schema is built, so its owner can still be written.
  const references = new Map(
    [...allReferences].map(([id, targets]) => [
      id,
      new Set([...targets].filter((target) => !unmigratable.has(target))),
    ]),
  );

  const order = [];
  const remaining = new Map([...references].map(([id, targets]) => [id, new Set(targets)]));
  for (;;) {
    const ready = [...remaining].filter(([, targets]) => targets.size === 0).map(([id]) => id);
    if (ready.length === 0) break;
    for (const id of ready) {
      order.push(id);
      remaining.delete(id);
    }
    for (const targets of remaining.values()) {
      for (const id of ready) targets.delete(id);
    }
  }
  return {
    order: order.filter((id) => !unmigratable.has(id)),
    bootstrap: [...remaining.keys()].filter((id) => !unmigratable.has(id)).sort(),
    unmigratable: [...unmigratable].sort(),
  };
}

/** Whether `start` can be reached again from its own direct references. */
function reachesItself(graph, start) {
  const stack = [...(graph.get(start) ?? [])];
  const seen = new Set();
  while (stack.length > 0) {
    const current = stack.pop();
    if (current === start) return true;
    if (seen.has(current)) continue;
    seen.add(current);
    stack.push(...(graph.get(current) ?? []));
  }
  return false;
}

/**
 * A component's schema with every reference to a composite that does not exist yet left out.
 *
 * Used only to bootstrap a mutually recursive pair: a field naming something that is not there
 * cannot be saved, so it is dropped for that one write and put back by the second. An array left
 * with no item types is dropped with it, because the CMS refuses an array that declares none.
 */
export function withoutUnavailableReferences(schema, available) {
  const kept = [];
  for (const field of schema) {
    const type = field.field_type;
    if (typeof type !== 'object' || type === null) {
      kept.push(field);
      continue;
    }
    if (type.CompositeField) {
      if (available.has(type.CompositeField.id)) kept.push(field);
      continue;
    }
    if (Array.isArray(type.Array)) {
      const items = type.Array.filter((item) =>
        item?.CompositeField ? available.has(item.CompositeField.id) : true,
      );
      if (items.length > 0) kept.push({ ...field, field_type: { Array: items } });
      continue;
    }
    kept.push(field);
  }
  return kept;
}

/**
 * Whether an entry should be published in the CMS.
 *
 * Strapi's draft/published model is the same one the CMS has, so the state travels with the
 * content: v3 stamps `published_at` on an entry that is live and leaves it null on a draft, and
 * an entry Strapi never published arrives here as a draft rather than being published by the
 * move. `--publish-all` and `--no-publish` override that.
 *
 * A content type that does not enable Strapi's draft/publish plugin has **no draft state at
 * all**: `published_at` is not even a field, and every entry is what the site serves. v3's
 * default is off (`_.get(model, 'options.draftAndPublish', false)`), so reading "no
 * `published_at`" as "a draft" would leave such a collection invisible.
 */
export function shouldPublish(entry, options, draftAndPublish = false) {
  if (options.noPublish) return false;
  if (options.publishAll) return true;
  if (draftAndPublish !== true) return true;
  return Boolean(entry?.published_at ?? entry?.publishedAt);
}

/** The extension to hand the CMS, from Strapi's `ext`, `mime` or the URL. */
export function fileExtension(file) {
  const fromExt = String(file.ext ?? '').replace(/^\./, '').trim();
  if (fromExt) return fromExt.toLowerCase();
  const fromName = path.extname(String(file.name ?? file.hash ?? '')).replace(/^\./, '');
  if (fromName) return fromName.toLowerCase();
  const fromUrl = path.extname(String(file.url ?? '').split('?')[0]).replace(/^\./, '');
  if (fromUrl) return fromUrl.toLowerCase();
  const fromMime = String(file.mime ?? '').split('/')[1];
  return fromMime ? fromMime.toLowerCase() : '';
}

/** The display name to give the upload, with the extension Strapi kept separate reattached. */
export function displayFilename(file, ext) {
  const name = String(file.name ?? file.hash ?? `file-${file.id}`);
  return ext && !name.toLowerCase().endsWith(`.${ext}`) ? `${name}.${ext}` : name;
}

/**
 * Order items so that each comes after everything it depends on.
 *
 * Used to publish in dependency order: the CMS refuses to publish an item whose `required`
 * relation would end up with no *published* target, so a referenced item has to be released
 * first. Only required relations become edges - an optional reference never blocks a publish -
 * and an edge to something outside `keys` is ignored, because nothing in this run can release it.
 *
 * A cycle is reported rather than hidden: `a` requiring `b` while `b` requires `a` cannot be
 * published in any order, and the caller has to say so instead of looping. (`order` still holds
 * every item that could be ordered; the cyclic ones are listed separately.)
 *
 * @param {string[]} keys
 * @param {Map<string, Set<string>>} dependencies  key -> the keys that must be published first.
 * @returns {{ order: string[], cyclic: string[] }}
 */
export function orderForPublish(keys, dependencies) {
  const wanted = new Set(keys);
  const pending = new Map();
  for (const key of keys) {
    const deps = [...(dependencies.get(key) ?? [])].filter((dep) => dep !== key && wanted.has(dep));
    pending.set(key, new Set(deps));
  }

  const order = [];
  for (;;) {
    const ready = [...pending].filter(([, deps]) => deps.size === 0).map(([key]) => key);
    if (ready.length === 0) break;
    for (const key of ready) {
      order.push(key);
      pending.delete(key);
    }
    for (const deps of pending.values()) {
      for (const key of ready) deps.delete(key);
    }
  }
  return { order, cyclic: [...pending.keys()].sort() };
}
