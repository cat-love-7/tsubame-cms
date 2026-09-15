# AWS: DynamoDB の設計

`aws` バックエンドの構造化データを **1 テーブル** に収めるための設計。実装はこの文書だけを
見て書ける状態を目標にする。前提は `doc/aws-plan.md`(決定事項と TODO)。

## 1. 原則

- **1 テーブル**、`PK`(S) + `SK`(S)。オンデマンド。GSI は使わない(すべての一覧が
  パーティションクエリで済む設計にするため)。
- **値は JSON 文字列として保存する**。この CMS のドメイン型は「スキーマだけが型情報を持つ
  `serde_json::Value`」なので、DynamoDB の型へ写す必要がない。`data` という 1 属性に入れる。
  - 利点: 型変換のコードがゼロになり、on-prem のレコードと 1 対 1 で対応する。
  - 代償: アイテム内のフィールドでサーバ側フィルタはできない(必要ない)。
- **読み取りは `ConsistentRead = true`**。CMS は「保存した直とに一覧を見る」ため、結果整合の
  取りこぼしがそのまま UI の不具合になる。読み取りコストは 2 倍だが、この規模では誤差。
- **アイテム ID はゼロ埋め**して `SK` に入れる(`{:020}`)。DynamoDB の `SK` はバイト順なので、
  ゼロ埋めしないと 10 が 2 より前に来る。

## 2. キー設計

| 実体 | PK | SK | 補足 |
|---|---|---|---|
| コレクションスキーマ | `collection#<name>` | `schema` | 存在確認も兼ねる |
| アイテム(公開コピー) | `collection#<name>` | `item#<id:020>` | 配信 API が見る唯一のコピー |
| アイテム(作業コピー) | `collection#<name>` | `draft#<id:020>` | |
| アイテムメタデータ | `collection#<name>` | `meta#<id:020>` | status / published_at / last_published_at / created_at / updated_at / published_by |
| 一意な値の予約 | `unique#<collection>#<field>` | `<value>` | その値を握っているアイテム id(条件付き put で確保、`attribute_not_exists(pk) OR data = :id`) |
| コレクションの ID カウンタ | `collection#<name>` | `counter` | `UpdateItem ADD` で原子的に採番 |
| コレクション名一覧 | `collections` | `<name>` | 名前だけの小さなアイテム |
| 単一ページスキーマ | `page#<name>` | `schema` | |
| 単一ページの内容(公開) | `page#<name>` | `item` | |
| 単一ページの作業コピー | `page#<name>` | `draft` | |
| 単一ページのメタデータ | `page#<name>` | `meta` | |
| 単一ページ名一覧 | `pages` | `<name>` | |
| 複合フィールド定義 | `composite#<id>` | `schema` | |
| 複合フィールド一覧 | `composites` | `<id>` | |
| ユーザー | `users` | `user#<uuid>` | |
| **ユーザー名の予約** | `users` | `username#<name>` | 値は user id。**一意性をここで担保** |
| 画像レコード | `images` | `image#<id:020>` | file_name も保持 |
| 画像の ID カウンタ | `images` | `counter` | |
| アップロードの単回トークン | `upload_keys` | `<key>` | 値は file_name、`expires_at`(TTL)付き |

- 一覧系はすべて「パーティションを指定した `Query`」になる:
  - コレクション名 / ページ名 / 複合フィールド / 画像 / ユーザー
  - アイテム: `PK = collection#<name>` かつ `SK begins_with "item#"`(公開)、`draft#`、`meta#`
- ユーザー名の予約アイテムをユーザー本体と**同じトランザクション**で書くことで、
  「同名の 2 人」が作れない。`attribute_not_exists(SK)` を条件にする。

## 3. アクセスパターン → 操作

| トレイトのメソッド(代表) | 操作 |
|---|---|
| `get_collection_schema` / `get_single_page_schema` / `get_composite_field_schema` | `GetItem` |
| `list_collection_names` / `list_all_page_names` / `list_composite_field_schemas` / `get_all_images` / `get_all_users` | `Query`(一覧パーティション) |
| `add_*_schema` / `update_*` | `PutItem` |
| `add_collection_item` | `UpdateItem ADD counter 1` → `PutItem item#` |
| `get_collection_item` / `get_collection_item_draft` / `get_item_metadata` | `GetItem` |
| `list_collection_items` / `list_collection_item_drafts` / `list_item_metadata` | `Query` + `begins_with` |
| `delete_collection_item` | `TransactWriteItems`(`item#` + `draft#` + `meta#` を削除。存在しないキーの削除は無視される) |
| `delete_collection` | `TransactWriteItems`(スキーマ + 一覧アイテム + 配下のアイテム/下書き/メタデータを削除) |
| 公開(`set_item_status` から) | 下書きを `GetItem` → あれば `TransactWriteItems`(公開コピーへ Put + 下書き Delete + メタデータ Put)、無ければメタデータだけ Put |
| `add_user` | `TransactWriteItems`(ユーザー Put + ユーザー名予約 Put、予約に `attribute_not_exists`) |
| `delete_user` | `TransactWriteItems`(ユーザー Delete + 予約 Delete) |
| `generate_image_upload_url` | `UpdateItem ADD`(画像 ID)→ レコード Put → S3 の presigned PUT URL(トークンは S3 が担うので不要) |

