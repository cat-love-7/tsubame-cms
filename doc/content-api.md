# 公開コンテンツ API と下書き / 公開

Gatsby などの静的サイトビルドが CMS の内容を読むための契約。
管理画面が使う `/models/*` とは別に、**認証不要・公開済みのみ**を返す `/content/*` を用意している。

## 1. なぜ `/models/*` と分けているか

| | `/models/*` | `/content/*` |
|---|---|---|
| 認証 | Bearer トークン必須 | 不要 |
| 返す内容 | 下書きを含む全件 | 公開済みのみ |
| 主な利用者 | 管理画面 | サイトビルド・配信 |
| スキーマ | 別リクエストで取得 | レスポンスに同梱 |

`/models/*` はスキーマも下書きも含むため公開できない。そこで読み取り専用の入口を分けた。
値は型タグを持たない(スキーマが唯一の型情報)ので、`/content/*` はスキーマを同時に返す。
これにより、値の解釈のためだけに認証付き API を叩く必要がない。

## 2. 下書き / 公開のモデル

**値は 2 つある。** 編集者が保存するのは**作業コピー**(draft)で、配信 API が読むのは
**公開コピー**(published)だけ。`publish` は作業コピーを公開コピーへコピーする操作で、
`unpublish` は状態を下げるだけでどちらのコピーも消さない。

| 保存先 | 中身 | 誰が見るか |
|---|---|---|
| アイテムストア | 公開コピー | `/content/*`(サイト) |
| 作業コピー用ストア | 作業コピー(未公開の変更) | `/models/*` の読み書き(管理画面・プレビュー) |

- **保存はライブサイトを変えない。** 公開コピーが置き換わるのは `publish` のときだけ。
  これが「編集はできるが公開はできない」ロールを安全にしている。
- 作業コピーが無い = 保留中の変更が無い。`publish` は公開コピーを作業コピーで置き換え、
  作業コピーを消す(何も無ければ `published_at` を更新するだけ)。
- 状態・日時は値とは**別のストア**(`item_metadata`)に入る。スキーマに `status` や
  `published_at` というフィールドがあっても衝突しない。
- コレクション / 単一ページ / アイテムを削除すると、作業コピーとメタデータも一緒に消える。
- 未公開の内容への `/content/*` は **404**(403 ではない)。存在自体を漏らさないため。
- `published_at` は publish 時に記録し、unpublish すると `null` に戻る(公開日時は保持しない)。
- 管理 API のメタデータは `has_draft` を返す。公開済み + `has_draft: true` が
  「公開中だが、未公開の変更がある」状態で、管理画面はこれを「変更あり」と表示する。

## 3. エンドポイント

### 公開(認証不要)

| メソッド | パス | 返すもの |
|---|---|---|
| GET | `/content/collections` | 公開アイテムを1つ以上持つコレクション名の配列 |
| GET | `/content/collections/{name}` | `{ "schema": [...], "items": [...], "total": 12, "limit": 50, "offset": 0, "next_offset": 50 }` |
| GET | `/content/collections/{name}/items/{id}` | `{ "id": 1, "published_at": "...", "values": {...} }` |
| GET | `/content/single-pages` | 公開済み単一ページ名の配列 |
| GET | `/content/single-pages/{name}` | `{ "schema": [...], "published_at": "...", "values": {...} }` |

### 管理(要トークン。`publish` / `unpublish` は編集権限が必要)

| メソッド | パス | 返すもの |
|---|---|---|
| GET | `/models/collections/{name}/items` | `[[id, values], ...]` + `X-Total-Count` ヘッダ |
| GET | `/models/collections/{name}/items/metadata` | `{ "1": { "status": "draft", "published_at": null, "created_at": "...", "updated_at": "...", "has_draft": false }, ... }` |
| GET | `/models/collections/{name}/items/{id}/metadata` | そのアイテムのメタデータ |
| POST | `/models/collections/{name}/items/{id}/publish` | 更新後のメタデータ(存在しない id は 404) |
| POST | `/models/collections/{name}/items/{id}/unpublish` | 更新後のメタデータ |
| GET | `/models/single_pages/{name}/item/metadata` | そのページのメタデータ |
| POST | `/models/single_pages/{name}/publish` | 更新後のメタデータ |
| POST | `/models/single_pages/{name}/unpublish` | 更新後のメタデータ |
| GET | `/models/collections/{name}/items/{id}/preview` | `{ "schema": [...], "id": 1, "values": {...} }`(作業コピー。要トークン) |
| GET | `/models/single_pages/{name}/preview` | `{ "schema": [...], "values": {...} }`(作業コピー。要トークン) |
| GET | `/models/images` | `[{ "id": 1, "url": "/images/...", "original_filename": "logo.png", "uploaded_at": "..." }, ...]`(新しい順) |
| DELETE | `/models/images/{id}` | 画像と実体を削除(存在しない id は 404) |

