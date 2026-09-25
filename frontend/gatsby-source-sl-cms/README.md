# gatsby-source-sl-cms

この CMS(`sl_cms`)の**公開コンテンツ API**(`/api/content/*`)を読む Gatsby の source plugin です。
`doc/content-api.md` の「Gatsby からの使い方」にある薄いプラグインを、実際に使える形にしたものです。

- **認証不要**。配信 API は公開済みだけを返すので、ビルドにトークンは要りません。
- CMS のスキーマから **GraphQL の型を組み立てる**。コレクション `blog` は `SlCmsBlogItem`、
  単一ページ `home` は `SlCmsHomePage`、複合フィールド定義 `block` は `SlCmsCompositeBlock` になります。
- **参照は GraphQL でたどれる**(順方向も逆方向も)。`Relation` フィールドは参照先のノードに
  `@link` されるので、`author { name }` と `author { articles { title } }`(逆引き、
  `inverse_name`)のどちらも 1 つのクエリで辿れます。**複合フィールドの中の参照も同じ**です。
- **Markdown フィールドは `text/markdown` のノード**になるので、`gatsby-transformer-remark` を
  入れるだけで HTML として使えます。**複合フィールドの中の Markdown も同じ**です。
- **画像は `images.download` で `File` ノード**にできるので、`gatsby-transformer-sharp` +
  `gatsby-plugin-image` で `gatsbyImageData` を作れます(§5)。

## 1. 使い方

```console
$ npm install gatsby-transformer-remark
$ npm install ../path/to/frontend/gatsby-source-sl-cms   # リポジトリ内のプラグイン
```

```javascript
// gatsby-config.js
module.exports = {
  plugins: [
    {
      resolve: 'gatsby-source-sl-cms',
      options: {
        // CMS のルート。`/api` は付けない(付けるのはプラグインの仕事)。
        apiUrl: process.env.SL_CMS_URL || 'http://127.0.0.1:8000',
      },
    },
    // Markdown を HTML にする。Tsubame 側は mediaType を付けているだけなので、変換はこの
    // プラグインに任せる(`doc/content-api.md` §7 の分担どおり)。
    'gatsby-transformer-remark',
  ],
}
```

`apiUrl` は CMS のルートです。API は常に `/api` の下にあり、配信 API が返す画像 URL も
`/api/images/...` を含んだままなので、プラグインはそれを `apiUrl` で絶対 URL に直します。
別の接頭辞で配備している場合だけ `apiPrefix` を指定してください。

### オプション

| オプション | 既定 | 意味 |
|---|---|---|
| `apiUrl` | (必須) | CMS のルート。例 `http://127.0.0.1:8000` |
| `apiPrefix` | `/api` | API の接頭辞。別の場所に載せているときだけ |
| `pageSize` | `50` | コレクションを辿るときの 1 リクエストの件数(API の上限は 200) |
| `typePrefix` | `SlCms` | 生成する GraphQL 型名の接頭辞 |
| `requestTimeout` | `30000` | 1 リクエストの制限時間(ms) |
| `retries` | `2` | ネットワーク失敗・5xx/429 の再試行回数 |
| `concurrency` | `4` | スキーマを読むときの同時リクエスト数 |
| `fetchOptions` | `{}` | `fetch` に渡す追加オプション(例: 前段に認証があるときの `headers`) |
| `images.download` | `false` | 画像をダウンロードして `File` ノードを作る(sharp 連携。§5) |
| `images.concurrency` | `4` | 画像を同時にダウンロードする数 |
| `images.requestHeaders` | `{}` | 画像取得時のヘッダ(配信が認証を要求するとき) |

ページは件数ではなく **`next_offset`** で辿ります。配信 API は件数だけでなくバイト数でも
1 ページを切るため、`offset + limit` で進むと取りこぼすことがあるからです(`doc/content-api.md` §3.1)。

## 2. 生成されるノード

