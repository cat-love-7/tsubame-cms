# AWS 対応の TODO

この CMS を AWS(Lambda + DynamoDB + S3)で動かすための作業一覧。**現状のコードを実際に読んで
確認した事実**だけを前提にしている。推測で書いた項目は「(要確認)」と明記した。

## 0. 現状

- バックエンドは **コンパイル時のフィーチャー選択**。`Cargo.toml` に `on-premises`(既定)/
  `aws`/`gcp`/`azure` があり、`main.rs` が「1 つだけ選べ」「0 個は駄目」「`aws` は未実装」を
  `compile_error!` で表現している。
- `aws` フィーチャーは `dep:lambda_http` を引くだけで、**`src/aws/` も `run_aws()` も無い**
  (`lambda_http` は `src/` から一度も参照されていない)。
- 差し替え口は整っている: `AppModule<R: Storage>`、`http::router` は `R: Storage` でジェネリック、
  共有層(`http` / `services` / `models`)に rkv への参照は無い。バックエンド固有の都合
  (rkv のパス、テスト用リポジトリ構築)は `on_premises` 側に寄せてある。
- 実装すべきトレイトは 5 つ・**計 44 メソッド**:
  | トレイト | メソッド数 | 主な中身 |
  |---|---|---|
  | `CollectionRepository` | 16 | スキーマ、アイテム、**下書き**、**メタデータ**、一覧 |
  | `SinglePageRepository` | 11 | スキーマ、1 アイテム、下書き、メタデータ |
  | `ImageRepository` | 7 | 一覧/取得、**アップロード URL 発行**、単回トークン、バイト読み書き |
  | `UserRepository` | 6 | id/username 取得、作成、更新、一覧、削除 |
  | `CompositeFieldRepository` | 4 | 定義の一覧/取得/追加/削除 |
- コンテンツは **2 コピー + メタデータ**: 公開コピー(配信 API が見る)、作業コピー(下書き)、
  状態/時刻/公開者。DynamoDB では 1 アイテムにつき 3 レコードになる。
- いまの統合テストは実ルーターを `oneshot` で叩き、**実バックエンド**に対して動く
  (`http/tests.rs` は `feature = "on-premises"` でゲート、テスト用リポジトリは
  `on_premises::open_test_repository` が用意する)。CI と compose はリポジトリに無い。
- プロセス内メモリに依存している箇所が 3 つある: **ログイン試行のカウンタ**、**画像アップロードの
  単回トークン**、**Webhook のバックグラウンド送信**。Lambda では共有されない/凍結される。

## 1. 先に決めること(後戻りが大きい順)

1. **画像の読み出し**: Lambda が `/images/{file_name}` をプロキシして現行のレスポンス契約を保つか、
   CloudFront/S3 の URL に変えるか。変える場合、配信 API・フロント・E2E の期待値が変わる。
2. **認証**: ローカルの HS256 のままか、Cognito に移すか。移すなら `sub` → ローカル権限レコードの
   対応づけ(`external_id`)が要る。移さない場合、パスワード変更・リセット・ロックアウトは
   現行実装をそのまま使う(ただしトークン失効の `token_version` は DynamoDB のユーザーレコードで
   読むので共有される)。
3. **Webhook の配信方式**: SQS / EventBridge 経由にするか、同期化するか(現状のままだと Lambda の
   応答後凍結で配信が失われ得る。`doc/content-api.md` §5 に既知の制約として記載済み)。
4. **IaC**: SAM / CDK / Terraform。テスト用の使い捨てスタックもこれで作る。
5. **下書き→公開の整合性**: DynamoDB では `TransactWriteItems`(最大 100 項目/4MB)で
   「公開コピーへ複製 + 下書き削除 + メタデータ更新」を 1 トランザクションにする前提でよいか。

## 2. TODO

各項目に「完了条件(= 何で確認するか)」を付ける。

### P0. 土台(AWS アダプタ無しでも進められる)

