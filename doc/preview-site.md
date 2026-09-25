# プレビューサイトとの契約

署名付きプレビューリンク(`doc/content-api.md` §5.6)を、**アカウントを持たない相手が読めるページ**に
するための取り決めです。CMS 側の API と、サイト側が実装する `/preview/*` の間のインターフェースを
定義します。Gatsby 固有の話は §8 だけで、それ以外は任意のフレームワーク(Next.js、素の SPA、
サーバサイドレンダリング)に当てはまります。

実装は `packages/tsubame-preview/`(依存ゼロの npm パッケージ)にあります。

## 1. 全体の流れ

```
管理画面            POST /api/models/.../preview-link   →  { path: "/api/preview/...?token=...", expires_at }
                   (スキーマが preview を許可しているときだけ。既定は無効 - §7)
                   link.path を preview_site_url の下へ写してコピー
レビュアー          GET  {preview_site_url}/preview/...?token=...   ← プレビューサイト(静的)
プレビューサイト    GET  {cms}/api/preview/...?token=...            ← 認証不要。{schema, values}
                   GET  {cms}/api/content/...                       ← 公開済みの関連先・複合定義だけ
                   サイトのコンポーネントで描画
```

**ビルドを経由しません。** プレビューは公開のたびに再ビルドする必要がなく、`gatsby-source-sl-cms`
のような source plugin も要りません。公開コンテンツの取得は一切行わず、レビュアーが開いた瞬間に
ブラウザから API を読みます。

## 2. URL の形

| | 形 |
|---|---|
| API が発行するパス | `/api/preview/collections/{c}/items/{id}?token=…` |
| | `/api/preview/single_pages/{p}?token=…` |
| レビュアーが開く URL | `{preview_site_url}` + API パスから `/api` を除いたもの |

つまり `https://preview.example.com/preview/collections/blog/items/7?token=…` です。

- **`preview_site_url` は origin だけ**(scheme・host・port)。ベースパスは許しません。ルートと
  ベースパスの境目をどちらのスラッシュで繋ぐかを決めなくて済むようにするためです
  (`sl_cms::config::parse_preview_site_url`)。
- **`token` はそのまま運びます。** トークンは資格情報そのもので、サイトはそれを API に返すだけです。
- 綴りは API のままです。プレビュー API は `single_pages`(アンダースコア)、**配信** API は
  `single-pages`(ハイフン)です。取り違えないよう、両方とも `sl-cms-preview` の `routes.js` に
  だけ書いてあります。

`sl-cms-preview` の `parsePreviewRoute(pathname)` が自分の URL から行き先を読み、
`previewSiteUrl(apiPath, origin)` が逆を行います(管理画面側が使う関数です)。

## 3. API が返すもの

`{ schema, values }` です。**値に型タグはありません**。どの値が Markdown で、どれが画像で、どれが
参照なのかは、隣にある `schema` の `field_type` だけが言います(`doc/content-api.md` §3)。

| `field_type` の綴り | 意味 |
|---|---|
| `"Number"` `"Boolean"` `"Date"` `"DateTime"` `"Image"` | オプションを持たないので素の文字列 |
| `{"Text":{}}` `{"Slug":{}}` `{"Markdown":{}}` `{"CompositeField":{"id":"seo"}}` `{"Relation":{…}}` `{"Array":[…]}` `{"TextEnum":[…]} ` | オプションを持つので1キーのオブジェクト |

参照の値は `{"target":"authors","item":7}`(`item` が無ければ単一ページ)です。

## 4. 解決後の形 (resolved values)

`resolvePreview()` が返す形です。**フレームワーク非依存**を意図して、プレーンなオブジェクト・
文字列・配列だけで構成し、フィールド名は **CMS の綴りのまま(snake_case)** にしています。