| ノード | 型 | 主なフィールド |
|---|---|---|
| コレクション | `SlCmsCollection` | `name` / `itemTypeName` / `itemCount` / `schema` / `fieldNames` |
| アイテム | `SlCms<Collection>Item` | `remoteId` / `collection` / `publishedAt` / `lastPublishedAt` / `values` / `fieldNames` + 各フィールド |
| 単一ページ | `SlCms<Page>Page` | `name` / `publishedAt` / `lastPublishedAt` / `values` / `fieldNames` + 各フィールド |
| Markdown | `SlCmsMarkdown` | `raw` / `field` / `path` / `collection` / `pageName` / `itemId` |
| 複合フィールド | `SlCmsComposite<Id>` | `id` / `values` + 定義の各フィールド |

`values` は API が返した生の値(型タグなし)そのままです。型付きのフィールドはその上に載ります。
`fieldNames` は「CMS の名前 → GraphQL の名前」の対応表です(後述)。

### フィールド型の対応

| CMS の `field_type` | GraphQL |
|---|---|
| `Text` / `Slug` | `String` |
| `Markdown` | `SlCmsMarkdown` (`@link`) |
| `Number` | `Float`(CMS は `f64`) |
| `Boolean` | `Boolean` |
| `Date` / `DateTime` | `Date`(Gatsby の scalar。`formatString` が使える) |
| `Image` | `SlCmsImage` |
| `TextEnum` | `[String]` |
| `Relation`(単一) | 参照先の型 (`@link`) |
| `Relation`(複数) | `[参照先の型]` (`@link`) |
| `CompositeField` | `SlCmsComposite<Id>` |
| `Array`(要素型が 1 つ) | `[要素型]` |
| `Array`(要素型が複数・不明) | `JSON` |
| 未知の型 | `JSON` |

`SlCmsImage` は `id` / `url`(API の値) / `absoluteUrl`(そのまま開ける URL) /
`stableUrl`(差し替えても壊れない `/api/images/by-id/{id}`)を持ちます。
`images.download` を有効にすると `localFile`(`File` ノードへの `@link`)も付きます(§5)。

### 名前の書き換え

GraphQL の名前は `[_A-Za-z][_0-9A-Za-z]*` なので、CMS の名前がそのまま使えないことがあります。

- 型名: `press-releases` → `SlCmsPressReleasesItem`、`お知らせ` → `SlCmsUnnamedItem`
- フィールド名: `published-at` → `published_at`

書き換えは**ビルドごとに同じ**で、対応はノードの `fieldNames`(と `SlCmsCollection.itemTypeName`)に
入ります。衝突した場合は `values` → `values_2` のように連番になります。

## 3. 参照を GraphQL でたどる

`Relation` フィールドは**参照先ノードの id**を持ち、`@link` で解決されます。サイト側で
突き合わせを書く必要はありません。

```graphql
query {
  allSlCmsBlogItem {
    nodes {
      title
      author { name }          # 単一の参照 → 参照先ノード
      editors { name }         # 複数の参照 → ノードの配列
      seo { description og_image { absoluteUrl } }
      blocks {                 # 複合フィールドも型付き
        caption
        text { childMarkdownRemark { html } }
        link { name }          # 複合フィールドの中の参照もたどれる
        children { caption text { childMarkdownRemark { rawMarkdownBody } } }
      }
      values                   # 生の値: 参照は {"target": "...", "item": 7} のまま残る
    }
  }
}
```

- **単一と複数**はスキーマの `has_many` で決まります(単一ページへの参照は定義上いつも単一)。
- **複数の参照は書いた順のまま**。relation は集合ではなく**順序つきリスト**で、CMS は並び順を保存し
  そのまま配信します(同じ参照の重複だけが除かれ、最初の位置に残ります)。プラグインはその順で
  ノードをつなぐので、「注目記事をこの順で」がサイトにそのまま出ます。並べ替えは CMS 側の話で、
  サイトは `values.<フィールド>` でも同じ順を見られます。
- **参照そのもの(相手の名前と id)が要る**ときは `values.<フィールド>` を読みます。配信 API の
  応答そのままです。
