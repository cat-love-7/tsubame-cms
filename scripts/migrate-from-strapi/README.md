# Strapi v3 からこの CMS へのデータ移行

`migrate.mjs` は、動いている **Strapi v3**(3.6 系)からコンテンツを読み、この CMS の
`/api/models/*` に書き込む Node スクリプトです。**依存パッケージはありません**(Node 24 の
`fetch` だけを使うので、チェックアウトからそのまま実行できます)。

```console
$ node scripts/migrate-from-strapi/migrate.mjs --help
```

## 1. 前提: v3 にはエクスポートが無い

最初に決まることなので先に書きます。

- **`strapi export` / `strapi import` は v4.6 で追加された機能で、v3 にはありません。**
  v3 の CLI にあるのは `configuration:dump` / `configuration:restore`(どちらも**設定**だけ)までです
  ([v3 CLI](https://docs-v3.strapi.io/developer-docs/latest/developer-resources/cli/CLI.html) /
  [Strapi v4.6 の発表](https://strapi.io/blog/announcing-strapi-v4.6))。
  したがってデータは **v3 の REST API から読みます**。
- **v3 の REST に `/api` の接頭辞はありません。** 既定は空で、`GET http://host:1337/articles` です。
  `/api` が既定になったのは v4 から。接頭辞を変えているプロジェクトは
  `config/middleware.js` の `settings.router.prefix` を `--strapi-prefix` で渡してください。
- **v3 に API トークンはありません**(v4 の機能)。このスクリプトは `POST /auth/local` で
  JWT を取るか、Public ロールで `find` を許可した前提で無認証で読みます。
- **リレーション・メディア・コンポーネントは既定で展開されて返ります。** v3 の REST に
  `populate` パラメータは無く、`?author=1` は「絞り込み」であって展開指定ではありません。
  そのため既定では `_populate` を**送りません**(`autoPopulate` を切ったプロジェクト向けに
  `--populate` だけ用意してあります)。
- **`GET /{plural}/count` は裸の数値**を返します(`{"count":n}` ではありません)。
- **既定の `_publicationState` は `live` で、下書きは返りません。** このスクリプトは既定で
  `preview` を要求し、**Strapi の公開/下書き状態をそのまま引き継ぎます**(後述)。

## 2. 使い方

定義(コンテンツタイプ・コンポーネント)の読み取り元は 2 つあります。どちらか一方が必要です。

- `--strapi-project <dir>`: v3 のチェックアウト。`api/*/models/*.settings.json`、
  `components/**/*.json`、`api/*/config/routes.json` を読みます。**ルートまで分かるのはこちらだけ**
  なので推奨。
- `--strapi-admin-email` / `--strapi-admin-password`(または `--strapi-admin-token`):
  動いているインスタンスの `content-type-builder` 管理 API から読みます。チェックアウトが
  手元に無い場合用ですが、ルートは API 名からの推測になるので必要なら `--route` で直します。

### ローカルの on-premises へ

```console
$ node scripts/migrate-from-strapi/migrate.mjs \
    --strapi-url http://localhost:1337 \
    --strapi-identifier strapi@example.com --strapi-password '…' \
    --strapi-project /path/to/strapi-v3 \
    --relations relation \
    --cms-url http://127.0.0.1:8080 \
    --cms-username admin@example.com --cms-password '…'
```

### AWS デプロイへ

AWS 側のサインインは Cognito なので `/api/auth/login` が使えません(501 を返します)。
管理画面から取得したトークンを `--cms-token` で渡してください。画像は S3 の署名付き URL に
直接 PUT されます(そのとき CMS のトークンは付けません。署名が credential のため)。

```console
$ node scripts/migrate-from-strapi/migrate.mjs … \
    --cms-url https://… --cms-token 'eyJ…'
```

### まず何が起きるか見る

`--dry-run` は **CMS に一切触らず**、Strapi を読んで計画と変換後の値を表示します。
リレーションの値も、参照 1 つなら `{ target, item }`、複数ならその配列として表示されます。

```console
$ node scripts/migrate-from-strapi/migrate.mjs \
    --strapi-url http://localhost:1337 --strapi-identifier … --strapi-password … \
    --strapi-project /path/to/strapi-v3 --relations relation --dry-run
```

## 3. 型の対応

| Strapi v3 | この CMS | 備考 |
|---|---|---|
| `string` / `text` | `Text` | `required` と `maxLength` / `minLength` を引き継ぐ |
| `richtext` | `Markdown` | |
| `uid` | `Slug` | CMS 側で正規化される(`Hello World` → `hello-world`) |
| `email` | `Text` | |
| `integer` / `biginteger` / `float` / `decimal` | `Number` | |
| `boolean` | `Boolean` | |
| `date` | `Date` | `YYYY-MM-DD` |
| `datetime` / `timestamp` | `DateTime` | RFC 3339 に正規化(オフセット欠落は `Z` を補う) |
| `time` | `Text` | 時刻型が無いため文字列として保持 |
| `enumeration` | `TextEnum` | 単一値なので**1 要素の配列**になる |
| `json` | `Text` | 型が無いので JSON 文字列として保持 |
| component(`repeatable: false`) | `CompositeField` | コンポーネントを複合フィールド定義として作成 |
| component(`repeatable: true`) | `Array([CompositeField])` | |
| dynamiczone | `Array([CompositeField, …])` | 要素ごとに `{"id": "<定義>", "values": {…}}` を書く |
| media(単数) | `Image` | 実体をアップロードし、CMS の画像 id を参照 |
| media(複数) | `Array([Image])` | |
| relation(単数) | `Relation` | `--relations=relation` のとき。後述 |
| relation(複数) | `Array([Relation])` | 同上。要素が参照 1 つ分 |
| `password` | (移さない) | 資格情報はコンテンツではない |
| `localizations` などのプラグイン項目 | (移さない) | 警告に出る |

`unique: true` は CMS では **Text のみ**意味を持つので、それ以外では落として警告します
(`Slug` は型として一意なので重複指定も落とします)。

## 4. リレーション(`--relations=relation`)

CMS は relations を持てるようになったので(`docs/relations-design.md`)、Strapi の
`oneToOne` / `oneToMany` / `manyToOne` / `manyToMany` / one-way を取り込めます。

```console
$ node scripts/migrate-from-strapi/migrate.mjs … --relations relation
```

| モード | 動き |
|---|---|
| `skip`(既定) | リレーションを落とし、**1 件ずつ警告**する |
| `text` | 相手の id を `Text`(複数なら `Array: [Text]`)として残す。CMS の参照にしたくない場合の安全策 |
| `relation` | 本物の参照として書く。**2 パス**で走り、公開は依存順になる |

`skip` が既定なのは、移行前に「まず本文だけ」を通せるようにするためです。警告には
`--relations=relation` で移せる旨が出ます。

### どう畳まれるか

- **片面だけ採用します。** Strapi は関係を**両側**に書きますが、この CMS は片側に定義し、
  逆側はインデックスから答えます。両方を移すと同じリンクが 2 重に入るため、どちらか一方だけを
  採用し、もう一方は警告して落とします。
  - 既定では、**単一参照を持つ側**を採用します(`oneToMany` / `manyToOne` の対なら「多」側。
    `docs/relations-design.md` の「多側に単一参照を置けば oneToMany」と同じ形)。
  - many-to-many のように**両側が集合**のときは、**API 名の順で決まる片側**を採用します
    (毎回同じ選択になるので、再実行しても安定します)。
  - **どちらを所有者にするかは指定できます**: `--relation-owner <apiId>.<field>`(後述)。
- **カーディナリティは参照 1 つか、その `Array` かの 2 種類**に畳まれます(`{ model: … }` は
  `Relation`、`{ collection: … }` は `Array([Relation])`。`has_many` はありません)。
- **単一ページも参照先になれます**(`{ model: 'home' }` → `{ kind: "single_page", name: "home" }`)。
  ページは id を持たないので、値は参照 1 つの `{ "target": "home" }` になります。
- **`inverse_name` に `via`(= 相手側での呼び名)を入れます。** 相手の画面の見出しと、
  いずれ来る `?populate=` に使われます。データは持ちません。
- **順序は移しません。** Strapi 側で並び順に意味がある場合はその旨を警告します
  (必要なら別フィールドで表現してください)。
- **移行できない参照先**(users-permissions の `user` など、移行対象でない
  コンテンツタイプ)は警告して落とします。

### どちら側が所有者かを指定する

片面だけ持てる以上、「どちらに持たせるか」はサイトの読み方の問題です。たとえば
`post.category` を持つのか `category.posts` を持つのかで、配信 API から引ける向きが変わります
(現状の配信 API は `?populate=` / `?where=` がまだ無いので、なおさら効きます)。

まず、どんなリレーションがあって**既定でどちら側が採用されるか**を確認します。

```console
$ node scripts/migrate-from-strapi/migrate.mjs \
    --strapi-url http://localhost:1337 \
    --strapi-admin-email … --strapi-admin-password … \
    --relations relation --list-relations
```

```
== relations ==
mutual (2) - the CMS keeps one side; the other is derived from its index
  field                         card    field                         card    kept by default
  category.posts                many    post.category                 single  post.category
  page.page_binder              single  page-binder.pages             many    page.page_binder
  keep the other side with:
    --relation-owner category.posts
    --relation-owner page-binder.pages

one-way (2) - the only side there is
  homepage.categories                     many    -> category
  links.page-binder-link.page_binder      single  -> page-binder  (inside a composite)
```

- **`mutual`**: 両側に定義があるもの。`kept by default` が既定で採用される側で、下の
  `--relation-owner` が「反対側を採用させる」ための指定です。
- **`one-way`**: 片側しか無いもの(複合定義の中の relation もここに出ます)。
- **`dropped`**: 移行対象に無いコンテンツタイプ(users-permissions の `user` など)を指すもの。

向きを変えるときはこうします。

```console
$ node scripts/migrate-from-strapi/migrate.mjs … --relations relation \
    --relation-owner category.posts     # 既定では post.category が持つところを逆にする
```

- 指定は `<apiId>.<フィールド名>`、繰り返し可(`--relation-owner a.b --relation-owner c.d`)。
- 指定した側が採用され、**もう一方は警告して落ちます**(理由に `--relation-owner` と出ます)。
- 片側しか持てないので、**1 つの関係の両側を指定するとエラー**になります。存在しない
  フィールド名もエラーです(指定したつもりで自動判定のまま、という事故を防ぐため)。
- 計画のログに `relation owner chosen for: …` と、どの関係を指定したかが出ます。

### 2 パスと公開順

参照は**この CMS が採番した id** を指すので、対象のアイテムが出来てからでないと書けません。
また CMS は **`required` な relation が空のままでは保存できず**、**未公開の相手しか無い状態では
公開を拒否**します。そこで次の順で進みます。

1. すべてのコレクション/単一ページを **relation フィールド抜き**で作る
2. すべてのアイテムを作る(relation はまだ値に含めない)
3. スキーマに **relation フィールドを追加する**(アイテムが出来た後なので、
   必須 relation が空で保存できない問題を踏まない)
4. **2 周目**で参照を書く
5. **依存順に公開する**(参照される側を先に)。互いに必須で参照し合っている組は
   どの順でも公開できないので、警告して試行し、拒否は失敗として報告します
6. 元の日時を書く

このため、**relation を移す場合の所要時間と Strapi への読み取りは 2 倍**になります
(2 周目はソースを読み直します。全アイテムをメモリに抱えないためです)。

## 5. 引き継げるもの / 引き継げないもの

**引き継ぎます。**

- **公開/下書きの状態。** Strapi で `published_at` が入っていたエントリだけを publish し、
  下書きは下書きのまま作ります(`--publish-all` / `--no-publish` で上書き)。
- **日時**。`PUT …/items/{id}/metadata` / `PUT …/single_pages/{name}/item/metadata` で
  `created_at` / `updated_at` / `published_at` を元の値にします。公開は `published_at` を
  「今」にするので、**作成 → 公開 → 日時**の順で走ります(API の推奨順でもあります)。
  画像の `uploaded_at` も `PUT /models/images/{id}` で戻します。
  日時が要らないときは `--no-dates`。
- **リレーション**(`--relations=relation` のとき)。上記のとおり。
- **配列経由で相互再帰するコンポーネント**(`a` が `b` の配列を持ち、`b` が `a` の配列を持つ)。
  CMS はどちらも**保存できます**が、**一発では入りません**。定義は参照先が存在しないと保存できず、
  相互だとどちらを先にも書けないためです。そこで**ブートストラップ**します: 片方を参照抜きで
  書き、もう片方を完全な形で書き、最後に最初のものを完全な形で書き直します。移行ツールは
  これを自動で行います(自己再帰 `tree` → `tree` の配列は CMS が保存中の定義を存在扱いするので
  一発で入ります)。直接の循環(`a → b → a`、配列を通らない)だけは CMS が拒否します。
- **アイテムの名前になるフィールド**。参照は相手の「名前」を表示するので、
  CMS の `is_title` を、`title` → `name` → `label` → … → 最初の1行で読めるフィールド、
  の順で推定して立てます(そのフィールドは一覧の列にも出します)。推定させたくないときは
  `--no-title-field`。
- **画像のサムネイル**(ライブラリとピッカーがタイルに使う小さいコピー)。この CMS の
  サムネイルは通常**アップロードしたブラウザが作ります**が(`docs/content-api.md` §5.9)、
  移行では**Strapi が作った `formats.thumbnail` のバイト列**を取得して
  `PUT /models/images/{id}/thumbnail?ext=…` に送ります。Strapi がサムネイルを作っていない
  ファイル(PDF、SVG、小さい画像など)は送りません。その場合タイルは原本を表示します
  (API から直接上げた画像と同じ扱いで、壊れはしませんが重くなります)。

**引き継げません。**

- **コンテンツのロケール。** この CMS にコンテンツの i18n はありません。1 ロケールだけ移すなら
  `--locale ja`、全ロケールを 1 つのコレクションにまとめるなら `--locale all`、
  ロケールごとに別コレクションにするなら `--locale ja --name article=article_ja` のように
  組み合わせてください。
- **直接の循環**(`a → b → a` で配列を通らないもの)。CMS は拒否します。ただしこれは
  **Strapi 側でも実質扱えない形**なので、移行で失うものではありません。v3 のバックエンドに
  循環を拒否する検査は無く、モデルファイルを手で書けば読み込めますが、
  `populateComponent` が訪問済み集合を持たずにコンポーネントを辿るため、そのモデルを配信する
  API は終わりません(循環コンポーネントで Strapi が落ちる報告があります:
  [#16089](https://github.com/strapi/strapi/issues/16089) /
  [#22293](https://github.com/strapi/strapi/issues/22293))。移行ツールは、手で編集されたモデルに
  これを見つけた場合、**警告してそのフィールドだけ落とし、実行は続けます**。
- `json` の中身は文字列になります。`time` は文字列になります。
- **参照の並び順**。Strapi 側のリレーションに順序は無いため、移行後の並びは移行ツールが読んだ順になります
  (CMS 側の値は順序を持つ配列で、その順序はそのまま保存・配信されます)。

## 6. 再実行・中断・レポート

- 進捗は `.strapi-migration/state.json`(既定)に **Strapi の id → CMS の id** の対応として
  書かれます。CMS 側は id を採番し直すため、対応表が無いと再実行で重複します。
- **再実行は安全です。** 作成済みのコンポーネント・スキーマは作り直し(PUT)になり、画像と
  エントリは対応表にあるものを飛ばします。参照と日時は毎回書き直すので、途中で落ちた実行も
  そのままもう一度流せば収束します(公開済みのものを含めて publish し直すため
  `last_published_at` は動きますが、`published_at` は元の日時で上書きされます)。
  対応表から再利用した画像(`already`)には触らないので、**サムネイル対応より前に上げた画像に
  サムネイルを付けたいときは `--force`** で上げ直してください。
- 最初からやり直すときは `--force`(または state ファイルを消す)。
- 終了時に `.strapi-migration/report.json` に計画・型対応・失敗・警告を書きます。1 件でも
  失敗があると終了コードは 1 です。
- `--max-items` / `--max-images` で試し移行、`--only` / `--skip` で対象を絞れます。

## 7. 検証

```console
$ scripts/test-migration.sh          # 単体テスト(ネットワーク不要)
```

実サーバーを相手にした end-to-end は、このリポジトリの CMS と同梱のスタブで再現できます。

```console
# 1. CMS を空のデータで起動
$ DATA_ROOT=/tmp/cms JWT_SECRET=0123456789012345678901234567890123456789 \
    ADMIN_USERNAME=admin@example.com ADMIN_PASSWORD=admin-password \
    ./backend/target/debug/tsubame &

# 2. Strapi v3 の代わりにスタブを起動(ポート 1337)
$ node scripts/migrate-from-strapi/test/fake-strapi-v3.mjs --port 1337 &

# 3. 移行(リレーション込み)
$ cd scripts/migrate-from-strapi
$ node migrate.mjs --strapi-url http://127.0.0.1:1337 \
    --strapi-identifier strapi@example.com --strapi-password strapi-password \
    --strapi-project test/fixtures/strapi-v3 --relations relation \
    --cms-url http://127.0.0.1:8080 \
    --cms-username admin@example.com --cms-password admin-password

# 4. 結果を見る(公開済みだけが返る)
$ curl -s http://127.0.0.1:8080/api/content/collections/article
$ curl -s http://127.0.0.1:8080/api/content/collections/category
```

スタブは本物の v3 の形(リストは裸の配列、`/count` は裸の数値、`_publicationState` の絞り込み、
管理 API の `{data:[…]}` とメディア/リレーションの正規化)を返すので、ソース側のクライアントも
ここで検証されます。fixture は次を含みます。

- 3 件目の記事は `title` が必須なのに空で、**失敗として報告され、他は移る**
- `article.category` ↔ `category.articles`: **所有側だけが採用される**(one-to-many)
- `article.tags` ↔ `tag.articles`: **many-to-many の片側だけが採用される**
- `category.section` は **必須**で、`category` は `section` より先に並ぶ API 名なので、
  **依存順に公開しないと失敗する**(公開順の検証になる)
- `article.featuredOn` → 単一ページ、`category.parent` → 自己参照
- `article.tree` → `blog.tree` ↔ `blog.branch` が**配列経由で相互再帰**しており、
  ブートストラップと、値の再帰変換の両方を検証する
- メディア 2 件のうち 1 件だけが `formats.thumbnail` を持ち、**それだけがサムネイル付きで
  入る**(移行後に `GET /api/models/images` の `thumbnail_url` で確かめられる)
- 直接循環するコンポーネントは単体テストで扱い、実行が止まらずフィールドだけ落ちることも
  `--no-items` の一時プロジェクトで確認している

## 8. 主なフラグ

`--help` が正です。よく使うものだけ:

| フラグ | 意味 |
|---|---|
| `--strapi-url` / `--strapi-prefix` | v3 の場所(接頭辞の既定は空) |
| `--strapi-jwt` / `--strapi-identifier` + `--strapi-password` | v3 の認証 |
| `--strapi-project` | 定義を読むチェックアウト |
| `--strapi-admin-email` + `--strapi-admin-password` | 定義を管理 API から読む |
| `--publication-state live\|preview` | 既定 `preview`(下書きも読む) |
| `--locale <code\|all>` | 対象ロケール |
| `--route <apiId>=<path>` | 読み取りパスの上書き(定義を管理 API から読んだときに必要になりやすい) |
| `--name <apiId>=<name>` | CMS 側のコレクション名を変える |
| `--relations skip\|text\|relation` | リレーションの扱い(既定 skip) |
| `--relation-owner <apiId>.<field>` | 相互リレーションのどちら側を移すか(繰り返し可) |
| `--list-relations` | リレーション一覧と既定の採用側を出して終了(何も書き込まない) |
| `--publish-all` / `--no-publish` | 公開状態の上書き |
| `--no-dates` | 日時を復元しない |
| `--no-title-field` | `is_title` を推定して立てない |
| `--no-schema` / `--no-images` / `--no-items` | 工程ごとのスキップ |
| `--dry-run` / `--sample <n>` | 書き込まずに確認 |
| `--state` / `--report` / `--force` | 対応表とレポートの場所、やり直し |
| `--concurrency` / `--page-size` | 並列度と 1 ページの件数 |

## 9. 注意

- 移行前に **CMS 側を空にしておく**のが安全です。スキーマは作り直し(PUT)になり、
  既存アイテムはそのまま残ります。
- relation の移行は **フィールドを後からスキーマに足します**。途中で止まった場合、
  スキーマに relation が入っていてアイテムに値が入っていない状態になり得ますが、
  もう一度流せば埋まります。
- Strapi 側は**読み取り専用**で触りません。
- 画像はバイト列を取得して CMS に上げ直すため、`MAX_IMAGE_BYTES`(既定 10MB)を超える
  ファイルは拒否され、失敗として報告されます。
- サムネイルも同じく取得して送るため、CMS の上限(`MAX_THUMBNAIL_BYTES`、512KiB)を超えると
  413 で拒否されます。そのときは**警告して原本だけを残します**(Strapi の既定サイズなら
  通常起きません)。
- 管理者権限の資格情報を使います(`publish` とスキーマ作成に必要)。使い捨ての環境で
  実行し、終わったらパスワードを変えるのが無難です。