| フィールド型 | 解決後 | 備考 |
|---|---|---|
| `Text` / `Slug` | `string \| null` | |
| `Number` | `number \| null` | |
| `Boolean` | `boolean \| null` | |
| `Date` / `DateTime` | `string \| null` | **ISO 文字列のまま**。整形はサイトの仕事 |
| `TextEnum` | `string[]` | |
| `Markdown` | `{ raw, html }` | `html` は注入した `renderMarkdown` の結果。無ければ `null` |
| `Image` | `{ id, url, absoluteUrl, stableUrl }` | ダウンロードはしない。`localFile` は無い |
| `Relation`(単一) | 解決済みオブジェクト \| `null` | 未公開・該当なしは `null` |
| `Relation`(複数) | 配列 \| `[]` | **未公開の要素は `null` で位置を保つ** |
| `CompositeField` | `{ id, values, …定義の各フィールド }` | 定義が読めなければ `{id, values}` のまま |
| `Array`(要素型が1つ) | 要素ごとに解決した配列 | |
| `Array`(要素型が混在・不明) | API が返した生の JSON | 型タグが無いので要素ごとに決められない |
| 未知の型 | API が返した生の値 | |

規約が3つあります。

- **参照は1ホップだけ既定で辿ります**(`relationDepth`、既定 `1`)。相手を1回 fetch して中身を
  埋めますが、その相手がさらに持つ参照は `null` のままです。相互参照が無限に続かないようにする
  ための予算で、`relationDepth: 2` で2ホップになります。**公開済みの相手だけ**を解決し、未公開は
  `null` です。
- **Markdown は読み出し時に HTML 化します。** ビルドが無いので `gatsby-transformer-remark` の
  ノードは存在しません。レンダラは**注入**します(`renderMarkdown`)。本番と同じパイプラインを
  渡せば見た目が一致し、渡さなければ `raw` だけが残ります。レンダラが例外を投げてもプレビューは
  白紙にならず、`html: null` と `onProblem` の通知になります。
- **失敗しても黙って欠けません。** 複合定義や参照先の読み込みに失敗した場合は `onProblem` に
  渡り、その部分だけが未解決のまま残ります。

## 5. 本番のビルドとの差(重要)

「本番と同じテンプレートで描く」ためには、**同じ形に正規化する**必要があります。プレビューが
原理的に埋められないものを先に列挙します。

| 本番(GraphQL) | プレビュー | サイト側の対処 |
|---|---|---|
| `cover { gatsbyImageData }` | `cover { absoluteUrl, stableUrl }` | 画像を union で受ける(§6) |
| `cover { localFile }` | **無い** | `gatsby-transformer-sharp` はプレビューで動かない |
| `body { childMarkdownRemark { html } }` | `body { html, raw }` | サイトが同じ remark パイプラインを注入 |
| `excerpt` / `timeToRead` / `headings` / `tableOfContents` | **無い** | 本番テンプレートのうち、これらを必須にする部分は分岐が要る |
| 逆引き `articles { … }` | **無い** | プレビューの schema は参照元の `inverse_name` を持たない |
| `Date` scalar | ISO 文字列 | view-model 側で揃える |
| `fieldNames` による GraphQL 名 | CMS の綴りのまま | `previewFieldNames()` (§8) |
| `@link` された参照ノード | 解決済みのオブジェクト | 同じ view-model に写す |

**Markdown の忠実性について。** `gatsby-transformer-remark` の `html` は
`remark.parse` → `mdast-util-to-hast` → `hast-util-to-html`(いずれも `allowDangerousHtml: true`)で、
既定で `remark-gfm` と `remark-footnotes` が有効です。同じ HTML を得るには、**同じライブラリの
同じ版**で同じ並びを組む必要があります。`remark-parse` → `remark-rehype` → `rehype-stringify` では
一致しません(raw HTML の通り方と脚注の綴りが違います)。また `gatsby-transformer-remark` の
`plugins` は unified プラグインではなく Gatsby の文脈(`getNode`、`files`)を受け取る関数なので、
**ファイルやノードを必要とする `gatsby-remark-*` はブラウザでは動きません**。AST だけを触るものは
移植できます。README の参照実装をそのまま使うのが安全です。

## 6. canonical presentation shape(推奨パターン)

本番とプレビューでコンポーネントを共有するには、**どちらでもない第3の形**を1つ決めます。これを
そのサイトの canonical shape と呼びます。コンポーネントはこの形だけを知ります。