- **参照先が未公開**のときは、そのノードが存在しないので `null`(複数なら空配列)になります。
  ビルドは失敗しません。「まだサイトに出ていない相手」は辿れない、という素直な答えです。
- **参照先のコレクションに公開アイテムが無くても**、その型は宣言されます。プラグインが
  `/api/content/collections/{name}`(空でもスキーマは返る)を辿って定義を読むためです。
  未公開の単一ページだけは公開スキーマが無い(404)ので、プラグイン自身のフィールドだけを持つ
  型を宣言します。そのページが公開された次のビルドで、本来のフィールドが現れます。

### 逆引き(`inverse_name`)

relation フィールドに `inverse_name` を書くと、**参照されている側**がその名前で参照元を持ちます。
プラグインはそれを GraphQL のフィールドにするので、カテゴリーから記事へも 1 クエリで辿れます。

```json
{ "name": "author", "field_type": { "Relation": {
  "target": { "kind": "collection", "name": "authors" },
  "has_many": false,
  "inverse_name": "articles"
}}}
```

```graphql
query {
  allSlCmsAuthorsItem {
    nodes {
      name
      articles { title }   # blog.author が inverse_name: "articles" を宣言している
      features { title }   # home.featured_author が "features" を宣言している
    }
  }
}
```

- **向き**: `inverse_name` は**参照している側**のフィールドに書きます。名前は**対象ごとに 1 つ**
  (別のスキーマが同じ対象に同じ名前を付けると CMS が 409 で拒否する)なので、逆引きフィールドは
  **宣言した側の型 1 つの配列**になります(union ではない)。
- **複合フィールドの中の relation は宣言になりません。** 複合定義は複数のコレクションに埋め込まれ
  得るので「誰が参照しているか」が 1 つに決まりません。これは配信 API の
  `?populate=<inverse_name>` と同じ規則で、プラグインもトップレベルの relation フィールドだけを
  宣言として読みます。
- **実装**: 逆引きは、そのビルドが既に読んだ公開コンテンツから**ローカルに索引を組んで**答えます。
  配信 API には `?where=<field>:<value>`(参照で絞る)と `?populate=<inverse_name>`(参照元を展開)
  がありますが、ビルドは全公開アイテムを持っているので、アイテムごとに 1 リクエスト投げるより
  こちらの方が速く、上限(既定 25 件)もありません。判定は API と同じです: **公開コピーが実際に
  その参照を持っているか**で決まり、複合や配列の中の参照も数えます。
- 逆引きフィールドの名前も `fieldNames` に入ります(元の名前 → GraphQL 名)。
- **逆引きに順序はありません。** 参照元は索引から引く集合なので、CMS も並び順を持ちません
  (プラグインはコレクション名→アイテム id の順に並べて、ビルド間で安定させます)。順序に意味が
  あるのは、参照を持つ側の順方向リストです。

## 4. Markdown を HTML にする

Markdown フィールドは `internal.mediaType: "text/markdown"` と `internal.content`(生のソース)を
持つ子ノードとして作られます。`gatsby-transformer-remark` は media type だけで対象を決めて
`internal.content` を読むので、間に入る設定はありません。アイテムのフィールドはそのノードに
`@link` されているため、`childMarkdownRemark` をそのまま辿れます。

```graphql
query {
  allSlCmsBlogItem(sort: { publishedAt: DESC }) {
    nodes {
      title
      slug
      publishedAt
      cover { absoluteUrl }
      body {
        childMarkdownRemark {
          html
          excerpt(format: PLAIN, pruneLength: 120)
          timeToRead
        }
      }
    }
  }
}
```

`createPages` で記事ページを作る:

