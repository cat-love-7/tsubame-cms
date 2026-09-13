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

- 保存しただけの内容は **draft**。公開は `publish` を明示的に呼んだときだけ行われる。
- 状態はアイテムの値とは**別のストア**(`item_metadata`)に入る。スキーマに `status` や
  `published_at` というフィールドがあっても衝突しない。
- レコードが無い = draft。既存データの移行は不要で、これまでどおりの保存は下書きのまま。
- コレクション / 単一ページを削除すると、その配下のメタデータも消える(同じ名前で作り直しても
  前の公開状態を引き継がない)。
- draft への `/content/*` は **404**(403 ではない)。未公開コンテンツの存在自体を漏らさないため。
- `published_at` は publish 時に記録し、unpublish すると `null` に戻る(公開日時は保持しない)。

## 3. エンドポイント

### 公開(認証不要)

| メソッド | パス | 返すもの |
|---|---|---|
| GET | `/content/collections` | 公開アイテムを1つ以上持つコレクション名の配列 |
| GET | `/content/collections/{name}` | `{ "schema": [...], "items": [...] }` |
| GET | `/content/collections/{name}/items/{id}` | `{ "id": 1, "published_at": "...", "values": {...} }` |
| GET | `/content/single-pages` | 公開済み単一ページ名の配列 |
| GET | `/content/single-pages/{name}` | `{ "schema": [...], "published_at": "...", "values": {...} }` |

### 管理(要トークン。`publish` / `unpublish` は編集権限が必要)

| メソッド | パス | 返すもの |
|---|---|---|
| GET | `/models/collections/{name}/items/metadata` | `{ "1": { "status": "draft", "published_at": null }, ... }` |
| GET | `/models/collections/{name}/items/{id}/metadata` | そのアイテムのメタデータ |
| POST | `/models/collections/{name}/items/{id}/publish` | 更新後のメタデータ(存在しない id は 404) |
| POST | `/models/collections/{name}/items/{id}/unpublish` | 更新後のメタデータ |
| GET | `/models/single_pages/{name}/item/metadata` | そのページのメタデータ |
| POST | `/models/single_pages/{name}/publish` | 更新後のメタデータ |
| POST | `/models/single_pages/{name}/unpublish` | 更新後のメタデータ |

`items/metadata` は**全アイテム分**を返す。保存されたことのないアイテムも `draft` として現れるので、
管理画面の一覧はこれだけで状態の列を描ける。

### 例

```bash
TOKEN=$(curl -s -X POST http://127.0.0.1:8000/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"admin@example.com","password":"..."}' | jq -r .token)

# 公開する
curl -X POST http://127.0.0.1:8000/models/collections/blog/items/1/publish \
  -H "Authorization: Bearer $TOKEN"
# => {"status":"published","published_at":"2026-09-13T07:19:42.672723691Z"}

# 誰でも読める
curl http://127.0.0.1:8000/content/collections/blog
# => {"schema":[...],"items":[{"id":1,"published_at":"...","values":{"title":"Hello"}}]}
```

## 4. Gatsby からの使い方

```text
GET /content/collections                → 公開コレクションの一覧
GET /content/collections/{name}         → スキーマ + 公開アイテム全件
GET /content/single-pages/{name}        → スキーマ + 単一ページの値
```

ビルド時にこれらを取得し、Gatsby のノードとして `createPages` する薄い source plugin を
書くのが素直な形。`schema` が同梱されているので、値の解釈に追加リクエストは要らない。
再ビルドの起動は Webhook(第 5 節)に任せる。CMS は「何か変わった」ことだけを通知し、
サイト側はいつもの手順でビルドする、という分担にしている。

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
- `publish` が 404 になるなど**状態が保存されなかった場合は送らない**。
- **outbox は無い**: 配信前にプロセスが落ちるとそのイベントは失われる。
- **AWS Lambda では応答後に実行環境が凍結され得る**ため、バックグラウンド送信が完了しない可能性がある。
  Lambda に載せる際は SQS / EventBridge 経由にするか、配信を同期化する必要がある。
- 順序は保証しない。受信側は「再ビルドを起動する」程度の使い方を想定している。

## 6. まだ無いもの(サイト運用の前に必要になる)

- **ページネーション**: `/content/collections/{name}` は現状**全件**返す。
- **`updated_at`**: 更新日時を記録していない。サイトマップや差分ビルドには必要。
- **プレビュー**: 下書きの確認は管理画面のみ。トークン付きプレビュー URL は未実装。
- **Webhook の永続化(outbox)**: 配信前にプロセスが落ちるとイベントは失われる(上記参照)。
- **`doc/swagger.yaml`**: 実装済みルートの一部しか載っていない古い記述のまま。

## 7. CMS 側に GraphQL を持たせない方針

Gatsby の GraphQL は**ビルド時のデータ層**であり、CMS が GraphQL を喋る必要はない。
`gatsby-source-graphql` は事実上非推奨で、Gatsby 自体も活発ではない。
そのため、まずは REST の公開 API を整え、必要になったら次の順で進める。

1. `/content/*`(本ドキュメント。実装済み)
2. publish / unpublish を契機にした Webhook(本ドキュメント。実装済み)
3. Gatsby の source plugin(REST → GraphQL ノード)
4. 消費側が増えて GraphQL が本当に必要になったときだけ、GraphQL 層を検討する