```ts
// src/view/article.ts
export type ImageView =
  | { kind: 'sharp'; alt: string | null; source: IGatsbyImageData }  // 本番のみ
  | { kind: 'url'; alt: string | null; src: string };                // プレビュー

export interface ArticleView {
  title: string;
  slug: string;
  publishedAt: string | null;      // ISO 文字列に揃える
  bodyHtml: string | null;
  cover: ImageView | null;
  author: { name: string; slug: string } | null;
  blocks: BlockView[];
}

// 本番: GraphQL の結果から
export function articleFromGraphQL(node: SlCmsBlogItem): ArticleView { … }

// プレビュー: sl-cms-preview の resolved values から
export function articleFromPreview(resolved: Record<string, unknown>): ArticleView { … }

// コンポーネントは view-model だけを受け取る
export function Article({ doc }: { doc: ArticleView }) { … }
```

- **画像は union で吸収します。** これは「プレビューでは `localFile` が null」という制約を型で
  表したものです。`getImage()` を無条件に呼ぶテンプレートはプレビューで落ちるので、`kind` で
  分岐します。
- **埋まらないフィールドは型で任意にします。** `excerpt` を必須にしたままだと、プレビュー用に
  別テンプレートを書く羽目になり、共有した意味が無くなります。
- **プレビューが使う `gatsby-config.js` の remark 設定を、ビルドと1つのモジュールから渡します。**
  2箇所に書けば必ずずれます。
- **検証は1本のテストで足ります。** 同じ内容の fixture を `articleFromGraphQL` と
  `articleFromPreview` の両方に通し、同じ `ArticleView` になることを assert します。これが
  「本番と同じテンプレート」の唯一の保証です。

## 7. セキュリティ

プレビューは**ビルドを経ておらず、本番サイトとは別のオリジン**に置きます。理由は管理画面の
トークンです。

- 管理画面の JWT は `localStorage` にあり、コード自身が「このオリジンのスクリプトから読める」と
  明記しています(`frontend/src/app/core/auth/auth.service.ts`)。管理画面は CMS の HTML を
  一切描画しないので、今のところ同じオリジンに XSS の入口はありません。**プレビューは下書きの
  Markdown を HTML として描画する**ので、同じオリジンに置けばそこが入口になります(本番サイトは
  別オリジンなので、公開済みの XSS が管理画面に届きません)。
- **別サブドメインはオリジン境界です。** パスを分けただけ(`/preview`)では `localStorage` を
  共有するので、境界になりません。
- **同じバケットを共有するなら、オリジン側で拒否します。** プレビューの実体を管理画面の
  バケットの `preview/` に置くと、オブジェクト自体は**管理画面のディストリビューションからも
  読めます**(拡張子付きのパスは書き換えずそのまま配られるため)。だから管理画面側のルーティング
  関数(`infra/app-routing.js`)が `/preview` と `/preview/*` を 404 で拒否します。これが無いと、
  プレビューのスクリプトが管理画面のオリジンで配られ、上の分離は「プレビューのアプリが知らない
  パスでは描画しない」という偶然に依存することになります。別バケットに分ければ、この拒否は
  要りません(その代わりリソースが増えます)。
- プレビューも `X-Robots-Tag: noindex` の下に置きます(`infra/no-index.js` は既に
  ディストリビューション全体に効きます)。トークン付きの URL が検索結果に出ないことは重要です。
- **`Referrer-Policy: no-referrer` を付けます。** トークンはクエリにあり、第三者リソースへの
  Referer で漏れます。
- **CSP は防波堤で、境界ではありません。** `script-src 'self'`(インライン禁止)は、Markdown に
  混入した `<script>` が `localStorage` を読む経路をほぼ塞ぎますが、per-document でしか効かず、
  将来 `unsafe-inline` が必要になれば破れます。別オリジンならこの依存が消えます。
- **トークンは資格情報です。** 期限は `PREVIEW_LINK_TTL_MINUTES`(既定60分)。CloudFront・S3・
  nginx のアクセスログにはクエリが残りうるので、ログの保持にも気を配ってください。