```javascript
// gatsby-node.js
const path = require('path')

exports.createPages = async ({ graphql, actions }) => {
  const { data, errors } = await graphql(`
    {
      allSlCmsBlogItem {
        nodes {
          slug
        }
      }
    }
  `)
  if (errors) throw errors

  for (const item of data.allSlCmsBlogItem.nodes) {
    actions.createPage({
      path: `/blog/${item.slug}/`,
      component: path.resolve('./src/templates/blog-post.js'),
      context: { slug: item.slug },
    })
  }
}
```

```javascript
// src/templates/blog-post.js
import React from 'react'
import { graphql } from 'gatsby'

export default function BlogPost({ data }) {
  const item = data.slCmsBlogItem
  return (
    <article>
      <h1>{item.title}</h1>
      <div dangerouslySetInnerHTML={{ __html: item.body.childMarkdownRemark.html }} />
    </article>
  )
}

export const query = graphql`
  query BlogPost($slug: String!) {
    slCmsBlogItem(slug: { eq: $slug }) {
      title
      body {
        childMarkdownRemark {
          html
        }
      }
    }
  }
`
```

**複合フィールドの中の Markdown** も同じ仕組みです。定義(`/api/content/composite-fields`)が
公開されたので、どの文字列が Markdown なのかをプラグインが知ることができます。Markdown
ノードは同じアイテムの子で、`path` が場所を示します(`blocks.0.text`、`blocks.0.children.0.text`)。

Markdown が**配列**の要素のとき(`Array: [{"Markdown":{}}]`)は、要素ごとにノードを作って
配列で `@link` します。

## 5. sharp で画像を処理する(`gatsby-transformer-sharp`)

`gatsby-transformer-sharp` は**ローカルの `File` ノード**しか見ません(`internal.mediaType` が
画像で、実体がディスクにあること)。配信 API が返すのは URL なので、そのままでは
`gatsbyImageData` は作れません。`images.download` を有効にすると、プラグインが各画像を
Gatsby のキャッシュに取得し、`File` ノードを作って `SlCmsImage.localFile` からリンクします。

```console
$ npm install gatsby-source-filesystem gatsby-plugin-sharp gatsby-transformer-sharp gatsby-plugin-image
```

```javascript
// gatsby-config.js
module.exports = {
  plugins: [
    // createRemoteFileNode と File 型のために必要(取得元ではないので path は何でもよい)。
    { resolve: 'gatsby-source-filesystem', options: { name: 'unused', path: './src/images' } },
    {
      resolve: 'gatsby-source-sl-cms',
      options: { apiUrl: 'http://127.0.0.1:8000', images: { download: true } },
    },
    'gatsby-plugin-sharp',
    'gatsby-transformer-sharp',
  ],
}
```

```graphql
query {
  slCmsBlogItem(slug: { eq: "hello" }) {
    cover {
      absoluteUrl
      localFile { childImageSharp { gatsbyImageData(width: 800) } }
    }
    gallery { localFile { childImageSharp { gatsbyImageData(width: 400) } } }
    seo { og_image { localFile { childImageSharp { gatsbyImageData } } } }
  }
}
```

`gatsby-plugin-image` の `GatsbyImage` / `getImage` にはその `gatsbyImageData` をそのまま渡せます。
複合フィールドの中の画像も同じです(定義を読んで値の木を歩くので、`seo.og_image` も
`blocks[0].image` も `localFile` を持ちます)。

- **既定は off**。ダウンロードは実際の仕事で、sharp を使わないサイトには不要だからです。
- **同じ画像は 1 回だけ**取得します(参照が何箇所にあっても id でまとめる)。
- **取得に失敗した画像は警告と `localFile: null`**。ビルドは止めません。`url` / `absoluteUrl` は
  そのまま残ります。
- `File` の media type は**ファイルの拡張子**から決まります。API の `url` は
  `/api/images/<file>.<ext>` なのでそのまま取得に使えます(`stableUrl` は拡張子を持たないので
  ダウンロードには使いません)。
