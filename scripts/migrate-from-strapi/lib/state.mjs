// The run's memory: what has already been written, so a migration that stopped can be run
// again without duplicating content, plus the failures and warnings the report is built from.
//
// The CMS assigns its own item ids, so the only way to know that Strapi entry 12 became CMS item
// 3 is to write it down. The file is rewritten after every content type, so an interrupted run
// loses at most the work in flight.

import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';

const EMPTY = {
  version: 1,
  startedAt: null,
  composites: {},
  schemas: {},
  images: {},
  items: {},
  singlePages: {},
  failures: [],
  warnings: [],
};

export class MigrationState {
  constructor(file, data = {}) {
    this.file = file;
    this.data = {
      ...structuredClone(EMPTY),
      ...data,
      // The maps and lists survive a JSON round trip, but a hand-edited file might not have them.
      composites: { ...(data.composites ?? {}) },
      schemas: { ...(data.schemas ?? {}) },
      images: { ...(data.images ?? {}) },
      items: { ...(data.items ?? {}) },
      singlePages: { ...(data.singlePages ?? {}) },
      failures: [...(data.failures ?? [])],
      warnings: [...(data.warnings ?? [])],
    };
  }

  static async load(file) {
    try {
      const data = JSON.parse(await readFile(file, 'utf8'));
      return new MigrationState(file, data);
    } catch (error) {
      if (error.code === 'ENOENT') return new MigrationState(file, {});
      throw new Error(`cannot read the state file ${file}: ${error.message}`);
    }
  }

  async save() {
    // Written to a neighbour and renamed, so a kill in the middle cannot leave a truncated file
    // that the next run would refuse to read. The first save of a run is also what creates the
    // directory the state and the report live in.
    await mkdir(path.dirname(this.file), { recursive: true });
    const temporary = `${this.file}.tmp`;
    await writeFile(temporary, `${JSON.stringify(this.data, null, 2)}\n`, 'utf8');
    await rename(temporary, this.file);
  }

  start(cmsUrl, strapiUrl) {
    this.data.startedAt ??= new Date().toISOString();
    this.data.strapi = strapiUrl;
    this.data.cms = cmsUrl;
  }

  hasComposite(id) {
    return Boolean(this.data.composites[id]);
  }

  markComposite(id, outcome) {
    this.data.composites[id] = outcome;
  }

  hasSchema(name) {
    return Boolean(this.data.schemas[name]);
  }

  markSchema(name, outcome) {
    this.data.schemas[name] = outcome;
  }

  imageId(strapiId) {
    const id = this.data.images[String(strapiId)];
    return id === undefined ? null : id;
  }

  markImage(strapiId, cmsId) {
    this.data.images[String(strapiId)] = cmsId;
  }

  itemId(collection, strapiId) {
    const id = this.data.items[collection]?.[String(strapiId)];
    return id === undefined ? null : id;
  }

  markItem(collection, strapiId, cmsId) {
    this.data.items[collection] ??= {};
    this.data.items[collection][String(strapiId)] = cmsId;
  }

  markSinglePage(name, outcome) {
    this.data.singlePages[name] = outcome;
  }

  get singlePages() {
    return this.data.singlePages;
  }

  fail(entry) {
    // Re-running a migration retries what failed last time; recording the same failure twice
    // would make the report read as if twice as much had gone wrong.
    const key = (failure) =>
      `${failure.scope}|${failure.contentType ?? ''}|${failure.strapiId ?? ''}|${failure.name ?? ''}`;
    const existing = this.data.failures.findIndex((failure) => key(failure) === key(entry));
    const record = { at: new Date().toISOString(), ...entry };
    if (existing === -1) this.data.failures.push(record);
    else this.data.failures[existing] = record;
  }

  warn(entry) {
    this.data.warnings.push(entry);
  }

  summary() {
    const items = Object.values(this.data.items).reduce(
      (total, collection) => total + Object.keys(collection).length,
      0,
    );
    return {
      composites: Object.keys(this.data.composites).length,
      schemas: Object.keys(this.data.schemas).length,
      images: Object.keys(this.data.images).length,
      items,
      singlePages: Object.keys(this.data.singlePages).length,
      failures: this.data.failures.length,
      warnings: this.data.warnings.length,
    };
  }
}
