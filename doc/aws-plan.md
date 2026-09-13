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

## 1. 決めたこと

| 論点 | 決定 |
|---|---|
| 画像の配信 | **S3 の URL**。保存するのは**安定した URL**(CloudFront + OAC、または公開バケット)で、**presigned URL は保存しない**(コンテンツに入ると数分で腐る)。presigned はアップロードの PUT にだけ使う |
| 認証 | **ローカルは現状のまま、AWS は Cognito**。トークン検証を抽象化し(HS256 / RS256 + JWKS)、`sub` → ローカル権限レコードに対応づける |
| アカウント管理 | AWS では**ローカル管理者を置かない**。作成・パスワード変更・リセットは Cognito の担当なので、該当 API は AWS ビルドで **501** を返す。UI は「この配備で何ができるか」で出し分ける |
| Webhook | **予算付きの同期呼び出し**(例: 合計 5 秒)。`Notifier` を async にし、on-prem は spawn のまま、AWS は await する。超過・失敗はログに残して諦める |
| IaC | **Terraform**(他クラウドも視野) |
| 公開の原子性 | **`TransactWriteItems` を使う**(公開コピーへの複製 + 下書き削除 + メタデータ更新) |

### 初回起動(AWS で最初の管理者をどう作るか)

認証を Cognito に移しても、**認可レコード(ロール・リソース単位の権限)はローカルに必要**なので、
最初の 1 人をローカルに作る手段が要る。

- `BOOTSTRAP_ADMIN_USERNAMES`(カンマ区切り)を環境変数で渡す。
- 未プロビジョニングの Cognito ユーザーがログインしてきて、その `username`(または email)が
  このリストにあれば、**その場でローカル管理者レコードを作成**して通す。リストに無ければ
  **403「未プロビジョニング」**(暗黙に権限を与えない)。
- 一度作られれば `external_id = sub` で結び付くので、**リストは初回だけ効く**。以降の追加は
  管理者がローカルレコードを作り、`external_id` は本人の初回ログインで束縛する
  (bind-on-first-use)。
- ユーザーテーブルが空でリストも未設定なら、**起動時に警告**する(on-prem の
  「`ADMIN_USERNAME` を設定しろ」と同じ体験)。
- 前提: Cognito プールは **username を識別子**として使う。`username` の文字集合は既に Cognito に
  合わせてある。
- 代替案とその欠点: Terraform で DynamoDB に直接レコードを書く(内部の JSON 形に結合して壊れ
  やすい)、`sub` を事前に環境変数で渡す(コンソールで調べる手間)、初回ログインで自動 viewer
  作成(暗黙の権限付与)。

### 配備ごとの差を API で表明する

Cognito を入れると「配備によってできることが違う」状態になる。UI の出し分け・E2E の分岐・
ドキュメントの記述が散らばらないよう、`GET /auth/capabilities`(または `/auth/me` の項目)で
**この配備で何ができるか**を返す。例: `{"local_accounts": true/false, "password_change":
true/false, "password_reset_links": true/false}`。

## 2. TODO

各項目に「完了条件(= 何で確認するか)」を付ける。

### P0. 土台(AWS アダプタ無しでも進められる)

- [x] 画像のバイト読み書きを共有トレイトから外した(`LocalImageBytes`)。共有層は「どの
      バックエンドも実装する義務があるもの」だけを語るようになり、ルート `GET/PUT
      /images/{file_name}` はローカルのフィーチャーでだけ登録される
      → 確認: 229 テスト + ブラウザ E2E 59/59(画像のアップロード/配信は E2E が実経路で見ている)。
- [x] テスト用リポジトリ構築をバックエンド別ヘルパー(`on_premises::open_test_repository`)に
      集約し、統合テストをバックエンドのフィーチャーでゲートした
      → 残り: AWS 側のヘルパーができたらゲートを `any(on-premises, aws)` に広げる。
- [x] `docker-compose.yml`(DynamoDB Local + S3 エミュレータ)
      → **確認済み**。DynamoDB Local で CreateTable → `UpdateItem ADD`(原子カウンタ。1 → 2 を確認)
      → ListTables → DeleteTable。S3 は **MinIO** に presign した PUT / GET / DELETE を実際に往復
      (改竄した署名は 403 = エミュレータが SigV4 を本当に検証している)。
      **LocalStack は不採用**: 現在のイメージはライセンストークンが無いと起動しない
      (`License activation failed!`, exit 55)。開発者や CI がアカウントを持つ前提になってしまう。
      MinIO はトークン不要で SigV4 を検証する。DynamoDB Local は実サービス同様なんらかの資格情報を
      要求するので、SDK には `AWS_ACCESS_KEY_ID=test` / `AWS_SECRET_ACCESS_KEY=test` を渡す。
- [x] テストを 1 コマンドにした(`scripts/test-rust.sh` / `test-frontend.sh` / `test-e2e.sh`)と
      CI(`.github/workflows/ci.yml`: rust / frontend / e2e の 3 ジョブ)
      → 確認: スクリプトは手元で実行して確認済み。**ワークフロー自体はこの環境にランナーが
      無いため未検証**。
