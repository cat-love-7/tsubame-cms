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

`/content/collections` を起点にすると「公開アイテムが1つも無いコレクション」は列挙されない。
空のコレクションもページにしたい場合は `/models/collections` (要トークン)を使うか、
サイト側で一覧を固定する。

## 5. まだ無いもの(サイト運用の前に必要になる)

- **ページネーション**: `/content/collections/{name}` は現状**全件**返す。
- **変更通知 (Webhook)**: 今はビルド時に全件取得する pull 型。publish/unpublish を契機に
  再ビルドを起動する仕組みは未実装。
- **`updated_at`**: 更新日時を記録していない。サイトマップや差分ビルドには必要。
- **プレビュー**: 下書きの確認は管理画面のみ。トークン付きプレビュー URL は未実装。
- **`doc/swagger.yaml`**: 実装済みルートの一部しか載っていない古い記述のまま。

## 6. CMS 側に GraphQL を持たせない方針

Gatsby の GraphQL は**ビルド時のデータ層**であり、CMS が GraphQL を喋る必要はない。
`gatsby-source-graphql` は事実上非推奨で、Gatsby 自体も活発ではない。
そのため、まずは REST の公開 API を整え、必要になったら次の順で進める。

1. `/content/*`(本ドキュメント。実装済み)
2. publish / unpublish を契機にした Webhook
3. Gatsby の source plugin(REST → GraphQL ノード)
4. 消費側が増えて GraphQL が本当に必要になったときだけ、GraphQL 層を検討する