**公開の「下書きが無い場合」**は、そもそも削除をトランザクションに入れない(先に読んで分岐する)。
`doc/aws-plan.md` に「要確認」と書いた『存在しないキーの削除がトランザクションを失敗させるか』は、
この設計では**問題にならない**(該当ケースで削除を発行しない)。

## 4. 上限と検証項目

- **1 アイテム 400KB**: アイテムの値・スキーマ・複合フィールド定義が収まること。超える場合は
  400 を返す(DynamoDB は ValidationException を返す)。複合フィールドを多用した大きめの値を
  入れるテストを置く。
- **`Query` 1MB**: 一覧は 1MB で切れる。アイテム一覧は `LastEvaluatedKey` で**ページを繰り返して
  全件取る**(トレイトの契約は「全件返す」なので、内部でループする)。
- **`TransactWriteItems` 100 項目 / 4MB**: 公開は 3 項目、ユーザー作成は 2 項目、コレクション削除は
  配下の件数に依存する → **コレクション削除は 25 件ずつに分けて実行**する(トランザクションは使わず、
  下書き/メタデータ/アイテムを順に消す。途中で失敗しても再実行できる)。
- **TTL**: アップロードトークンの `expires_at` に使う(削除は非同期なので、消費時に期限も確認する)。

## 5. テストの作り方

- **テーブルはテストごとに作る**: `test_<uuid>` を作り、`Drop` で消す。`open_test_repository` の
  AWS 版を `aws.rs` に置き、`http/tests.rs` のゲートを `any(on-premises, aws)` に広げる。
- **エンドポイント差し替え**: `AWS_ENDPOINT_URL`(既定 `http://localhost:8000`)、
  `AWS_REGION=us-east-1`、`AWS_ACCESS_KEY_ID=test` / `AWS_SECRET_ACCESS_KEY=test`。
  DynamoDB Local は資格情報を要求する(実サービスと同じ)。
- **S3 は MinIO**: `S3_ENDPOINT_URL`(既定 `http://localhost:9000`)、`force_path_style(true)`、
  `MINIO_ROOT_USER=test` / `MINIO_ROOT_PASSWORD=test-secret`。presign → PUT → GET を実際に往復する
  (MinIO は SigV4 を検証するので、presigner が壊れればテストが落ちる)。
- 起動: `docker compose up -d`(`sl_cms/docker-compose.yml`)。

## 6. 依存とビルド

`aws` フィーチャーで追加する予定: `aws-config`、`aws-sdk-dynamodb`、`aws-sdk-s3`
(プリサインド URL に必要なフィーチャーは**実装時に確認**する)。`aws-sdk-*` は依存が多く
ビルドが重いので、`aws` フィーチャーのビルド時間は on-premises より明確に長くなる。
既定ビルドには影響しない(`dep:` で括る)。

## 7. 先に決めること: 同期トレイトと非同期 SDK

**リポジトリのトレイトは同期**(`fn get_collection_schema(&self, ...) -> Result<...>`)で、これは
on-premises の rkv が同期だから。**AWS SDK は非同期**なので、そのままでは実装できない。

| 案 | 内容 | 代償 |
|---|---|---|
| **A. トレイトを async にする** | 41 トレイトメソッド + サービス層 47 メソッドを async 化。HTTP ハンドラは既に async なので `.await` を足すだけ。on-prem 実装は「await しない async fn」になる。テストは `#[tokio::test]` + `.await` | 機械的だが差分は大きい(サービス内テスト 53 件)。オブジェクト安全は不要(すべてジェネリック)なので `async fn` in trait で書ける |
| **B. アダプタ内でブロックする** | 専用のランタイムを持つスレッドを 1 本立て、同期メソッドからそこへ投げて待つ | 差分は小さいが、**すべての DB 呼び出しでスレッドを跨いでブロック**する。Lambda では動くが、常駐サーバでは同時実行をスレッド数で制限してしまう。将来 A に置き換えると二度手間 |