`items/metadata` は**全アイテム分**を返す。保存されたことのないアイテムも `draft` として現れるので、
管理画面の一覧はこれだけで状態の列を描ける。

**変更系(`POST` / `PUT` / `DELETE`)は `200 OK` と空ボディを返す。** 以前は "… successfully" という
テキストを返していたが、Angular の `HttpClient` は本文を JSON として解釈するため、成功しているのに
クライアント側は「保存に失敗した」と判定していた(本文が空ならパースされない)。
本文が意味を持つもの(アイテム作成は id、publish / unpublish はメタデータ、画像アップロードは URL)
だけが JSON を返す。

### 画像

| メソッド | パス | 返すもの |
|---|---|---|
| POST | `/models/images/get_upload_url` | `{ "id": 1, "upload_url": "/images/<file>?key=..." }`(要トークン) |
| PUT | `/images/{file_name}?key=...` | 実体を保存(要トークン。`key` は一度きりで、発行時のファイル名に紐づく) |
| GET | `/images/{file_name}` | 実体の配信。**認証不要**(`<img>` はヘッダを付けられないため) |

アップロードは 2 段階(場所を貰う → 送る)。AWS では同じ契約を S3 の presigned URL が担う。
管理画面の「Images」(`/settings/images`)が `GET /models/images` を一覧し、アップロードと削除を行う。
コンテンツ編集の画像フィールドからは、同じ一覧を開いて**既存の画像を選び直せる**。

**画像の配列**(`{ "Array": ["Image"] }`)は、書き込み時に `[3]`(id のみ)でも `[{ "id": 3, "url": "..." }]`
でも受け付け、読み出しは `[{ "id", "url" }]` の配列で返す。管理画面ではサムネイルの並びとして編集でき、
ライブラリから**複数まとめて追加**・並べ替え・削除ができる(JSON を直接編集する表示にも切り替えられる)。
`Array: ["Image", "Number"]` のように Number と併用すると、素の数値が id なのか数値なのか決まらないため
サーバも管理画面も拒否する。

**削除は参照を検査しない。** 画像を id で参照しているアイテムはそのまま残り、参照先が解決しなくなる
だけ(使用中チェックと参照の書き換えは行わない、という判断)。

### 3.1 ページネーション

アイテム一覧は `?limit=&offset=` を受け付ける。並び順は**アイテム id の昇順**で固定なので、
`offset` を進めれば重複も抜けもなく全件を辿れる。

| | 公開 API (`/content/collections/{name}`) | 管理 API (`/models/collections/{name}/items`) |
|---|---|---|
| `limit` 未指定 | `50` 件(`total` と `next_offset` で続きが分かる) | **全件**(API の既定。管理画面は常に `25` 件ずつ要求する) |
| `limit` の上限 | `200` | `200` |
| 総件数 | レスポンスの `total` | `X-Total-Count` ヘッダ |
| 本文の形 | 変わらず `{schema, items, ...}` | 変わらず `[[id, values], ...]` |

- `limit=0`・`limit=201`・数値でない `limit` は **400**。黙って既定値に丸めたりはしない。
- `offset` が末尾を越えた場合はエラーではなく**空の最終ページ**(`next_offset: null`)。
- `total` は「このコレクションの公開アイテム数」(公開 API)/「全アイテム数」(管理 API)。
- 管理画面の一覧は `mat-paginator` から 10 / 25 / 50 / 100 件を選べる。最終ページの最後の 1 件を
  削除したときは 1 つ前のページへ戻る(空のページに取り残されないように)。
- 現状はアダプタが全件読んでから切り出す。ページング自体は API の契約なので、
  DynamoDB アダプタでは `Limit` / `ExclusiveStartKey` に押し下げる余地がある。
