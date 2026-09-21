# sl-cms-preview

`sl_cms` の**署名付きプレビューリンク**を、ビルドを経由せずにページとして描画するための
小さなパッケージです。**Gatsby にも React にも依存しません**(依存パッケージはゼロ、
`fetch` と普通のオブジェクトだけ)。

契約の全体は `doc/preview-site.md` にあります。この README は使い方と API だけです。

## 何をするか

```
レビュアーが開く           https://preview.example.com/preview/collections/blog/items/7?token=…
プレビューサイトが読む     GET https://cms.example.com/api/preview/collections/blog/items/7?token=…  → {schema, values}
関連先(公開済みだけ)を読む GET https://cms.example.com/api/content/authors/items/9
複合フィールド定義を読む   GET https://cms.example.com/api/content/composite-fields
```

1. `parsePreviewRoute(location.pathname)` で行き先を読む
2. `fetchPreview({ target, token })` で作業コピーを取る
3. `resolvePreview({ schema, values, renderMarkdown, ... })` でページが読める形にする
4. サイト自身のコンポーネントで描く

```console
$ npm install ../path/to/frontend/sl-cms-preview
```

## 使い方(フレームワーク非依存)

```js
import {
  parsePreviewRoute,
  fetchPreview,
  createContentClient,
  resolvePreview,
} from 'sl-cms-preview';

const target = parsePreviewRoute(window.location.pathname); // {kind:'collection', collection:'blog', id:7}
const token = new URLSearchParams(window.location.search).get('token');

const apiUrl = 'https://cms.example.com';
const client = createContentClient({ apiUrl });

const { schema, values } = await fetchPreview({ apiUrl, target, token });

const resolved = await resolvePreview({
  apiUrl,
  schema,
  values,
  loadPublished: client.loadPublished,
  loadCompositeSchema: client.loadCompositeSchema,
  // 本番と同じパイプラインを渡す(推奨)。渡さなければ Markdown は raw のまま。
  renderMarkdown: (raw) => myMarkdownPipeline(raw),
});
```

同じオリジンに API があるなら `apiUrl` は `''` で構いません(パスは `/api/...` のまま)。

## API

### どこにプレビューがあるか

| 関数 | 返すもの |
|---|---|
| `previewApiPath(target, apiPrefix?)` | API のパス（`/api/preview/collections/blog/items/7`） |
| `previewRoutePath(target)` | プレビューサイトのルート（`/preview/collections/blog/items/7`） |
| `parsePreviewRoute(pathname)` | サイトの URL が指す `target`、または `null`。ベースパスは問わない |
| `previewSiteUrl(apiPath, siteOrigin, apiPrefix?)` | 管理画面がコピーする URL（トークンごと運ぶ） |
| `checkTarget(target)` | 不正ならその説明、正しければ `null` |

`target` は `{ kind: 'collection', collection, id }` か `{ kind: 'single_page', page }` です。

### CMS を読む

| 関数 | 返すもの |
|---|---|
| `fetchPreview({ apiUrl, target, token, apiPrefix?, fetchImpl? })` | `{schema, values}`。拒否は `status` 付きの `Error` |
| `createContentClient({ apiUrl, apiPrefix?, fetchImpl? })` | `{ loadPublished, loadCompositeSchema }`。404 は `null` |
| `joinUrl(base, path)` | 素朴な URL 連結 |

### 値を解決する

`resolvePreview(options) → Promise<resolved>`。オプションは `doc/preview-site.md` §4 の表のとおりです。

| オプション | 既定 | 意味 |
|---|---|---|
| `schema` / `values` | (必須) | `fetchPreview` が返したもの |
| `loadPublished` | `async () => null` | 公開済みの関連先を読む。`null` は「未公開」 |
| `loadCompositeSchema` | `async () => null` | 複合フィールド定義を読む |
| `renderMarkdown` | なし | `(raw) => string \| Promise<string>`。**本番と同じものを渡す** |
| `apiUrl` / `apiPrefix` | `''` / `'/api'` | 画像の `absoluteUrl` / `stableUrl` の組み立て |
| `relationDepth` | `1` | 参照を何ホップまで辿るか。`0` なら参照は `null` |
| `onProblem` | なし | `{path, message}` を受け取る。描画を止めずに知らせる |

## 本番と一致させるために

- **Markdown**: `renderMarkdown` に本番と同じパイプラインを渡してください。
  `gatsby-transformer-remark` の `html` は `remark.parse` → `mdast-util-to-hast` →
  `hast-util-to-html`(両方 `allowDangerousHtml: true`、既定で gfm と footnotes)です。
  `remark-parse` → `remark-rehype` → `rehype-stringify` では一致しません。
- **画像**: ダウンロードしません。`absoluteUrl` を使ってください(§4)。
- **足りないもの**: `localFile` / `gatsbyImageData`、`excerpt` / `timeToRead` / `headings` /
  `tableOfContents`、逆引き参照。サイト側の view-model で吸収します(`doc/preview-site.md` §6)。

## Gatsby サイトの場合

`frontend/gatsby-source-sl-cms` の `previewFieldNames()` を使うと、ビルドと同じ GraphQL 名
(`published-at` → `published_at`)が得られます。アダプタはそれだけで、残りは view-model の仕事です。
詳しくは `gatsby-source-sl-cms/README.md` の「ライブプレビュー」節を参照してください。

## テスト

```console
$ npm test
# または
$ scripts/test-preview.sh
```

ネットワークも CMS も Gatsby も要りません。`fetch` は注入でき、配信 API は
`doc/content-api.md` の形をした偽の応答で置き換えています。