- **差し替えにも追随します。** CMS の差し替えは **id を変えずにファイル名(= URL)を変える**ので、
  プラグインはそのビルドで API が返した**現在の `url`** を取得します。次のビルドで `url` /
  `absoluteUrl` / `localFile` / `gatsbyImageData` が新しくなり、`stableUrl`
  (`/api/images/by-id/{id}`)だけが変わりません — 変えないためのリンクだからです。Gatsby の
  キャッシュを残したまま(`--clear-cache` なしで)再ビルドしても更新されます。
- **未変更の画像は取得し直しません。** 同じ実体の `File` ノードが前のビルドのストアにあれば、
  それを再利用し `touchNode` で Gatsby に残します。同一性は URL から**クエリを除いた**
  `origin + pathname`、つまりオブジェクトキーです。この CMS はアップロードごとに新しいファイル名を
  振り、差し替えでは使い回さないので、**同じキー＝同じ実体 / キーが変わった＝差し替え**と言い切れます。
- **署名付き配信(AWS の `AWS_IMAGE_DELIVERY=presigned`)でも再利用できます。** 署名はクエリにあり
  読むたびに変わりますが、パスはオブジェクトキーのままなので判定は影響を受けません。実測では、
  署名だけ変わった再ビルドで**画像の取得は 0 回**、`File` ノードの id も sharp の出力もそのままで、
  差し替えでは 1 回だけ取得して新しい `File` と変換結果になりました。
  - ただし API が返す `url` / `absoluteUrl` は署名ごとに変わります(値そのものなので隠しません)。
    そのため presigned モードでは画像入りのページの `contentDigest` は毎ビルド変わり、ページは
    再生成されます(画像の再変換はありません)。
  - 再利用した `File` ノードの `url` は**前のビルドの署名のまま**です。`localFile` は
    `childImageSharp` / `gatsbyImageData` 経由で使い、URL を直接読まないでください
    (presigned モードは「ビルド時に取得して自前で配信する」ためのモードです)。

## 6. ライブプレビュー(ビルドを経由しない)

公開前の作業コピーを、アカウントを持たない相手に見せるための入口です。共有プレビュー URL
(`doc/content-api.md` §5.6)の仕事で、**このプラグインもビルドも通りません**。契約の全体は
`doc/preview-site.md`、実装は `frontend/sl-cms-preview/`(このプラグインに依存しない、依存ゼロの
パッケージ)です。

**このプラグインは `sl-cms-preview` に依存しません。** サイトが両方をインストールします。
プレビューを Gatsby 以外のサイトへ移しても、このプラグインの依存は増えません。

### 配線

consuming site 側で `gatsby-config.js` / `gatsby-node.js` を環境変数で分け、**本番の成果物に
`/preview/*` を含めない**ようにします。

```javascript
// gatsby-config.js
const preview = process.env.BUILD_MODE === 'preview'

module.exports = {
  plugins: [
    // プレビュービルドは公開コンテンツを一切読みません: 作業コピーはブラウザが API から取ります。
    ...(preview ? [] : [{ resolve: 'gatsby-source-sl-cms', options: { apiUrl: process.env.SL_CMS_URL } }]),
    // Markdown のパイプラインはどちらのビルドでも同じものを使う(下記)。
    ...(preview ? [] : ['gatsby-transformer-remark']),
  ],
}
```

```javascript
// gatsby-node.js
const path = require('path')

exports.createPages = async ({ actions }) => {
  if (process.env.BUILD_MODE === 'preview') {
    // クライアントオンリーのルートを1つだけ。行き先は URL から読みます。
    actions.createPage({
      path: '/preview',
      matchPath: '/preview/*',
      component: path.resolve('./src/preview/preview-page.js'),
    })
    return
  }
  // 本番: これまでどおり全ページを生成する(README §4)。
}
```

`src/preview/preview-page.js` は `sl-cms-preview` で読んで、サイトの表示コンポーネントに渡すだけです。

