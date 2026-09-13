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
- [x] AWS アダプタの置き場を作った。**その後ワークスペース分割で形が変わった**(下記 P7):
      バックエンドは feature ではなく **パッケージ**(`crates/aws`)になり、「1 つだけ選べ」
      ガードも `compile_error!` も無くなった。選ぶのは `-p sl-cms-aws` / `-p sl-cms-on-premises`
      という**ビルド対象の選択**そのものになる。
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
      → **完了**(設定は `crates/aws/src/settings.rs` に移り、`AwsSettings::from_env()` が
      同じ検証をする。`the_jwks_url_is_derived_so_pool_and_region_cannot_disagree` など 4 件)。

### P1. DynamoDB アダプタ(本丸)

- [x] **単一テーブルのキー設計**を決めて [`doc/aws-dynamodb-design.md`](aws-dynamodb-design.md) に
      書いた。1 テーブル + `PK`/`SK`、**GSI なし**(一覧はすべてパーティションクエリ)、値は JSON 文字列、
      読み取りは `ConsistentRead`、アイテム ID はゼロ埋め、ユーザー名の一意性は予約アイテム +
      条件付き書き込み、公開は `TransactWriteItems`
      → 確認: 実装が必要とするアクセスパターンを表で網羅(44 メソッド分)。
- [x] **44 メソッドを実装**(collections 16 / single pages 11 / users 6 / composite fields 4 /
      images 4 + S3)。実装は async で書き、同期トレイトへは `BlockingRuntime` で委譲する
      (経緯と順序は [`doc/aws-dynamodb-design.md`](aws-dynamodb-design.md) §7.1)。
      **HTTP の契約スイートが DynamoDB Local + MinIO に対して全部通る**
      (`cargo test --workspace`、HTTP 契約スイート 43 件 × 2 バックエンドを含む 284 件)。
      実行: `docker compose -f sl_cms/docker-compose.yml up -d` してから上記コマンド。
      エミュレータが無いときはエミュレータが要るテストだけ自分を飛ばす。
      押さえるべき点:
      - アイテム ID は **`UpdateItem` の `ADD` で原子的に採番**(on-prem の `id_counter` 相当。
        並行作成で重複しないこと)。
      - 下書き一覧・メタデータ一覧は **SK の `begins_with` クエリ**(DynamoDB に prefix scan は無い)。
      - 公開は **`TransactWriteItems`** で 3 レコードをまとめる。
      - 一覧のページングは `Limit` / `ExclusiveStartKey` に押し下げる
        → 配信 API は達成(下の「公開一覧のページング押し下げ」)。管理画面の一覧は
        2 つのキー空間のマージが要るので残っている。
      - 条件付き書き込みで「存在しないアイテムの削除」等を冪等にする(rkv 実装が握っている
        挙動と同じ結果にする)。
      → **完了条件**: **共有の契約スイートが DynamoDB Local に対して全部通る**
      → **達成**(上記 231 件。DynamoDB Local + MinIO に対して実行)。
- [x] サイズの検証: 300KB の値は往復し、400KB を超える値は**拒否される**
      (`a_large_value_round_trips_and_an_oversized_one_is_refused`)。画像のバイトを S3 に
      出したことが、1 レコード 400KB に収まる根拠になっている。
- [x] 並行作成の検証: 8 スレッド × 5 件で **id が 1..=40 で重複しない**
      (`concurrent_creates_do_not_share_an_id`)。`UpdateItem ADD` の原子性そのもの。
- [x] クエリ 1MB のページ境界: 10KB × 120 件(1MB 超)を一覧して**全件が一度ずつ**返ること。
      ラウンドトリップ回数も数えて、境界を実際に跨いだことをテスト自身が確かめる
      (`a_list_longer_than_one_query_page_is_read_to_the_end`)。
- [x] 公開の原子性: `apply_item_status` / `apply_page_status` を足し、AWS は
      `TransactWriteItems` 1 回、on-prem は rkv の write トランザクション 1 回で
      「公開コピー・作業コピー・状態」をまとめて書く。400KB を超える作業コピーで
      トランザクションを失敗させ、**3 レコードとも元のまま**であることをテストした。
- [x] **公開一覧のページング押し下げ**(`list_published_items_page`): 配信 API の一覧は
      **状態レコード(`meta#`)を id 順に走査**し、窓に入る分だけ本文(`item#`)を読む。
      全件の本文を読んでから絞るのをやめたので、1 万件のコレクションの 1 ページ目は
      「1 万件の小さい状態 + 1 ページ分の本文」で済む。`total` は状態の走査で数えるので
      `X-Total-Count` と `next_offset` は今までどおり。
