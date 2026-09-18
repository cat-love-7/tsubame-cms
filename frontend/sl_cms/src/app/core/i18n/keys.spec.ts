import en from '../../../assets/i18n/en.json';
import ja from '../../../assets/i18n/ja.json';

/**
 * The keys the code asks for, against the keys the catalogs hold.
 *
 * Key parity between the two languages is not enough on its own: a typo in a template leaves both
 * catalogs complete and the screen showing `content.previewLnks`. This walks the templates and
 * components and fails on any key that is not in the catalogs.
 *
 * The sources are read through Vite's glob rather than Node's file APIs, because the unit tests
 * run in a browser-like environment (see `src/test-providers.ts` for the other half of the test
 * environment).
 */
const sources = (
  import.meta as unknown as {
    glob(pattern: string, options: Record<string, unknown>): Record<string, string>;
  }
).glob('../../**/*.{html,ts}', { query: '?raw', import: 'default', eager: true });

/** Just the code that ships: not the specs, and not this file. */
const CODE = Object.entries(sources).filter(
  ([path]) => !path.endsWith('.spec.ts') && !path.endsWith('keys.spec.ts'),
);

/**
 * Every key an expression names.
 *
 * A `transloco` pipe reads the expression in front of it, which may be a ternary naming two keys;
 * so the text between the start of the binding (`{{`, `="`) and the pipe is what gets scanned.
 */
function keysInPipeExpressions(source: string): string[] {
  const keys: string[] = [];
  const pipe = /\|\s*transloco\b/g;
  let match: RegExpExecArray | null;
  while ((match = pipe.exec(source)) !== null) {
    const start = Math.max(
      source.lastIndexOf('{{', match.index),
      source.lastIndexOf('="', match.index),
      source.lastIndexOf('>', match.index),
    );
    const expression = source.slice(start + 1, match.index);
    keys.push(...[...expression.matchAll(/'([A-Za-z][\w]*(?:\.[\w]+)+)'/g)].map((m) => m[1]));
  }
  return keys;
}

/** Keys named in TypeScript: `t('…')`, `translate('…')`, `failure('…')`. */
function keysInCalls(source: string): string[] {
  return [
    ...source.matchAll(/\b(?:t|failure|apiMessage|translate)\(\s*'([A-Za-z][\w]*(?:\.[\w]+)+)'/g),
  ].map((match) => match[1]);
}

function keyExists(tree: Record<string, unknown>, key: string): boolean {
  return (
    key
      .split('.')
      .reduce<unknown>(
        (node, part) =>
          node && typeof node === 'object' ? (node as Record<string, unknown>)[part] : undefined,
        tree,
      ) !== undefined
  );
}

describe('translation keys used by the code', () => {
  const catalogs: Record<string, Record<string, unknown>> = {
    en: en,
    ja: ja,
  };

  const used = new Map<string, string[]>();
  for (const [path, source] of CODE) {
    const keys = path.endsWith('.html')
      ? keysInPipeExpressions(source)
      : [...keysInCalls(source), ...keysInPipeExpressions(source)];
    for (const key of keys) {
      used.set(key, [...(used.get(key) ?? []), path]);
    }
  }

  it('finds some, or this test is not looking where the keys are', () => {
    // A guard against the glob silently matching nothing and passing forever after.
    expect(used.size).toBeGreaterThan(50);
  });

  it('names only keys the catalogs hold', () => {
    const unknown = [...used.entries()]
      .filter(([key]) => !keyExists(catalogs['en'], key))
      .map(([key, paths]) => `${key} (${[...new Set(paths)].join(', ')})`);

    expect(unknown, 'keys used by the code but missing from en.json').toEqual([]);
  });

  it('names only keys the Japanese catalog holds as well', () => {
    const unknown = [...used.keys()].filter((key) => !keyExists(catalogs['ja'], key));

    expect(unknown, 'keys used by the code but missing from ja.json').toEqual([]);
  });
});