- [x] `src/aws.rs` を追加し、「未実装」の `compile_error!` をそこへ移した(「1 つだけ選べ」
      「0 個は駄目」のガードは `main.rs` のまま)
      → 確認: 既定ビルドは通り、`--no-default-features` / `--no-default-features --features aws`
      はそれぞれ想定のメッセージで拒否される。
- [x] AWS 用の設定を整理した。`Config` が `AWS_REGION` / `DYNAMODB_TABLE` / `S3_BUCKET` /
      `COGNITO_USER_POOL_ID` / `BOOTSTRAP_ADMIN_USERNAMES` を読み、`aws_settings()` が
      「起動に必要なもの」を検証して最初の不足を名指しする。JWKS の URL は region とプール ID から
      **導出**する(食い違えない)。`DATA_ROOT` と `ADMIN_*` は on-premises 専用であることを
      コメントで明示(AWS で `ADMIN_*` を設定していれば起動時に警告する)
      → 確認: `parse_usernames` のテスト + `aws_settings()` のテスト(不足項目のエラーと JWKS URL)。
- [x] `Notifier::notify` を **await する**形にした(オブジェクト安全を保つため boxed future)。
      on-prem の実装は従来どおり裏で spawn して即座に完了し、AWS 側は await できる
      → 確認: 既存の Webhook テスト(配信・再試行・切断時の挙動)がすべて通る。
- [x] `aws.rs` に起動点の形(`aws::run`)を書き、`main.rs` の AWS 側の腕から呼ぶようにした。
      設定検証と「`ADMIN_*` は無視される」「`BOOTSTRAP_ADMIN_USERNAMES` が空」の警告まで
      → 確認: ガードを一時的に外して `cargo test --no-default-features --features aws` を実行し、
      **AWS 側のコードがコンパイルでき 181 件が通る**ことと、`aws_settings` のテストが動くことを
      確認した(その後ガードは戻し、`--no-default-features --features aws` が拒否されることも再確認)。

### P1. DynamoDB アダプタ(本丸)

- [x] **単一テーブルのキー設計**を決めて [`doc/aws-dynamodb-design.md`](aws-dynamodb-design.md) に
      書いた。1 テーブル + `PK`/`SK`、**GSI なし**(一覧はすべてパーティションクエリ)、値は JSON 文字列、
      読み取りは `ConsistentRead`、アイテム ID はゼロ埋め、ユーザー名の一意性は予約アイテム +
      条件付き書き込み、公開は `TransactWriteItems`
      → 確認: 実装が必要とするアクセスパターンを表で網羅(44 メソッド分)。
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
- [ ] S3 実装は `ImageRepository`(レコード CRUD + presigned PUT)だけを満たせばよい。
      バイトの読み書きは `LocalImageBytes` に分離済みなので、AWS アダプタに
      「呼ばれないメソッドのダミー実装」は要らない
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

### P4. Cognito(採用する。§1 の決定どおり)

- [ ] トークン検証器を抽象化(ローカル HS256 / Cognito RS256 + JWKS)。JWKS 取得は差し替え可能に
      → **完了条件**: テスト内で RSA 鍵を生成して、正常・期限切れ・iss/aud 不一致・別鍵・
      `alg` 混乱・未知の `kid` を単体テストで判定。
- [ ] `User` に `external_id`(`sub`)を追加(`#[serde(default)]`、既存レコードは `None`)
      → **完了条件**: Cognito の `sub` から権限レコードを解決するテスト。
- [ ] 未プロビジョニングの扱いと初回ブートストラップ(§1「初回起動」)を実装
      → **完了条件**: `BOOTSTRAP_ADMIN_USERNAMES` の 1 人が初回ログインで管理者になり、リスト外の
      未プロビジョニングは 403。2 回目以降は `external_id` で解決する。
- [ ] ローカルのパスワード系(作成・変更・リセットリンク)を AWS ビルドで **501** にし、
      `GET /auth/capabilities` で配備の差を表明する(§1「配備ごとの差を API で表明する」)
      → **完了条件**: UI が capabilities で出し分け、AWS ビルドでは該当画面が出ない。
- [ ] 権限(ロール・リソース単位)はローカルのままであることを確認
      → **完了条件**: Cognito のトークンでローカル権限が効くテスト。

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
| S3 | **MinIO**(presign → PUT → GET。検証済み) | IAM、CloudFront、転送 |
| Lambda 起動点 | localhost で `serve` して E2E / `cargo lambda watch` | イベント形状、コールドスタート、凍結 |
| Cognito | 鍵を生成して JWKS を注入(単体) | 実プールの設定 |
| 全体 | — | 使い捨てスタック + 既存 E2E |

エミュレータは **API を真似るが性能特性は真似ない**。だから緑になったローカルスイートを
「AWS で動く証明」とは扱わず、staging の煙テストを残す。

## 4. やらないこと

- オンプレ用の rkv 実装を DynamoDB に合わせて作り替えること(両方は残す。フィーチャーで選ぶ)。
- 移行スクリプトをこのリポジトリの内部 API に結合させること(HTTP のみ)。
- エミュレータだけで完結させること(実 AWS の確認を省く)。