- [ ] 管理画面の一覧(`get_collection_items_page`)の押し下げ: こちらは
      **公開コピーと作業コピーの和集合**(編集画面は作業コピーを見る)なので、
      2 つのキー空間を id 順にマージする必要がある。配信 API と違って認証済みの管理画面で、
      件数もコレクションの規模に収まるため、後回しにした(設計は同じ「窓をストレージに渡す」)。
- [ ] 配信 API を**カーソル方式**(`LastEvaluatedKey` をそのまま次ページの鍵にする)にするか。
      いまは offset/limit のままなので、`total` を出すために状態レコードを最後まで走査する。
      カーソルにすると `total` を諦める代わりに走査も消えるが、**API の形が変わる**
      (`doc/content-api.md` §3.1 の余地)。

### P2. S3(画像)

- [x] `generate_image_upload_url` を **presigned PUT** にした(有効期限 15 分)。
      単回トークンは on-prem のローカル PUT 経路のための仕組みで、S3 では不要になった:
      署名が「1 回だけ・特定のキーだけ」を担う(終了後に PUT しても 403)。
      `take_upload_key` / `read_image_bytes` / `write_image_bytes`(`LocalImageBytes`)は
      on-prem だけの契約なので、AWS アダプタには要らない(`Storage` の cfg 分岐もそのまま)。
      → **完了条件**: presign → PUT → 公開 URL で GET → 削除の往復テスト
      → **達成**(`aws::repository::images` の往復テスト。MinIO に対して実行)。
- [x] 保存する URL は**安定 URL**(`AWS_IMAGE_BASE_URL` > S3 エンドポイントの path-style >
      `https://<bucket>.s3.<region>.amazonaws.com/<key>`)。**presigned URL はコンテンツに
      入れない**(ページに載った瞬間に期限切れになる)。署名が漏れていないこともテストで確認。
- [ ] 配信は CloudFront にするか、バケットを公開読み取りにするか(Terraform 側で決める)
      → **完了条件**: staging で画像が表示され、E2E の画像チェックが通る。

### P7. ワークスペース分割(2026-09、完了)

feature での切り替えは「1 ビルド = 1 feature 集合」なので、共有層に `#[cfg]` が漏れる
(`Storage` の二重定義、画像ルートの出し分け、テスト側の型エイリアスと `#![cfg]`)うえ、
**1 コマンドで両バックエンドをテストできない**。境界をパッケージに移した:

| パッケージ | 中身 | バイナリ |
|---|---|---|
| `crates/core` | models / repositories / services / http / auth / config / webhook | — |
| `crates/on-premises` | rkv + ローカル画像 | `sl-cms` |
| `crates/aws` | DynamoDB + S3(設定もここ) | `sl-cms-aws` |
| `crates/tests` | 契約スイート(両アダプタに依存) | — |

- **core に `cfg(feature = ...)` は 1 つも無い**。能力は feature ではなく**トレイト**と
  **合成**で表す: `LocalImageBytes` は普通のトレイト、バイトを扱うルートは
  `http::local_images` にあり、各バックエンドの `build_router` が載せるかどうかを決める。
- 片側だけのビルドは `cargo build -p sl-cms-aws`(rkv をコンパイルしない。依存クレートは
  519 ↔ 946)。`default-members` で素の `cargo build` / `cargo test` は on-premises のまま。
- 契約スイートは `suite.rs` を 2 つのランナーが `include!` する形で、**1 コマンドで両方**に
  対して走る(43 × 2)。バイトを扱う 3 件は `Backend::SERVES_IMAGE_BYTES` で自分を飛ばす。
- AWS の契約スイートは**エミュレータが無ければ失敗する**(黙って通らない)。
  `scripts/test-rust.sh` がポートを見てファイルごとスキップし、CI の `rust-aws` ジョブは
  `docker-compose.yml` からエミュレータを起動して本気で走らせる。

### P8. 保存層を async にそろえる(2026-09、完了)

`doc/aws-dynamodb-design.md` §7 の **A**。トレイト 6 つ 48 メソッドを
`impl Future + Send` にし、サービス・ハンドラ・テストを async に、**AWS のブリッジを削除**した。

- core のサービスは `async fn`、on-prem の実装は「await しない async fn」。
- `crates/aws/src/bridge.rs` は無くなり、AWS 実装は素の `async fn`。
- 変換は rustc の提案 span から機械的に適用(`sl_cms/scripts/migration/`)。
- **契約スイート 43 件 × 2 バックエンドは変わらず緑**(`cargo test --workspace` で 281 件)。
  AWS 側の実行時間は 15 秒 → 3.5 秒になった(呼び出しごとのスレッド往復が消えたため)。