```javascript
import React, { useEffect, useState } from 'react'
import {
  parsePreviewRoute, fetchPreview, createContentClient, resolvePreview,
} from 'sl-cms-preview'
import { renderMarkdown } from '../markdown'   // ビルドと同じ並び(下記)
import { articleFromPreview } from '../view/article'
import { Article } from '../view/article-view'

export default function PreviewPage({ location }) {
  const [doc, setDoc] = useState(null)
  useEffect(() => {
    const target = parsePreviewRoute(location.pathname)
    const token = new URLSearchParams(location.search).get('token')
    const apiUrl = process.env.GATSBY_SL_CMS_URL
    const client = createContentClient({ apiUrl })
    fetchPreview({ apiUrl, target, token })
      .then(({ schema, values }) =>
        resolvePreview({
          apiUrl, schema, values,
          loadPublished: client.loadPublished,
          loadCompositeSchema: client.loadCompositeSchema,
          renderMarkdown,
        }),
      )
      .then((resolved) => setDoc(articleFromPreview(resolved)))
      .catch((error) => setDoc({ error: error.message }))
  }, [location.pathname, location.search])

  return doc ? <Article doc={doc} /> : null
}
```

### 名前と Markdown

- **フィールド名**: プレビューは CMS の綴り(`published-at`)で返します。ビルドと同じ GraphQL 名が
  要るなら `previewFieldNames(kind, schema)`(`src/preview-adapter.js`)を使ってください。同じ
  planner を使うので、ビルドとずれません。
- **Markdown**: `renderMarkdown` に**本番と同じパイプライン**を渡します。
  `gatsby-transformer-remark` の `html` は `remark.parse` → `mdast-util-to-hast` →
  `hast-util-to-html`(両方 `allowDangerousHtml: true`、既定で gfm と footnotes)です。ファイルや
  ノードを必要とする `gatsby-remark-*` はブラウザでは動かないので、AST だけを触るものに限ります。
- **本番との差**(`localFile`、`excerpt`、逆引き参照など)と、view-model での吸収のしかたは
  `doc/preview-site.md` §5・§6 にあります。

### 置き場所

- **AWS**: 管理画面とは別のサブドメインに置きます(`infra/preview.tf`)。管理画面とオリジンを
  分けるのは、下書きの HTML を管理画面の `localStorage` と同じオリジンで描かないためです
  (`doc/preview-site.md` §7)。
- **Cloudflare Pages / オンプレミス**: 別ホスト名で配り、`PREVIEW_SITE_URL` を CMS に設定します。
  オンプレミスの nginx 例は `doc/preview-site.md` §8 にあります。

## 7. この版でできないこと

配信 API の契約から決まる制限です。

- **下書きは見えない**。意図どおりです。プレビューは共有プレビュー URL(`doc/content-api.md` §5.6)
  の仕事で、ビルドとは別の入口です。
- **公開アイテムが 0 件のコレクションはノードも 0 件**。型は宣言されますが、アイテムは
  ありません(`/api/content/collections` が公開アイテムを持つコレクションしか列挙しないため)。
- **`Date` / `DateTime` は Gatsby の `Date`**。生の文字列は `values` に残ります。
- 複合フィールドの定義 ID が配信 API に無い場合、その値は `SlCmsComposite`(`id` と `values` だけ)
  になります。CMS が `/api/content/composite-fields` より古い場合は警告が出ます。
- **配信 API の `?where=` / `?populate=` はプラグインからは呼びません。** ビルドは全公開
  コンテンツを持っているので、参照の絞り込みも展開もローカルで答えます(理由は §3 の逆引き)。
  実行時に一部だけ読むクライアントは、これらのパラメータを直接使ってください。

## 8. テスト

Gatsby も CMS も要りません。依存パッケージはありません(`fetch` と Node の `node:test` だけ)。

```console
$ scripts/test-gatsby-source.sh
# または
$ cd frontend/gatsby-source-sl-cms && npm test
```

テストは契約(`doc/content-api.md`)の形をした偽の配信 API と、Gatsby が渡すのと同じ引数で
2 つのフックを動かします。ページの辿り方、型の割り当て、複合定義の解決、参照のリンク、
Markdown ノードの生成とリンクまでを見ています。