### スキーマごとの許可(既定は無効)

プレビューサイトが在ることと、**どのスキーマをプレビューしてよいか**は別の話です。スキーマ
(コレクション・単一ページ)ごとに `preview` を持ち、**既定は無効**。スキーマ編集画面の
「プレビューリンクを許可する」を入れて保存したものだけがリンクを発行できます(要管理者。
`GET`/`PUT /api/models/collections/{name}/settings`、`/api/models/single_pages/{name}/settings`)。

- 許可の無いスキーマへの発行も、そのリンクを開くことも **403 `preview_disabled`**。開く側でも
  見るのは、**設定を切った瞬間に発行済みのリンクが死ぬ**ようにするためです(§7 の「失効させる」
  手段がこれで、`JWT_SECRET` を回さずに済みます)。
- 発行できるのは `can_edit` を持つ人、許可を変えられるのは管理者だけです。編集のできない相手に
  プレビューを配る導線を作らないため、管理画面は**許可が無いスキーマではボタン自体を出しません**。
- 設定はスキーマ定義とは別のレコードです(`collection_settings` / `single_page_settings`、
  DynamoDB は `settings`)。フィールドの保存が設定を書き戻すことはなく、コレクション・ページを
  消せば設定も消えます。**既存のデプロイは設定を持たないので、これまで配っていたコレクションも
  一度は管理者が入れ直すまで発行できません。**

## 8. 配備

### AWS(サブドメイン)

管理画面とは**別の CloudFront ディストリビューション**をプレビュー用サブドメインに立てます
(`infra/preview.tf`)。バケットは同じものを使い、オリジンに `origin_path = "/preview"` を付けます。
同じバケットなので管理画面側からも `preview/` のオブジェクトは読めるため、管理画面の
ディストリビューションは `infra/app-routing.js` で `/preview/*` を 404 にします(§7)。
証明書は管理画面のものがサブドメインを覆っていれば使い回せます(`*.example.com` のワイルドカード、
または `preview.example.com` を含む SAN)。`PREVIEW_SITE_URL` が Lambda に渡され、
`/auth/capabilities` が `preview_site_url` として配ります。別オリジンになるので
`CORS_ALLOWED_ORIGINS` にもプレビューの origin が要ります(Terraform が自動で入れます)。

### オンプレミス

管理画面を配信しているのと同じ nginx に、プレビュー用の `server`(別ホスト名)を足します。
管理画面側の `location` はそのままです。

```nginx
# プレビューサイト: 別ホスト名なので、管理画面の localStorage とは別オリジンになる。
server {
    listen 443 ssl;
    server_name preview.cms.example.com;

    root /var/www/sl-cms-preview;

    # トークンがクエリにあるので、Referer で漏らさない。
    add_header Referrer-Policy "no-referrer" always;
    add_header X-Robots-Tag "noindex, nofollow" always;
    add_header Content-Security-Policy "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' https://cms.example.com; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'" always;

    # 拡張子の無いパスはサイト自身のルートに落とす(クライアントサイドルーティング)。
    location / {
        try_files $uri /index.html;
    }
}
```

そして `PREVIEW_SITE_URL=https://preview.cms.example.com` を CMS に設定します。設定しなければ
`/auth/capabilities` に `preview_site_url` が出ず、管理画面は共有の代わりに「プレビューサイトが
未設定」と表示します(生の JSON URL はコピーしません)。

### ビルド

本番ビルドとプレビュービルドを `BUILD_MODE` などで分け、**本番の成果物に `/preview/*` を含めない**
でください。プレビュービルドは `gatsby-source-sl-cms` を読み込まず、クライアントオンリーの
ルートだけを登録します。例は `packages/gatsby-source-tsubame/README.md` にあります。

## 9. 関連

- `packages/tsubame-preview/README.md` — パッケージの API と使用例
- `packages/gatsby-source-tsubame/README.md` — Gatsby での配線と `previewFieldNames()`
- `doc/content-api.md` §5.6 — プレビューリンクを発行する側の API