- on-prem アダプタは**ストレージ操作を 1 つずつ直列化**している。LMDB はトランザクション中に
  名前付き DB を開けず、このアダプタは操作ごとにストアを開くため、同時アクセスを許すと
  500 になる(管理画面は一覧・状態・ナビゲーションを並行して取りに行くので実際に踏んでいた)。
  スループットは単一ライター相当だが、同時リクエストで壊れることはない。
  読み書きを並列化するのは、スケールアウトを担う AWS アダプタ側の課題。

```bash
# 1 ページ目
curl 'http://127.0.0.1:8000/content/collections/blog?limit=2'
# => {"schema":[...],"items":[{...},{...}],"total":5,"limit":2,"offset":0,"next_offset":2}

# 最終ページまで next_offset を辿る
curl 'http://127.0.0.1:8000/content/collections/blog?limit=2&offset=2'

# 管理側: 総件数はヘッダで
curl -D - 'http://127.0.0.1:8000/models/collections/blog/items?limit=2' -H "Authorization: Bearer $TOKEN"
# => x-total-count: 5
```

### 3.2 `updated_at` と `created_at`

アイテム(単一ページはその 1 件)には、公開状態とは別に 2 つの時刻が付く。

| フィールド | 出る場所 | 意味 |
|---|---|---|
| `created_at` | 管理 API | 値が最初に保存された時刻 |
| `updated_at` | 管理 API | **作業コピー**が最後に保存された時刻 |
| `published_at` | 両方 | 最後に公開された時刻(unpublish で `null` に戻る) |
| `published_by` | 管理 API | 最後に公開した**アカウント**(`{ id, email }`)。unpublish で `null` に戻る |

公開 API に `updated_at` は**出しません**。2 コピーでは「編集した時刻」と「公開物が変わった
時刻」が別で、未公開の編集を `lastmod` として見せてしまうためです。公開物の最終更新は
`published_at` を使ってください(公開コピーが変わるのは publish のときだけ)。

- **`updated_at` は「作業コピーが変わった時刻」**。publish / unpublish では動かない。
- サイトの差分ビルドに使うのは **`published_at`**(公開物が変わった時刻)。
- 値を保存すると `updated_at` だけが進み、`created_at` と公開状態は変わらない
  (公開済みのアイテムを編集しても draft に戻らない)。
- この機能より前に保存された内容は `created_at` / `updated_at` が `null` になる(移行不要)。
  タイムスタンプを持たない古いメタデータレコードもそのまま読める。
- 管理 API のメタデータは `has_draft` も返すので、公開済み + 未公開の変更、を見分けられる。
- `published_by` は**監査用**: 公開した時点のメールと id を記録する。メールはその時の値なので、
  アカウントが後で改名・削除されても記録は読める。`published_at` と同じく unpublish で消える
  (サイトから下りたものに「誰が公開したか」は残らない)。**公開 API には出しません**。


#### 例

```bash
TOKEN=$(curl -s -X POST http://127.0.0.1:8000/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@example.com","password":"..."}' | jq -r .token)

# 公開する
curl -X POST http://127.0.0.1:8000/models/collections/blog/items/1/publish \
  -H "Authorization: Bearer $TOKEN"
# => {"status":"published","published_at":"2026-09-13T07:19:42.672723691Z",
#     "published_by":{"id":"...","email":"admin@example.com"},"created_at":"...","updated_at":"..."}

# 誰でも読める
curl http://127.0.0.1:8000/content/collections/blog
# => {"schema":[...],"items":[{"id":1,"published_at":"...","updated_at":"...","values":{"title":"Hello"}}],"total":1,"limit":50,"offset":0,"next_offset":null}
```

## 4. Gatsby からの使い方

```text
GET /content/collections                → 公開コレクションの一覧
GET /content/collections/{name}         → スキーマ + 公開アイテム(1 ページ分)
GET /content/collections/{name}?offset= → next_offset が null になるまで繰り返す
GET /content/single-pages/{name}        → スキーマ + 単一ページの値
```

ビルド時にこれらを取得し、Gatsby のノードとして `createPages` する薄い source plugin を
書くのが素直な形。`schema` が同梱されているので、値の解釈に追加リクエストは要らない。
再ビルドの起動は Webhook(第 5 節)に任せる。CMS は「何か変わった」ことだけを通知し、
サイト側はいつもの手順でビルドする、という分担にしている。