**推奨は A**。この CMS の層構成(トレイト → サービス → HTTP)は元々「ストレージを差し替えられる」
ために作ってあり、非同期ストレージを差し込むならトレイトを async にするのがその延長線上にある。
テスト 230 件が安全網になる。

### 7.1 実際に採った順序: B′ を経て A へ(完了)

A を機械的に一括で当てるのを 3 回試して 3 回落ちた(同名の同期/非同期メソッドが複数の型に
あり、`to_response` や `notify` の呼び出しが別の型のメソッドに化ける。複数行のチェーンと
`format!` の中の `{}` のせいで「囲っている関数」の検出も安定しない)。そこで順序を変えた:

1. **アダプタは async で書く**(AWS SDK が求める形。ここが最終的に残る実装)。
2. トレイトの同期メソッドは、専用スレッドでランタイムを持つ `BlockingRuntime` に
   1 メソッド 1 往復で委譲する(差分は小さい)。
3. 契約スイートを DynamoDB Local に対して通し、アダプタの正しさを先に固める。
4. そのあと **A**(トレイトを async に)を行い、1 の実装をそのまま残して 2 を消す。

「同期メソッドをランタイムの中から呼ぶ」が一番壊れやすいので、そこは
`aws::repository::tests::a_collection_round_trips_against_dynamodb_local` が
`#[tokio::test]` の中から同期トレイトを呼ぶ形で押さえている。

**→ A は完了した(2026-09)。** トレイトは
`fn f(...) -> impl Future<Output = ...> + Send` になり、`BlockingRuntime` ブリッジと
`bridge.rs`、`s3_blocking` / `delete_table_blocking` は消えた。分かったこと:

- **`async fn` ではなく `impl Future + Send` を書く。** `async fn` in trait は Send を約束せず、
  `R: Storage` にジェネリックな axum ハンドラが「future cannot be sent between threads」で
  落ちる。実装側は `async fn` のままでよい。
- **変換はコンパイラに運転させる。** トレイトを async にした瞬間、全呼び出し箇所が型エラーに
  なる。rustc は「`await` をここに入れろ」を span と置換文字列で出すので、名前で置換するより
  確実(以前それで `to_response` / `notify` の別型メソッドを壊した)。同じ提案が lib と test の
  2 回出るので**重複除去が必須**。
- **半分だけ変換したアダプタはデッドロックする。** ブリッジ経由の trait と直接 async の trait が
  同じ SDK クライアントを使うと、接続プールがランタイム単位なので固まる。アダプタは 1 単位で
  変換する。
- 残る山は**借用**だった: `let result = service.call(&"x".into()); ... result.await` のような
  テストは、await を定義側へ移す(`let result = service.call(&"x".into()).await;`)と両方直る。
  整形処理(`models/field.rs`)は画像をイテレータの closure の中で引いていたので、
  **解決済みの画像地図を渡す**形にして純粋関数にした(1 リクエスト 1 クエリになり、以前の
  「値ごとに 1 回」より速い)。

変換に使った使い捨ての道具(ブレース対応を正しく扱う書き換えと、rustc の診断 span から
`.await` を差し込むスクリプト)は**削除した**。移行は完了していて、対象だった `BlockingRuntime`
ブリッジも無いので、残しても実行できない。コードは git 履歴にある。残った教訓は上の箇条書き
のとおり(**Rust を正規表現で書き換えない。コンパイラの診断を入力にする**)で、それがこの節の
値打ちでもある。

**確認済み**: テーブル作成、スキーマと一覧の索引、原子的採番(1 → 2)、公開/下書き/メタデータの
3 レコード、アイテム削除で 3 レコード、コレクション削除で配下と索引まで消えること。

**ブリッジの掟(実装中に踏んだ罠)**: SDK の HTTP クライアントは接続をプールし、その接続は
**開いたランタイムに属する**。別のランタイムから流すと、所有側は `block_on` で塞がっているので
永遠に待つ。つまり「あるクライアントの最初のリクエストはブリッジ経由で」が必須で、テストでも
S3 のバケット作成を直接 SDK で叩くとその後の `delete_image` が固まる(実際に踏んだ)。
テスト用の `AwsRepository::s3_blocking` はこのためにある。

## 8. 未確定(実装時に確認)

- `aws-sdk-s3` のプリサインド URL に必要な正確なフィーチャー名と API。
- `TransactWriteItems` の `Update` で `ADD` を使えるか(使えれば採番もトランザクションに含められる。
  現設計は「採番 → Put」の 2 手なので、失敗時に ID が 1 つ飛ぶだけで済む)。
- `Query` の `ConsistentRead` を全一覧に使ったときのコスト感(必要なら一覧だけ結果整合にする)。