### P3. Lambda 起動点

- [x] `lambda_http` で既存ルーターを載せた(`crates/aws/src/lambda.rs`)。
      `lambda_http::run(service_fn(...))` に、ルーターを `tower` サービスとして渡す形。
      イベント形状は `lambda_http::request::LambdaRequest`(公開だが doc-hidden)へ
      **本物と同じ経路でデシリアライズ**してから `http::Request` に変換するので、
      API Gateway **REST(1.0)/ HTTP API(2.0)** の両方を合成イベントでテストしている。
      - **base64**: `isBase64Encoded: true` の本文が復号されてルーターに届くことをテスト
        (`a_base64_body_arrives_as_the_bytes_it_stands_for`)。応答は `Body::Binary` で返すので、
        API Gateway 側で base64 として扱われ、Content-Type ごとそのまま戻る。
      - **6MB/base64**: Lambda の呼び出し上限は 6MB で、API Gateway がバイナリ本文を base64 に
        すると 1/3 増える。CMS のリクエストは JSON(画像は presigned S3 直行)なので
        **4MB を上限**として `MAX_BODY_BYTES` で先に 413 を返す(理由を本文に書く)。
        テスト: `a_body_over_the_lambda_limit_is_refused_with_a_reason`。
      - ローカル起動(`AWS_LAMBDA_RUNTIME_API` が無いとき)も同じルーターで提供し、
        `run_local` が無ければテーブルを作る。実機確認: エミュレータに対して
        `cargo run -p sl-cms-aws` → `/` 200、無認証は 401、テーブル自動作成のログ。
      → **完了条件**: 合成イベントを通すテスト + staging のスモーク
      → テストは達成(4 件)。**staging のスモークは P5(Terraform)待ち**。
- [ ] 応答後凍結の対策: Webhook を SQS に載せる(1-3 の決定どおり)。`Notifier` は既に
      boxed future を返す形なので、AWS 用に `SqsNotifier` を足して `build_notifier` を
      バックエンドごとに選ぶ形にする。テストには SQS のエミュレータが要る(ElasticMQ を
      `docker-compose.yml` に足すか、`Notifier` の単体テストだけで済ませるかの判断が要る)。
      Terraform 側に queue + Lambda の publish 権限 + 配信 Lambda が要る(P5)。
      → **完了条件**: publish が配信完了を待たずに応答し、配信が失われないことを staging で確認。
- [~] プロセス内状態の棚卸し(2026-09 時点):

      | 状態 | いまの置き場 | Lambda で 2 インスタンスだと | 行き先 |
      |---|---|---|---|
      | ログイン試行のカウンタ(`auth/throttle.rs`) | プロセスの `HashMap` | **AWS では持たない**(サインインは Cognito。CMS は試行を見ない) | **決定: on-prem 専用**(下の P4「ログイン試行回数の扱い」) |
      | 画像の単回トークン | AWS では**存在しない**(presigned S3) | 問題なし | — |
      | Webhook | リクエスト処理中に配信 | **失われる**(応答後に実行環境が凍結されうる) | 次の項目の SQS |
      | 発行器(token / preview / reset) | 秘密鍵から導出、状態なし | 問題なし | — |
      | rkv の `Manager` シングルトン | on-prem 専用 | 対象外 | — |

      → **完了条件**: 「Lambda を 2 インスタンスで動かしても壊れない」項目がゼロ。
      → 残りは上の 2 つ(Webhook とログイン試行)。

### P4. Cognito(採用する。§1 の決定どおり)

#### パスワード認証の置き場所(2026-09 完了)

「資格情報は資格情報を持つ配備のもの」という線で分けた。`User` は身元と権限だけを持ち、
パスワードは on-prem のアダプタが別レコードとして保存する。AWS はパスワードを持たないので、
その能力がクレートグラフとルート表の両方から消える(501 のスタブだけが残る)。

残り: **UI が capabilities を読んで出し分ける**こと(Angular 側)と、P4 の
「Cognito ユーザーの作成・削除・パスワード再設定を CMS のアカウント管理から呼ぶ」こと。

#### ログイン試行回数の扱い(2026-09 決定: **A を採用**)