ページを跨ぐときは `next_offset` をそのまま `?offset=` に渡す。全件を舐めるのはビルド時の
一度きりなので、`updated_at` を保持しておけば次回以降は「前回より新しいアイテムだけ」を
処理する差分ビルドにも広げられる。

`/content/collections` を起点にすると「公開アイテムが1つも無いコレクション」は列挙されない。
空のコレクションもページにしたい場合は `/models/collections` (要トークン)を使うか、
サイト側で一覧を固定する。

## 5. Webhook(公開・非公開の通知)

サイトの再ビルドを起動するために、`publish` / `unpublish` のたびに設定した URL へ JSON を POST する。

### 設定(環境変数)

| 変数 | 意味 |
|---|---|
| `WEBHOOK_URLS` | 通知先(カンマ区切り)。未設定または空なら webhook は無効。`http` / `https` のみ受け付け、URL の誤りは**起動時**にエラーになる |
| `WEBHOOK_SECRET` | 本文の HMAC-SHA256 署名鍵(任意)。未設定なら署名せずに送る |

### リクエスト

```http
POST /hook HTTP/1.1
Content-Type: application/json
X-CMS-Event: collection_item.published
X-CMS-Delivery: c8876228-2f03-4b39-801d-f19f60131969
X-CMS-Signature: sha256=...
```

```json
{
  "event": "collection_item.published",
  "collection": "blog",
  "id": 1,
  "status": "published",
  "published_at": "2026-09-13T07:26:21.005941238Z",
  "occurred_at": "2026-09-13T07:26:21.006176750Z"
}
```

| `event` | 意味 |
|---|---|
| `collection_item.published` / `collection_item.unpublished` | アイテムの公開状態が変わった(`collection` と `id`) |
| `single_page.published` / `single_page.unpublished` | 単一ページの公開状態が変わった(`page`) |

- 本文に**値は含まない**。受け取った側が必要な `/content/*` を取りに行く(全件を送ると本文が
  肥大し、直後の再取得と二重管理になるため)。
- `published_at` は publish の時刻、unpublish では `null`。`occurred_at` はイベント発生時刻。
- `X-CMS-Delivery` は配信ごとの UUID で、受信側の重複排除に使える。

### 署名の検証(HMAC-SHA256)

`X-CMS-Signature` は `sha256=` + **生のリクエストボディ**に対する HMAC-SHA256(16 進小文字)。
JSON をパースして再シリアライズするとバイト列が変わるので、必ず生ボディで検証すること。

```python
expected = "sha256=" + hmac.new(SECRET, raw_body, hashlib.sha256).hexdigest()
if not hmac.compare_digest(request.headers["X-CMS-Signature"], expected):
    return 401
```

### 配信の性質(重要)

- 配信は**リクエストを待たせない**。publish は状態を保存した時点で応答し、送信はバックグラウンドで行う。
- 失敗時は 500ms → 1s の間隔で最大 3 回試行する。`4xx` は再試行しない(受信側が理解して拒否しているため)。
- 受信先が落ちていても publish は成功する(200)。失敗した配信はサーバログの
  `webhook delivery failed` に出る。
- 複数の URL は独立して配信される。1 つが落ちても他は受け取る。
- `https` 宛の証明書検証は**デプロイ先のトラストストア**を使う(ルート証明書を同梱しない)。
  CA 証明書を持たない最小構成のコンテナでは検証に失敗するので、デプロイ先にシステムの
  CA バンドルがあることを確認する。暗号実装は `ring` を起動時に登録している
  (`reqwest` 0.13 はプロバイダを同梱しないため)。
- `publish` が 404 になるなど**状態が保存されなかった場合は送らない**。
- **outbox は用意しない**(現時点の方針): 配信前にプロセスが落ちるとそのイベントは失われる。
  「再ビルドを起動する」用途では取りこぼしても次の publish でやり直せるため、許容している。
- **AWS Lambda では応答後に実行環境が凍結され得る**ため、バックグラウンド送信が完了しない可能性がある。
  Lambda に載せる際は SQS / EventBridge 経由にするか、配信を同期化する必要がある。
- 順序は保証しない。受信側は「再ビルドを起動する」程度の使い方を想定している。

## 5.5 権限

| 操作 | 必要 |
|---|---|
| 読む(下書きを含む) | `can_view` |
| 値の作成・編集(作業コピー) | `can_edit` |
| 公開 / 非公開、コンテンツの削除 | `can_publish` |
| スキーマ・コレクション・単一ページ・複合フィールドの変更 | `is_admin` |
| アカウント管理(`/auth/users`) | `is_admin` |