- [ ] `http/tests.rs` のゲートを `any(feature = "on-premises", feature = "aws")` に広げ、
      テスト用リポジトリ構築をバックエンド別ヘルパー経由にする
      → **完了条件**: `cargo test`(既定)が今までどおり 229 件通り、AWS 用ヘルパーを足す場所が
      1 か所に定まっている。
- [ ] `docker-compose.yml`(DynamoDB Local + LocalStack S3)を追加
      → **完了条件**: `docker compose up -d` の後、SDK の `endpoint_url` で両方に到達できる。
- [ ] CI(GitHub Actions)を追加。ジョブは ①on-prem テスト ②エミュレータ + AWS フィーチャーの
      テスト ③(夜間/手動)staging への E2E
      → **完了条件**: ①②が PR で回り、③は手動トリガのみ。
- [ ] `main.rs` の「AWS 未実装」`compile_error!` を外し、`mod aws;` と
      `aws::build_app_module(&config)` の骨組みを追加
      → **完了条件**: `cargo check --no-default-features --features aws` が通り、
      「未実装」以外の理由で落ちない。
- [ ] AWS 用の設定を整理(`AWS_REGION`、テーブル名、バケット名、Cognito を使うなら
      プール ID / JWKS URL)。`DATA_ROOT` は on-premises 専用であることを明示
      → **完了条件**: `Config` のテストで既定値と必須項目の欠落時エラーを確認。

### P1. DynamoDB アダプタ(本丸)

- [ ] **単一テーブルのキー設計を決めて文書化**する。案: `PK = collection#<name>` /
      `SK = item#<id>`(公開)、`draft#<id>`(下書き)、`meta#<id>`(メタデータ)、
      スキーマは `PK = schema#collection` / `SK = <name>`、単一ページ・複合フィールド・ユーザーも
      同様に前置きで分ける。GSI の要否(ユーザー名の一意性、ユーザー一覧、コレクション一覧)も決める
      → **完了条件**: `doc/` に表として残し、実装がそれだけを見て書ける。
- [ ] 44 メソッドを実装。押さえるべき点:
      - アイテム ID は **`UpdateItem` の `ADD` で原子的に採番**(on-prem の `id_counter` 相当。
        並行作成で重複しないこと)。
      - 下書き一覧・メタデータ一覧は **SK の `begins_with` クエリ**(DynamoDB に prefix scan は無い)。
      - 公開は **`TransactWriteItems`** で 3 レコードをまとめる。
      - 一覧のページングは `Limit` / `ExclusiveStartKey` に押し下げる(現状は全件取得してから
        ページングしている。`doc/content-api.md` §3.1 に余地として記載済み)。
      - 条件付き書き込みで「存在しないアイテムの削除」等を冪等にする(rkv 実装が握っている
        挙動と同じ結果にする)。
      → **完了条件**: **共有の契約スイートが DynamoDB Local に対して全部通る**
      (`cargo test --no-default-features --features aws`)。
- [ ] サイズと上限の検証: 1 レコード 400KB を超えないこと(特に複合フィールドを含む値)、
      クエリ 1MB のページ境界
      → **完了条件**: 大きめの値を入れる専用テスト。
- [ ] on-prem 固有テスト(例: `concurrent_requests_do_not_break_the_storage_environment`)を
      AWS 側の同等テスト(原子採番、トランザクションの原子性)に置き換える
      → **完了条件**: 並行作成・並行公開のテストが両バックエンドに存在する。

### P2. S3(画像)

- [ ] `generate_image_upload_url` を **presigned PUT** に、`take_upload_key` の単回トークンを
      **DynamoDB + TTL** に持たせる(現行の「1 回だけ・特定ファイル名に束縛」を維持)
      → **完了条件**: 契約スイートの画像テスト(トークン再利用拒否、別ファイル名への転用拒否)が通る。
- [ ] `read_image_bytes` / `write_image_bytes` を S3 実装にし、1 で決めた配信方式を反映
      → **完了条件**: LocalStack S3 に対して presign → PUT → GET の往復テスト。
- [ ] 配信方式を変える場合は、フロントと E2E の期待値を更新
      → **完了条件**: E2E の画像チェック(アップロード・表示・削除)が staging で通る。

### P3. Lambda 起動点