**AWS では回数制限を持たない。** サインインはブラウザと Cognito の間で完結し、CMS は
JWT を検証するだけなので、CMS は試行そのものを見ない。Cognito も**失敗回数を返す API を
持たない**(返すのは `TooManyFailedAttemptsException` という状態だけで、回数・期間は
設定できない)。threat protection は Plus プランのリスクスコアリングで、ドキュメントが
「流量は見ていない、DDoS には AWS WAF を付けろ」と明言している。失敗で発火するトリガも無い
(`PreAuthentication` は試行時に呼ばれるが成否は渡されず、既定では存在しないユーザーでは
呼ばれない)。

| | on-premises | AWS |
|---|---|---|
| 失敗回数のカウンタ | **`auth/throttle.rs`**(5 回/15 分、429 + `Retry-After`、識別子の存在を隠す) | **持たない**(Cognito のロックアウトに委ねる) |
| 存在しない識別子の隠蔽 | 同一メッセージ + 全識別子を数える | `PreventUserExistenceErrors: ENABLED`(アプリクライアント設定) |
| 大量アクセス | プロセス内カウンタ | **AWS WAF のレートベースルール**(P5 の Terraform) |
| パスワード系 API | 使える | `/auth/login` など **501** + capabilities で表明 |

したがって **DynamoDB のカウンタも、Cognito からの回数取得も作らない。** 二重に持つと
「どちらで弾かれたか」が説明できなくなる。サーバー側で `AdminInitiateAuth` して自前で数える案
(B)は、MFA や `NEW_PASSWORD_REQUIRED` のチャレンジフローを実装する代償に見合わないと判断した。

- [x] トークン検証器を抽象化した(2026-09)。
      `auth/identity.rs` の `TokenVerifier` が `Identity`(Local = 自前 HS256 / External =
      プロバイダの `sub`)を返し、`auth/cognito.rs` が RS256 + JWKS を検証する。鍵の取得は
      `JwksSource` の背後(本番は HTTPS、テストは固定の鍵セット)で、キャッシュ
      (10 分 + 未知の `kid` でのみ再取得)は検証器側にある。
      → **完了条件**: 正常・期限切れ・iss 不一致・aud 不一致・別鍵・`alg` 混乱(HS256 で
      公開鍵を秘密鍵として署名)・未知の `kid`・`token_use` が access・`exp` 無し・鍵セット
      不達 → **11 件のテストで達成**。テスト用の RSA 鍵は `auth/test_rsa_key*.pem` /
      `test_jwks.json`(テスト専用。本番は検証だけなので秘密鍵を持たない)。
      期限の余裕(`leeway`)は **10 秒**に固定した(既定は 60 秒で、本来切れているトークンが
      1 分生き残る)。
      **まだ配線していない**: `AuthService::user_from_token` は今も自前トークンだけを見る
      (次の `external_id` の項目でつなぐ)。
- [ ] `User` に `external_id`(`sub`)を追加(`#[serde(default)]`、既存レコードは `None`)
      → **完了条件**: Cognito の `sub` から権限レコードを解決するテスト。
- [ ] 未プロビジョニングの扱いと初回ブートストラップ(§1「初回起動」)を実装
      → **完了条件**: `BOOTSTRAP_ADMIN_USERNAMES` の 1 人が初回ログインで管理者になり、リスト外の
      未プロビジョニングは 403。2 回目以降は `external_id` で解決する。
- [x] ローカルのパスワード系を **配備ごとの能力**にした(2026-09)。
      - 資格情報は `User` レコードから出て、**on-prem のアダプタが持つ**
        (`repositories::local_credentials::LocalCredentials` を `crates/on-premises` が実装。
        Argon2id と rkv の `credential` DB)。**core は argon2 に依存しない**——`User` に
        パスワード欄が無いので、デバッグ出力やログから漏れることも構造上ありえない。
      - パスワード系のルートは `http::password_auth`(`local_images` と同じ形)にあり、
        **on-prem の `build_router` だけが合成**する。
      - AWS は同じパスを **501** で登録する(404 だと「URL が違う」と読まれるため)。
        メッセージは Cognito を名指しする。
      - `GET /auth/capabilities` を両方が公開: `password_login` / `password_reset_links` /
        `image_upload`(`proxied` | `presigned`)。
      - 契約スイートは `Backend::PASSWORD_LOGIN` でパスワード系 6 件をスキップし、AWS では
        アカウント作成とトークン発行をハーネスが直接行う(権限のテストは両方で走る)。
      → **完了条件**: UI が capabilities で出し分け、AWS ビルドでは該当画面が出ない
      → テストは達成(45 件 × 2、capabilities と 501 のテストを含む)。
        **UI 側の出し分けは未着手**(下の「残り」)。
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