ロールはこのフラグの組み合わせとして扱う。

| ロール | can_view | can_edit | can_publish | is_admin |
|---|---|---|---|---|
| 確認(閲覧のみ) | ✓ | – | – | – |
| 編集(下書きまで) | ✓ | ✓ | – | – |
| 公開(編集 + 公開操作) | ✓ | ✓ | ✓ | – |
| 管理者 | ✓ | ✓ | ✓ | ✓ |

判定は「読み取りは `can_view`、それ以外は `can_edit`」を 1 か所のミドルウェアで行い、
メソッドだけでは決まらないもの(公開・削除・構造変更)はハンドラ側で追加判定する。
**編集は公開コピーに触れない**ので、`can_edit` だけのロールを安全に運用できる。

### アカウント管理 API

| メソッド | パス | 内容 |
|---|---|---|
| GET | `/auth/users` | 一覧 |
| POST | `/auth/users` | 作成(`email` / `password` / `is_admin` / `permission`) |
| PATCH | `/auth/users/{id}` | `is_admin` / `is_active` / `permission` の部分更新 |
| DELETE | `/auth/users/{id}` | 削除 |
| POST | `/auth/users/{id}/password` | パスワード再設定(現在のパスワード不要) |
| POST | `/auth/me/password` | 自分のパスワード変更(現在のパスワードが必要) |

`/auth/users*` は管理者のみ。`/auth/me/*` は自分自身への操作なので、**書き込み権限の無い
アカウントでも使える**(閲覧のみの人がパスワードを変えられない、という状態を避けるため)。

- **有効な管理者が 0 人になる変更は拒否する**(最後の管理者の降格・無効化・削除は 409)。
  自分自身を降格してロックアウトすることもできない。
- 無効化したアカウントはログインできず、既存トークンも拒否される。削除は履歴ごと消える。
- **パスワード変更は既存のトークンをすべて失効させる**。アカウントはトークン世代
  (`token_version`)を持ち、トークンは発行時の世代を運ぶ。認証は毎回アカウントを読むので、
  世代が古いトークンは残りの有効期限によらず 401 になる(クロックの一致も要らない)。
  - `POST /auth/me/password` は**新しい世代のトークン**を返す。変更した本人のセッションも
    切れるため、画面はこれを保存して続行する(他の端末のセッションは終了したまま)。
    応答: `{"token":"...","expires_at":"..."}`
  - `POST /auth/users/{id}/password`(管理者による再設定)は対象アカウントのトークンだけを
    失効させる。実行者自身のセッションは続く。応答は本文なしの 200。

### ログイン試行の制限

総当たりを止めるため、**同じメールアドレスに対する失敗を数え**、一定回数を超えると 429 を返す。

| 項目 | 値 |
|---|---|
| 失敗の許容回数 | 5 回 |
| 失敗を数える期間 | 15 分(これより間が空いた失敗は数えない) |
| ロックの長さ | 最後の失敗から 15 分 |
| サインインに成功したとき | カウンタを消す |

- 429 の本文は理由と待ち時間、ヘッダに `Retry-After`(秒)を付ける。
- ロック中は**正しいパスワードでも通らない**(パスワードを見る前に拒否する)。
- **存在しないアドレスも同じように数える**。実在するアカウントだけを数えると、「429 になったか
  どうか」でアカウントの有無が分かってしまい、401 のメッセージを統一している意味が無くなる。
- ロック中に試行を重ねても**ロックは延びない**(最後の失敗から 15 分で必ず解ける)。1 人の
  ミスの連発で組織全体が止まることもない(カウンタはアドレスごと)。
- カウンタは**プロセスのメモリ**にある。再起動で消え、複数のプロセスでは共有されない
  (Lambda のようにインスタンスが入れ替わる環境では共有ストレージが要る)。
- 同じカウンタを `POST /auth/me/password` の現在のパスワード確認にも使う。

### リソース単位の権限

アカウント共通のロールに加えて、**コレクション / 単一ページごとに許可を上書き**できる。上書きは
そのリソースだけ**置き換え**なので、広げることも狭めることもできる。