- [ ] `lambda_http` で既存ルーターを載せる。イベント形状(API Gateway v1/v2)と
      **バイナリ応答(base64)** を確認(画像をプロキシするなら必須)(要確認: `lambda_http` の
      正確な適合関数名・バージョン 1.3.1 の API)
      → **完了条件**: 合成イベントを通すテスト + staging のスモーク。
- [ ] 応答後凍結の対策: Webhook を SQS/EventBridge に載せる(1-3 の決定どおり)
      → **完了条件**: publish が配信完了を待たずに応答し、配信が失われないことを staging で確認。
- [ ] プロセス内状態の棚卸し: ログイン試行のカウンタ(→ DynamoDB か Cognito に寄せる)、
      画像の単回トークン(P2 で DynamoDB 化)、Webhook(P3 で SQS 化)
      → **完了条件**: 「Lambda を 2 インスタンスで動かしても壊れない」項目がゼロになっている。

### P4. Cognito(採用する場合のみ)

- [ ] トークン検証器を抽象化(ローカル HS256 / Cognito RS256 + JWKS)。JWKS 取得は差し替え可能に
      → **完了条件**: テスト内で RSA 鍵を生成して、正常・期限切れ・iss/aud 不一致・別鍵・
      `alg` 混乱・未知の `kid` を単体テストで判定。
- [ ] `User` に `external_id`(`sub`)を追加(`#[serde(default)]`、既存レコードは `None`)
      → **完了条件**: Cognito の `sub` から権限レコードを解決するテスト。
- [ ] パスワード変更・リセット・ロックアウトを Cognito に寄せるか併存させるかを決めて実装
      → **完了条件**: どちらに寄せても、権限(ロール・リソース単位)はローカルのままであることを
      テストで確認。

### P5. デプロイと運用

- [ ] IaC でスタック定義(Lambda + Function URL/API Gateway、DynamoDB、S3、CORS、環境変数、
      シークレットは Secrets Manager)。テスト用の使い捨てスタックも同じ IaC で
      → **完了条件**: 新規アカウント領域に 1 コマンドで作成・削除できる。
- [ ] staging に対して既存 E2E を回す(`BASE_URL` / `ADMIN_USERNAME` / `ADMIN_PASSWORD` を
      向けるだけ。ハーネスは変更不要のはず)
      → **完了条件**: `npm run e2e` が staging で通る(夜間/手動)。
- [ ] IAM 最小権限、ログ/メトリクス、コスト確認(オンデマンド、テストは TTL と接頭辞で掃除)
      → **完了条件**: ドキュメント化。

### P6. 別トピック(今回は対象外)

- 他 CMS からの移行スクリプト(別プロジェクトで HTTP を叩く方針で合意済み)。
  件数が多くて耐えられない場合のみ、**一括作成エンドポイントを CMS 側に足す**。
- `doc/swagger.yaml` の更新。

## 3. テスト方針(要約)

| 層 | ローカル | 実 AWS |
|---|---|---|
| ルーター / ドメイン | `oneshot` + **共有契約スイート**(両バックエンド) | — |
| DynamoDB | DynamoDB Local(`endpoint_url` 差し替え) | GSI の反映遅延、スロットリング、上限 |
| S3 | LocalStack / MinIO(presign → PUT → GET) | IAM、CloudFront、転送 |
| Lambda 起動点 | localhost で `serve` して E2E / `cargo lambda watch` | イベント形状、コールドスタート、凍結 |
| Cognito | 鍵を生成して JWKS を注入(単体) | 実プールの設定 |
| 全体 | — | 使い捨てスタック + 既存 E2E |

エミュレータは **API を真似るが性能特性は真似ない**。だから緑になったローカルスイートを
「AWS で動く証明」とは扱わず、staging の煙テストを残す。

## 4. やらないこと

- オンプレ用の rkv 実装を DynamoDB に合わせて作り替えること(両方は残す。フィーチャーで選ぶ)。
- 移行スクリプトをこのリポジトリの内部 API に結合させること(HTTP のみ)。
- エミュレータだけで完結させること(実 AWS の確認を省く)。
