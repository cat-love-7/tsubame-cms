// A miniature Tsubame delivery API, for the one test that runs a real Gatsby build.
//
// The unit tests in `test/` fake `fetch`; a build cannot - the plugin is loaded by Gatsby, in
// Gatsby's process - so the build test needs a server. This one answers the same shapes
// `docs/content-api.md` promises, and nothing else: three collections, one item each, and one
// relation target (`nowhere`) that is in no list and answers 404, so an element of a multi-target
// array has a target this build has no node type for.

import { createServer } from 'node:http';

const field = (name, field_type, extra = {}) => ({
  name,
  field_type,
  required: false,
  width: 12,
  height: 1,
  ...extra,
});

const TEXT = (name, extra) => field(name, { Text: {} }, extra);

export const BLOG_SCHEMA = [
  TEXT('title', { required: true, is_title: true, show_in_list: true }),
  field('related', {
    Array: [
      { Relation: { target: { kind: 'collection', name: 'authors' } } },
      { Relation: { target: { kind: 'collection', name: 'editors' } } },
    ],
  }),
  field('mentions', {
    Array: [
      { Relation: { target: { kind: 'collection', name: 'authors' } } },
      { Relation: { target: { kind: 'collection', name: 'nowhere' } } },
    ],
  }),
  field('card', { CompositeField: { id: 'card' } }),
];

// A composite definition holds a relation array too, and it is not a node type: the union's field
// resolver is attached to the composite's own type, which is the second place it has to work.
export const COMPOSITE_FIELDS = {
  card: [
    TEXT('label'),
    field('related', {
      Array: [
        { Relation: { target: { kind: 'collection', name: 'authors' } } },
        { Relation: { target: { kind: 'collection', name: 'editors' } } },
      ],
    }),
  ],
};

const AUTHORS_SCHEMA = [TEXT('name', { required: true, is_title: true })];
const EDITORS_SCHEMA = [TEXT('name', { required: true, is_title: true })];

const BLOG_ITEM = {
  id: 1,
  published_at: '2026-01-01T00:00:00Z',
  last_published_at: '2026-01-02T00:00:00Z',
  values: {
    title: 'Hello',
    related: [
      { target: 'authors', item: 7 },
      { target: 'editors', item: 2 },
    ],
    mentions: [
      { target: 'authors', item: 7 },
      { target: 'nowhere', item: 5 },
    ],
    card: {
      id: 'card',
      values: {
        label: 'Card',
        related: [
          { target: 'authors', item: 7 },
          { target: 'editors', item: 2 },
        ],
      },
    },
  },
};

const COLLECTIONS = new Map([
  ['blog', { schema: BLOG_SCHEMA, items: [BLOG_ITEM] }],
  ['authors', { schema: AUTHORS_SCHEMA, items: [{ id: 7, values: { name: 'Ada' } }] }],
  ['editors', { schema: EDITORS_SCHEMA, items: [{ id: 2, values: { name: 'Grace' } }] }],
]);

const send = (response, status, body) => {
  const payload = Buffer.from(JSON.stringify(body));
  response.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': payload.length });
  response.end(payload);
};

/**
 * Start the stub on a free port.
 *
 * @returns {Promise<{url: string, close: () => Promise<void>}>} the API's root, and how to stop it
 */
export async function startDeliveryStub() {
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://localhost');
    const path = url.pathname.replace(/^\/api/, '');

    if (path === '/content/collections') {
      return send(response, 200, [...COLLECTIONS.keys()]);
    }
    if (path === '/content/single-pages') {
      return send(response, 200, []);
    }
    if (path === '/content/composite-fields') {
      return send(response, 200, COMPOSITE_FIELDS);
    }
    const collection = path.match(/^\/content\/collections\/([^/]+)$/);
    if (collection !== null) {
      const name = decodeURIComponent(collection[1]);
      const found = COLLECTIONS.get(name);
      // `nowhere` is not in any list and answers 404: the plugin reads that as "no node type for
      // this target in this build", which is the case a relation union's reference member is for.
      if (found === undefined) {
        return send(response, 404, { code: 'not_found', message: `no collection '${name}'` });
      }
      const limit = Number(url.searchParams.get('limit') ?? 50);
      return send(response, 200, {
        schema: found.schema,
        items: found.items,
        total: found.items.length,
        limit,
        offset: 0,
        next_offset: null,
      });
    }

    return send(response, 404, { code: 'not_found', message: `no route for '${path}'` });
  });

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();

  return {
    url: `http://127.0.0.1:${port}`,
    close: () =>
      new Promise((resolve) => {
        server.closeAllConnections?.();
        server.close(resolve);
      }),
  };
}