| 例 | `permission` | `collection_permissions` |
|---|---|---|
| どこでも閲覧のみ | viewer | – |
| `blog` だけ編集できる | viewer | `{ "blog": editor }` |
| `legal` だけ触らせない | editor | `{ "legal": すべて false }` |

- `PATCH /auth/users/{id}` の `collection_permissions` / `single_page_permissions` に
  **マップ全体**を送る(部分マージではない)。`{}` を送れば上書きは全部消える。
- 判定は「そのリソースの実効権限」で行う。読み取りは `can_view`、書き込みは `can_edit`、
  公開・非公開・削除は `can_publish`。**管理者は常にすべて可**(上書きで締め出せない)。
- `/models/collections` と `/models/single_pages` の一覧は**読めるものだけ**返すので、拒否した
  リソースはナビにも出ない(直接開けば 403)。
- 存在しない名前への付与は 400。綴り間違いが「どこにも出てこない効かない許可」として残らない。
- 対象はコレクションと単一ページだけ。画像ライブラリと複合フィールド定義はアカウント共通
  (`can_edit` / 管理者)のまま。

## 5.6 共有できるプレビュー URL

アカウントを持たない相手(クライアント、翻訳者)に下書きを見せるための、**署名付き・期限付き**の
URL。相手はトークンもアカウントも要らない。

```bash
# 管理側がリンクを発行する(要 can_edit)
curl -X POST http://127.0.0.1:8000/models/collections/blog/items/1/preview-link \
  -H "Authorization: Bearer $TOKEN"
# => {"path":"/preview/collections/blog/items/1?token=1758000000.3f9c...","expires_at":"..."}

# 受け取った人はトークン無しで開ける(作業コピーが見える)
curl http://127.0.0.1:8000/preview/collections/blog/items/1?token=1758000000.3f9c...
```

| メソッド | パス | 内容 |
|---|---|---|
| POST | `/models/collections/{name}/items/{id}/preview-link` | リンクを発行(要 `can_edit`) |
| POST | `/models/single_pages/{name}/preview-link` | 同上 |
| GET | `/preview/collections/{name}/items/{id}?token=...` | 作業コピーを返す(認証不要) |
| GET | `/preview/single_pages/{name}?token=...` | 同上 |

- 期限は `PREVIEW_LINK_TTL_MINUTES`(既定 60 分)。トークンは `有効期限.署名` の形で、
  署名は**行き先と有効期限そのもの**に対する HMAC-SHA256(`JWT_SECRET` を使用、メッセージには
  専用の接頭辞を付けるので他の署名と使い回せない)。
  - 行き先を書き換えれば署名が合わなくなるので、**1 本のリンクは 1 つの作業コピーしか開けない**。
  - 有効期限を先に延ばすこともできない。期限切れは 403、署名違い・壊れたトークンは 401。
- サーバ側に**保存するものが無い**(期限が署名に含まれるので、消すべきレコードが存在しない)。
- ただしリンクは**持っている人にとっては資格情報**。期限まではその 1 件の下書きを読めるので、
  渡す相手と有効期限は意識すること。失効させたい場合は `PREVIEW_LINK_TTL_MINUTES` を短くするか、
  `JWT_SECRET` を変える(全トークンが無効になる)。
- 返す本文は管理側のプレビューと同じ形(schema + values)なので、サイト側は 1 つのパーサで済む。

## 6. まだ無いもの

- **`published_at` を使った差分ビルド**: 値は返っているが、サイト側の実装はこれから。
- **監査ログ(履歴)**: いまは「最後に公開したアカウント」だけを持つ。誰がいつ何を保存・公開・
  削除したかの**履歴**は残していない(追記型のログが要る)。
- **`doc/swagger.yaml`**: 実装済みルートの一部しか載っていない古い記述のまま。

## 7. CMS 側に GraphQL を持たせない方針

Gatsby の GraphQL は**ビルド時のデータ層**であり、CMS が GraphQL を喋る必要はない。
`gatsby-source-graphql` は事実上非推奨で、Gatsby 自体も活発ではない。
そのため、まずは REST の公開 API を整え、必要になったら次の順で進める。

1. `/content/*`(本ドキュメント。実装済み)
2. publish / unpublish を契機にした Webhook(本ドキュメント。実装済み)
3. Gatsby の source plugin(REST → GraphQL ノード)
4. 消費側が増えて GraphQL が本当に必要になったときだけ、GraphQL 層を検討する
