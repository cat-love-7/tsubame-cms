# AWS の決定録

この CMS を AWS(Lambda + DynamoDB + S3)で動かすと決めたときの判断と、実装して分かったこと。
スタックそのものは [`infra/README.md`](../infra/README.md)、テーブル設計は
[`aws-dynamodb-design.md`](aws-dynamodb-design.md)、動かし方は [README](../README.md) にある。

**実装は完了しており、staging で動いている**(2026-09)。§2 の `[x]` は「そう決めた」ではなく
「そう作り、こう確認した」の記録として残している — 同じ判断をやり直す人にとって、確認の方法まで
書いてある方が役に立つ。まだ終わっていないものだけが §2 の末尾「残っている課題」にある。

## 0. いまの形

- バックエンドは**クレートで分かれている**(コンパイル時のフィーチャー選択ではない):
  `crates/core`(モデル・サービス・HTTP 層)+ `crates/on-premises`(rkv とローカル画像)+
  `crates/aws`(DynamoDB・S3・Lambda)。差し替え口は `AppModule<R: Storage>` と
  `http::router`, `crates/tests` が**同じ契約スイートを両方に対して**走らせる。
- `crates/aws` には 5 つのリポジトリ実装、DynamoDB のヘルパー、S3 のアップロード先、
  **Lambda の起動点**(`lambda.rs`)、Cognito のアカウント管理(`provisioner.rs`)が入っている。
- リポジトリのトレイトは async。かつての「同期トレイトをランタイムスレッドで包む」ブリッジは
  無くなった(`aws-dynamodb-design.md` §7.1)。
- エミュレータ(DynamoDB Local + versitygw)は `docker-compose.yml`、配備は `infra/`(Terraform)、
  テストは `scripts/test-rust.sh` が 1 コマンド。
- プロセス内メモリに依存していた 3 つ(ログイン試行のカウンタ、画像アップロードの単回トークン、
  Webhook のバックグラウンド送信)は、AWS 側でそれぞれ DynamoDB のレコード、S3 の presigned PUT、
  予算付きの同期送信に置き換えた。§1 に決定、残る穴は §2 の「残っている課題」。

## 1. 決めたこと

| 論点 | 決定 |
|---|---|
| 画像の配信 | **S3 の URL**。保存するのは**安定した URL**(CloudFront + OAC、または公開バケット)で、**presigned URL は保存しない**(コンテンツに入ると数分で腐る)。presigned はアップロードの PUT にだけ使う |
| 認証 | **ローカルは現状のまま、AWS は Cognito**。トークン検証を抽象化し(HS256 / RS256 + JWKS)、`sub` → ローカル権限レコードに対応づける |
| アカウント管理 | 作成・削除・リセット・有効/無効は**どちらの配備でも管理者の仕事**なので、ルートは共有(`http::auth`)。違うのは**渡すもの**: CMS がパスワードを持つ配備はリンク、Cognito が持つ配備は `AdminSetUserPassword` の**一時パスワード**。`GET /api/auth/capabilities` の `password_reset` が事前に、応答の `kind` が直前に言う。CMS 自身のパスワード経路(`/auth/login`、`/auth/password-reset`、`/auth/me/password`)だけが AWS ビルドで **501** |
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
ドキュメントの記述が散らばらないよう、`GET /api/auth/capabilities`(または `/api/auth/me` の項目)で
**この配備で何ができるか**を返す。例: `{"password_login": true/false, "password_reset":
"link"|"temporary", "image_upload": "proxied"|"presigned", "login_url": …}`。

## 2. 実装の記録

各項目に「完了条件(= 何で確認するか)」を付けてある。`[x]` は実装して確認済み、`[ ]` は残っている。

### 土台(AWS アダプタ無しでも進められる)

- [x] 画像のバイト読み書きを共有トレイトから外した(`LocalImageBytes`)。共有層は「どの
      バックエンドも実装する義務があるもの」だけを語るようになり、ルート `GET/PUT
      /api/images/{file_name}` はローカルのフィーチャーでだけ登録される
      → 確認: 229 テスト + ブラウザ E2E 59/59(画像のアップロード/配信は E2E が実経路で見ている)。
- [x] テスト用リポジトリ構築をバックエンド別ヘルパー(`on_premises::open_test_repository`)に
      集約し、統合テストをバックエンドのフィーチャーでゲートした
      → 残り: AWS 側のヘルパーができたらゲートを `any(on-premises, aws)` に広げる。
- [x] `docker-compose.yml`(DynamoDB Local + S3 エミュレータ)
      → **確認済み**。DynamoDB Local で CreateTable → `UpdateItem ADD`(原子カウンタ。1 → 2 を確認)
      → ListTables → DeleteTable。S3 はエミュレータに presign した PUT / GET / DELETE を実際に往復
      (改竄した署名は拒否 = エミュレータが SigV4 を本当に検証している)。
      **LocalStack は不採用**: 現在のイメージはライセンストークンが無いと起動しない
      (`License activation failed!`, exit 55)。開発者や CI がアカウントを持つ前提になってしまう。
      DynamoDB Local は実サービス同様なんらかの資格情報を要求するので、SDK には
      `AWS_ACCESS_KEY_ID=test` / `AWS_SECRET_ACCESS_KEY=test` を渡す。

      **2025-10 追記: MinIO から versitygw へ。** MinIO はオープンソース版のサーバー・クライアント・
      イメージをアーカイブし、配布を終了した(Docker Hub は 404、quay.io は 401、`dl.min.io` は
      アーカイブ一覧まで 410。公式の告知どおり「セキュリティ更新も脆弱性報告も受けない」)。
      コミュニティのミラーを CI に pin すると、更新の無いソフトウェアを検証もできずに動かすことになる。
      代わりに **versitygw**(Apache-2.0、いまもリリースされている S3 ゲートウェイ)に替えた:
      SigV4 を検証し、`--iam-dir` のユーザー + **バケットポリシー**で「配備と同じ権限のユーザー」を
      作れるので、権限に依存するテストはそのまま意味を保っている(`deployment_bucket_policy`)。
- [x] テストを 1 コマンドにした(`scripts/test-rust.sh` / `test-frontend.sh` / `test-e2e.sh`)と
      CI(`.github/workflows/ci.yml`: rust / frontend / e2e の 3 ジョブ)
      → 確認: スクリプトは手元で実行して確認済み。**ワークフロー自体はこの環境にランナーが
      無いため未検証**。
- [x] AWS アダプタの置き場を作った。**その後ワークスペース分割で形が変わった**(下記 P7):
      バックエンドは feature ではなく **パッケージ**(`crates/aws`)になり、「1 つだけ選べ」
      ガードも `compile_error!` も無くなった。選ぶのは `-p tsubame-aws` / `-p tsubame-on-premises`
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

### DynamoDB アダプタ

- [x] **単一テーブルのキー設計**を決めて [`docs/aws-dynamodb-design.md`](aws-dynamodb-design.md) に
      書いた。1 テーブル + `PK`/`SK`、**GSI なし**(一覧はすべてパーティションクエリ)、値は JSON 文字列、
      読み取りは `ConsistentRead`、アイテム ID はゼロ埋め、ユーザー名の一意性は予約アイテム +
      条件付き書き込み、公開は `TransactWriteItems`
      → 確認: 実装が必要とするアクセスパターンを表で網羅(44 メソッド分)。
- [x] **44 メソッドを実装**(collections 16 / single pages 11 / users 6 / composite fields 4 /
      images 4 + S3)。実装は async で書き、同期トレイトへは `BlockingRuntime` で委譲する
      (経緯と順序は [`docs/aws-dynamodb-design.md`](aws-dynamodb-design.md) §7.1)。
      **HTTP の契約スイートが DynamoDB Local + MinIO に対して全部通る**
      (`cargo test --workspace`、HTTP 契約スイート 43 件 × 2 バックエンドを含む 284 件)。
      実行: `docker compose -f backend/docker-compose.yml up -d` してから上記コマンド。
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
      (`docs/content-api.md` §3.1 の余地)。

### S3(画像)

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
- [x] **署名付き配信を配備の選択肢にした**(2026-09、`AWS_IMAGE_DELIVERY`)。
      `public`(既定)= オブジェクトのアドレスを配る + バケットは公開読み取り。
      `presigned` = **バケットは非公開**で、API が返す URL は毎回**署名付き GET**
      (`AWS_IMAGE_URL_TTL_SECONDS`、既定 1 時間、60 秒〜7 日)。
      **向いているのは「ビルド時に取得して自前で配信する」サイト**(SSG が画像を変換して自分の
      コピーを配る場合): 署名はビルドの間だけ生きていればよく、バケットは誰も読めない。
      逆に、ページに CMS の URL をそのまま載せるサイトでは**キャッシュした URL が期限切れで
      死ぬ**ので既定にはしない。`/api/images/by-id/{id}` の転送も同じ経路を通るので、モードに
      かかわらず動く(手書きの Markdown リンクはそのまま使える)。
      → **完了条件**: 署名モードで (1) API の URL が署名付き、(2) その URL で実体が取れる、
      (3) **署名なしでは取れない** → `a_presigned_deployment_serves_a_url_that_expires` で達成。
- [x] **アップロード情報に安定 URL を載せた**(`NewImageInfo.url`)。S3 の
      presigned URL は「署名」であって object の公開アドレスではないので、クライアントが
      `upload_url` から導出するのは AWS では不可能だった(on-prem は `?key=` を落とすだけで
      済んでいた)。API が返すようにしたので、フロントは両配備で同じ経路を通る。
      インターセプタも**外部ホストには bearer を付けない**(S3 への PUT に `Authorization` を
      足すと署名不一致になりうるし、S3 の 401 でサインアウトするのは誤り)。
- [x] 配信は CloudFront にするか、バケットを公開読み取りにするか(Terraform 側で決める)
      → **決定: 公開読み取り**(`storage.tf` のバケットポリシーが `s3:GetObject` を `*` に許す)。
      理由は下の「保存する URL は安定 URL」と同じ: コンテンツは**アドレスを保存**し、サイトの
      ビルドが**後から**取りに行く。署名付き URL は最長 7 日で切れるので、静的サイトの HTML に
      載った時点で死んだリンクになる。CloudFront を前に置く場合も、**署名ではなく**
      オリジンとして読む前提(`AWS_IMAGE_BASE_URL` を CDN に向けるだけで、CMS もコンテンツも
      変更不要)。**残り**: staging で表示確認(E2E は on-prem の配信経路で通っている)。

### ワークスペース分割(2026-09)

feature での切り替えは「1 ビルド = 1 feature 集合」なので、共有層に `#[cfg]` が漏れる
(`Storage` の二重定義、画像ルートの出し分け、テスト側の型エイリアスと `#![cfg]`)うえ、
**1 コマンドで両バックエンドをテストできない**。境界をパッケージに移した:

| パッケージ | 中身 | バイナリ |
|---|---|---|
| `crates/core` | models / repositories / services / http / auth / config / webhook | — |
| `crates/on-premises` | rkv + ローカル画像 | `tsubame` |
| `crates/aws` | DynamoDB + S3(設定もここ) | `tsubame-aws` |
| `crates/tests` | 契約スイート(両アダプタに依存) | — |

- **core に `cfg(feature = ...)` は 1 つも無い**。能力は feature ではなく**トレイト**と
  **合成**で表す: `LocalImageBytes` は普通のトレイト、バイトを扱うルートは
  `http::local_images` にあり、各バックエンドの `build_router` が載せるかどうかを決める。
- 片側だけのビルドは `cargo build -p tsubame-aws`(rkv をコンパイルしない。依存クレートは
  519 ↔ 946)。`default-members` で素の `cargo build` / `cargo test` は on-premises のまま。
- 契約スイートは `suite/`(話題ごとのファイル + `mod.rs` のハーネス)を 2 つのランナーが
  `#[path]` でモジュールとして取り込む形で、**1 コマンドで両方**に対して走る(71 × 2)。
  バイトを扱うテストは `Backend::SERVES_IMAGE_BYTES` で自分を飛ばす。
- AWS の契約スイートは**エミュレータが無ければ失敗する**(黙って通らない)。
  `scripts/test-rust.sh` がポートを見てファイルごとスキップし、CI の `rust-aws` ジョブは
  `docker-compose.yml` からエミュレータを起動して本気で走らせる。

### 保存層を async にそろえる(2026-09)

`docs/aws-dynamodb-design.md` §7 の **A**。トレイト 6 つ 48 メソッドを
`impl Future + Send` にし、サービス・ハンドラ・テストを async に、**AWS のブリッジを削除**した。

- core のサービスは `async fn`、on-prem の実装は「await しない async fn」。
- `crates/aws/src/bridge.rs` は無くなり、AWS 実装は素の `async fn`。
- 変換は rustc の提案 span から機械的に適用(使い捨ての道具は移行後に削除。教訓は
  `docs/aws-dynamodb-design.md` §7)。
- **契約スイート 43 件 × 2 バックエンドは変わらず緑**(`cargo test --workspace` で 281 件)。
  AWS 側の実行時間は 15 秒 → 3.5 秒になった(呼び出しごとのスレッド往復が消えたため)。

### Lambda の起動点

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
      - **応答側も同じ計算**: バッファ応答の上限は 6MB(バイナリ扱いなら base64 で 1/3 増)なので、
        応答が `MAX_RESPONSE_BYTES`(4MB)を超えないよう、配信 API は**バイト数でもページを切る**
        (`http::content`)。それ以外の経路(全件取得の管理一覧など)は `lambda.rs` のガードが
        CMS の 413 で答える。件数だけで切っていると 50 件 × 400KB で関数ごと失敗し、
        クライアントには 502 しか届かない。
      - **応答ストリーミングは今は採らない(2026-09 決定)**: `InvokeMode: RESPONSE_STREAM` にすると
        応答上限は 6MB → **200MB** になる(最初の 6MB は無制限、以降 2MB/s、クライアントが
        切断しても実行時間は最後まで課金、コンソールでは常にバッファ表示)。Rust でも
        `lambda_http::run_with_streaming_response` で対応できるが、**うちの応答は axum の `Json` で
        最後まで組み立ててから返す**ので TTFB は改善せず、天井が上がるだけ。大きい応答が要るのは
        「全件取得の管理一覧」やエクスポートで、**作りながら流す API を作るとき**
        (NDJSON など)に `invoke_mode` ごと切り替える。`MAX_BODY_BYTES` のリクエスト側は
        ストリーミングでも変わらない(6MB のまま)。
      - ローカル起動(`AWS_LAMBDA_RUNTIME_API` が無いとき)も同じルーターで提供し、
        `run_local` が無ければテーブルを作る。実機確認: エミュレータに対して
        `cargo run -p tsubame-aws` → `/` 200、無認証は 401、テーブル自動作成のログ。
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

### Cognito(§1 の決定どおり)

#### パスワード認証の置き場所(2026-09 完了)

「資格情報は資格情報を持つ配備のもの」という線で分けた。`User` は身元と権限だけを持ち、
パスワードは on-prem のアダプタが別レコードとして保存する。AWS はパスワードを持たないので、
その能力がクレートグラフとルート表の両方から消える(501 のスタブだけが残る)。

残り: **UI が capabilities を読んで出し分ける**こと(Angular 側)と、P4 の
「Cognito ユーザーの作成・削除・パスワード再設定を CMS のアカウント管理から呼ぶ」こと。

#### ログイン試行回数の扱い(2026-09 決定: **A を採用**)

**AWS では回数制限を持たない。** サインインはブラウザと Cognito の間で完結し、CMS は
JWT を検証するだけなので、CMS は試行そのものを見ない。Cognito も**失敗回数を返す API を
持たない**(`InitiateAuth` の失敗は `NotAuthorizedException` で、回数を問い合わせる手段は無い)。
threat protection は Plus プランのリスクスコアリングで、ドキュメントが
「流量は見ていない、DDoS には AWS WAF を付けろ」と明言している。失敗で発火するトリガも無い
(`PreAuthentication` は試行時に呼ばれるが成否は渡されず、既定では存在しないユーザーでは
呼ばれない)。

**ただし WAF は付けない(2026-09 決定)。** 上の引用に従えば大量アクセス対策は WAF の
レートベースルールだが、守れるのは**サインインの試行だけ**で、CloudFront も `/api` も画像配信も
守れない。その一方でプール側のカテゴリ別クォータ(例: `UserAuthentication` はアカウント・
リージョン全体で 120 RPS)が既に被害の上限を作っている。web ACL はリソースと月額
(約 $5 + ルール $1 + リクエスト $0.60/100 万)を足す割に、今の規模で得るものが無いと判断した。
アカウント単位の防御(リスクスコアリングとロックアウト)は Plus プランの threat protection で、
これは WAF では代替できない。必要になったら CloudFront 側(CLOUDFRONT スコープ、us-east-1)に
足す — そちらは API とアプリの両方を守れる — という別の判断になる。

| | on-premises | AWS |
|---|---|---|
| 失敗回数のカウンタ | **`auth/throttle.rs`**(5 回/15 分、429 + `Retry-After`、識別子の存在を隠す) | **持たない**(Cognito は回数を返さない。Plus の threat protection のみ) |
| 存在しない識別子の隠蔽 | 同一メッセージ + 全識別子を数える | `PreventUserExistenceErrors: ENABLED`(アプリクライアント設定) |
| 大量アクセス | プロセス内カウンタ | **持たない**(Cognito のカテゴリ別クォータに委ねる) |
| パスワード系 API | 使える | `/api/auth/login` など **501** + capabilities で表明 |

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
- [x] `User` に `external_id`(Cognito の `sub`)を追加した(`#[serde(default)]`)。
      `UserRepository::get_user_from_external_id` を足し、on-prem は `identity` という専用 DB
      (`credential` と同じ形)、AWS は `username#` と同じ予約レコード(`external_id#<sub>`)で
      引く。追加・変更・削除はどちらも**索引をレコードと同じトランザクションで**維持し、
      外れた識別子は解放する(古い `sub` がいつまでも誰かに解決してはいけない)。
      `AuthService::user_from_token` は `TokenVerifier` の返す `Identity` で分岐するように
      なった(自前トークン = 世代検査、プロバイダ = `external_id` で解決)。
      → **完了条件**: `sub` から権限レコードを解決するテスト
      → **達成**(`a_provider_identity_is_resolved_through_external_id`)。
- [x] 未プロビジョニングの扱いと初回ブートストラップを実装した。
      `BOOTSTRAP_ADMIN_USERNAMES` の誰かが初回サインインすると、その `sub` に結びついた管理者
      レコードを作る。リスト外は **403**(「組織の誰でもプールにサインインできる = 誰でもサイトを
      編集できる、にはしない」)。2 回目以降は `external_id` で解決する。無効化されたアカウントは
      解決しても 403。
      AWS 側は `COGNITO_CLIENT_ID` を設定に足し、**Lambda 起動経路**(`build_deployed_module`)が
      `CognitoVerifier`(+ `HttpJwks`)を据え、`BOOTSTRAP_ADMIN_USERNAMES` を認証サービスへ渡す。
      **ローカル起動はあえて別の合成**(`build_app_module` = 自前 HS256)を通る: テストは
      `JWT_SECRET` で署名したトークンでサインインし、また**配備では `JWT_SECRET` が
      リセット/プレビューリンクの署名にも使われる**ので、配備が自前トークンを受け入れると
      その秘密を知る者が管理者トークンを鋳造できてしまう。
      → **完了条件**: 配備の合成が自前トークンを拒否すること(401)、検証器がプールを正しく
      指すこと → `deployed_verifier_tests` で達成。
      → Cognito のユーザー作成・削除・リセットは後に配線した(下の P4 の項目、2026-09)。

- [x] **Hosted UI でのサインインを最後まで通した**(2026-09)。
      これまでは `login_url` へのリンクがあるだけで、**Cognito 側に `callback_urls` も
      OAuth の許可も無く**、戻ってきた `code` を交換する経路も無かった。
      → Terraform のクライアントに `callback_urls`(`<app_url>/auth/callback`)・
      `allowed_oauth_flows = ["code"]`・`allowed_oauth_scopes` を設定し、画面が PKCE
      challenge + `state` + `redirect_uri` を足して送り、`/auth/callback`(画面のルート)が state を
      照合して `POST /api/auth/cognito/exchange` に交換を依頼する(交換はサーバー:
      トークンエンドポイントは CORS を返さない)。検証は `CognitoVerifier` が行う。
      → **完了条件**: challenge が RFC 7636 の例と一致、state 不一致・取り消し・交換拒否の
      3 経路が画面に出す → フロントの 8 テストで達成。
      → **完了条件**: リスト内の 1 人が初回ログインで管理者、リスト外は 403、2 回目は
      `external_id` で解決
      → **達成**(同じテストで、作成・再解決・無効化・部外者の 4 点を確認)。
- [x] ローカルのパスワード系を **配備ごとの能力**にした(2026-09)。
      - 資格情報は `User` レコードから出て、**on-prem のアダプタが持つ**
        (`repositories::local_credentials::LocalCredentials` を `crates/on-premises` が実装。
        Argon2id と rkv の `credential` DB)。**core は argon2 に依存しない**——`User` に
        パスワード欄が無いので、デバッグ出力やログから漏れることも構造上ありえない。
      - パスワード系のルートは `http::password_auth`(`local_images` と同じ形)にあり、
        **on-prem の `build_router` だけが合成**する。アカウント作成・リセットは
        `http::auth` の**共有ルート**にある(管理者の仕事はどちらの配備にもあるため)。
      - AWS は残ったパスワード経路(`POST /auth/login`、`POST /auth/password-reset`、
        `POST /auth/me/password`)を **501** で登録する(404 だと「URL が違う」と読まれるため)。
        メッセージは Cognito を名指しする。
      - `GET /api/auth/capabilities` を両方が公開: `password_login` / `password_reset`
        (`link` | `temporary` | null) / `image_upload`(`proxied` | `presigned`) / `login_url`。
      - 契約スイートは `Backend::PASSWORD_LOGIN` でパスワード系 6 件をスキップし、
        `Backend::PASSWORD_RESET` で「渡すもの」の形を断言する。AWS のアカウント作成は
        **本物のルート**を通る(プロバイダだけが偽で、Cognito のエミュレータが無いため)。
      → **完了条件**: UI が capabilities で出し分けること。AWS でも**アカウント画面は出る**
      (作成・削除・リセットは管理者の仕事)が、リンクの代わりに一時パスワードを渡し、
      自分のパスワード変更欄は出ない(そちらは 501 のままで、案内文に置き換わる)
      → テストは達成(契約スイート 46 件 × 2、capabilities と 501 のテストを含む)。
      - [x] **UI 側も対応した**(`CapabilitiesService`)。`GET /api/auth/capabilities` を最初の
        画面が 1 回だけ読み、**答えが来るまでは「パスワード方式」とみなす**(capabilities を
        知らない古いサーバーや、応答が落ちた場合に UI が壊れない)。出し分けは:
        * ログイン画面: `password_login` が false ならフォームの代わりに
          「この配備は ID プロバイダでサインインする」と案内する(501 を返すパスワード欄を
          見せない)。
        * ユーザー一覧: パスワード欄と「パスワード再設定」「リセットリンク発行」を同様に隠す。
        テスト: `CapabilitiesService` の 4 件(既定・反映・1 回だけ・失敗時)と、ログイン画面の
        出し分け 1 件(フロント 121 件が緑)。
        **残り**: 配備が `login_url`(Cognito のホスト UI など)を capabilities で返せば、
        案内文ではなくそこへ誘導できる。いまは行き先を知らないので文面のみ。
- [x] 権限(ロール・リソース単位)は**CMS のもの**であり、Cognito には置かない。
      境界は「Cognito = 身元(パスワード・MFA・セッション)、CMS = 認可(ロール・
      リソース単位の許可)」。`User.permission` / `is_admin` / `is_active` /
      `collection_permissions` / `single_page_permissions` はアカウントレコードにあり、
      `PATCH /api/auth/users/{id}` は**共有ルート**なので AWS でもそのまま使える。
      トークンの中身で判定しない(ミドルウェアは毎回レコードを引く)ので、**権限変更は次の
      リクエストから効く**——トークンの再発行を待たない。Cognito のグループに寄せなかったのは、
      (a) リソース単位の上書き(コレクションごとの 3 ビット)がグループに収まらない、
      (b) 変更の反映がトークン更新に縛られる、(c) 監査と UI はどちらにせよローカルが要る、の 3 点。
      → **完了条件**: Cognito のトークンでローカル権限が効くテスト
      → **達成**: `a_role_change_takes_effect_on_the_next_request`(ビューア → 編集許可 →
      コレクション単位の拒否、を同じトークンで確認。両バックエンドで実行)。

- [x] **Cognito のユーザー管理**(2026-09)。`aws-sdk-cognitoidentityprovider` を足し、
      `crates/aws/src/provisioner.rs` の `CognitoAccountProvisioner` が
      `auth::provisioner::AccountProvisioner` を実装する。作成・削除・リセット・有効/無効が
      プール側にも届き、`AuthService` は**プロバイダを先に**呼ぶ(拒否されたら CMS 側は何も
      変わらない)。継ぎ目は `CognitoAdmin` の 4 呼び出しで、**Cognito のエミュレータが無い**以上
      手元で見られるのは対応付けだけ——だから unit テストは偽のプールで
      「何を・どの順で・どの値で呼ぶか」「拒否は成功に見せない」を確かめる。

      実装で分かった 2 点、どちらも「コードを読むだけでは気づきにくい」もの:

      - **`create` はプール側の識別子(`sub`)を返す**。CMS のトークン解決は `external_id` でしか
        引かないので、管理者が作ったアカウントにこれを記録しないと、本人がサインインしても
        `not_provisioned` で弾かれる(両方に存在するのに、片方から見つけられない)。
        既にプールにある名前なら `AdminGetUser` で拾う。
      - **リセットは `AdminSetUserPassword`(`Permanent=false`)で一時パスワード**。CMS は
        パスワードを持たないので値を作れず、代わりに「次回サインインで変更させる」。
        `AdminCreateUser` は `MessageAction::Suppress`(メールを送らない配備なので招待は届かない)。
        渡す値はリセットが作る——画面はリンクでも一時パスワードでも「渡すものをコピーさせる」
        同じ形で、`password_reset` が事前に、応答の `kind` が直前に言う。

      `POST /api/auth/users` とリセットは**共有ルート**(`http::auth`)になり、501 スタブは
      自分のパスワード変更だけに縮んだ。契約スイートは `Backend::PASSWORD_RESET` で形を断言し、
      AWS 側は in-memory の偽プールで**本物のルート**を通す。

      → **確認(staging、2026-09)**: `frontend/e2e/hosted-accounts.mjs` が実ブラウザで
      作成 → 一時パスワード → **本人としてサインインし変更を強制される** → 削除 → 削除後は
      プールが拒否、まで通す(管理者 `cat` と実 Cognito プールに対して)。

### デプロイと運用

#### 画面の配信(S3 + CloudFront、2026-09)

管理画面は**同じ CloudFront の 1 オリジン**から配る。アプリは S3(非公開 + OAC)、API は
Function URL で、ビヘイビアは 2 つだけ:

| リクエスト | オリジン | キャッシュ |
|---|---|---|
| `/api/*` | Function URL | しない(すべてトークン次第の答え)。`Authorization` は転送する |
| 拡張子のあるパス | S3 | オブジェクトの指定どおり(バンドルは 1 年、`index.html` はしない) |
| それ以外のパス | S3(`/index.html` として) | 画面のルート。アプリは 1 ドキュメント |

- **同一オリジン**なのでアプリから API への CORS は不要で、`apiUrl()` の `/api` を剥がす
  作業も無い(`API_PREFIX` をサーバーが持っている)。プリサインド PUT だけはブラウザから
  S3 へのクロスオリジンなので、画像バケットの CORS には `app_url` のオリジンを常に足す。
- **SPA のディープリンク**はバケットにオブジェクトが無い。写すのは **CloudFront Function
  (default behavior のみ)** で、`/api/*` には付かない。distribution 全体の custom error
  response にすると、**API 自身の 403/404**(公開権限の無い利用者、存在しないアイテム)まで
  `index.html` の 200 に化ける — これは実際に踏むところだったので Function 側に置いた。
- **キャッシュはオブジェクトに決めさせる**: アプリ用のキャッシュポリシーを `min_ttl = 0` で
  作り、`scripts/deploy-frontend.sh` が「内容ハッシュ付きのバンドルは 1 年 immutable、
  名前が変わらないもの(`index.html` / `favicon.ico` / 通知類)は毎回再検証」と指定する。
  マネージドの CachingOptimized は `min_ttl` が 1 時間なので、`index.html` の `no-cache` が
  1 時間に切り上げられてしまう。
- **`app_url` は入力**。配布ドメインから導くと
  `distribution → function URL → function → Cognito client → callback_urls → distribution` の
  循環になる。証明書(us-east-1 の ACM)と DNS レコードも運用者が用意する — JWT secret と
  同じ流儀で、Terraform は「何を指すか」(`terraform output frontend_url`)だけを出す。
- デプロイは `scripts/deploy-frontend.sh`(`ng build` → `aws s3 sync --delete` →
  `/` と `/index.html` を invalidate)。Terraform が持つのはバケット・distribution・
  ポリシーまでで、**中身はビルド成果物**なので IaC に持たせない。

- [x] IaC でスタック定義(Lambda + Function URL、DynamoDB、S3、CORS、環境変数、シークレットは
      Secrets Manager)— **完了**。`infra/` がそれで、`terraform apply` 1 回で作れる。
      API Gateway は挟まない: function URL で足りる。
- [x] staging に対して既存の E2E を回す — **完了**。管理画面のサインインは
      `frontend/e2e/hosted-signin.mjs` / `hosted-accounts.mjs` が実ブラウザで通す。
- [x] IAM 最小権限、ログ/メトリクス、コスト確認 — **完了**。配備用ポリシーは
      `infra/deployer-policy-*.json`(133 アクション)、ログは保持期間つきのロググループ、
      コストは README の「What it costs to run」に、実測値つきで書いてある。

### 残っている課題

- **管理画面の一覧の押し下げ**: 配信 API の一覧は状態レコードを id 順に走査して窓に入る本文だけを
  読む形になったが、管理画面の一覧は公開コピーと作業コピーの**和集合**なので、2 つのキー空間を
  id 順にマージする必要がある。認証済みで件数も規模に収まるため後回しにしている
  (設計は同じ「窓をストレージに渡す」)。
- **配信 API をカーソル方式にするか**: いまは offset/limit で、`total` を出すために状態レコードを
  最後まで走査する。`LastEvaluatedKey` を次ページの鍵にすれば走査は消えるが、`total` を諦める
  ことになり、API の形が変わる(`content-api.md` §3.1 の余地)。
- **Webhook を SQS に載せる**: 応答後に実行環境が凍結されうるので、配信は「予算付きの同期送信」で
  逃げている(§1)。配信が失われては困る規模になったら、queue + publish 権限 + 配信 Lambda を
  Terraform に足し、`Notifier` をバックエンドごとに選ぶ。テストには queue のエミュレータが要る。

#### 実行アーキテクチャ(2026-09: **arm64 / Graviton を既定**)

Lambda は **arm64** で動かす。x86_64 より GB 秒あたりの単価が安く、この処理内容では速度も
同等以上なので、選ばない理由がない。切り替えは次の 2 か所だけで済む。

| どこ | 何 |
|---|---|
| `scripts/build-lambda.sh` | `--arch arm64`(既定)で `aarch64-unknown-linux-gnu` 向けにビルドし、`infra/build/tsubame-aws-arm64.zip` を書く |
| `infra` | `function_architecture`(既定 `arm64`)が `architectures` に入り、`local.function_zip` が**同じ名前の zip** を指す |

**アーキテクチャの食い違いは invoke するまで分からない**(デプロイは成功する)。だから
スクリプトが `file` で成果物を検査し、zip の名前にもアーキテクチャを入れ、Terraform 側は
その名前を変数から導出している。

x86_64 からクロスビルドするには**ターゲット用の C コンパイラ**が要る(`ring` が C を
コンパイルするため。リンカだけでは足りない)。**zig を使う経路を既定の案内**にしている
(システムに何も入れず、libc も zig が持つ。検証済み)。

```
cargo install cargo-zigbuild
# zig は https://ziglang.org/download/ から(このリポジトリの CI は 0.13.0 を使う)
scripts/build-lambda.sh --arch arm64 --zig
```

ディストリビューションのクロスツールチェーンでも同じことができる(`--zig` を付けない場合)。

```
apt-get install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross
rustup target add aarch64-unknown-linux-gnu
scripts/build-lambda.sh --arch arm64
```

- スクリプトは先に**クロスツールチェーンが実際に動くか**を 1 行の C で確かめる。コンパイラが
  あっても binutils が無いと、ホストの `as` を呼んで `as: unrecognized option '-EL'` という
  分かりにくい失敗になるため(この確認は `--zig` では不要なので飛ばす)。
- 成果物が本当にそのアーキテクチャかは `file` で検査する。ここで気付かないと、Lambda が
  報告するのは invoke 時になる。
- **CI に arm64 ビルドの job がある**(`lambda-artifact`)。zig を入れて `--zig` でビルドし、
  zip の中身が aarch64 であることまで見る。ローカルの開発機が x86_64 でも、この経路は腐らない。
- zig でビルドしたバイナリが要求する glibc は **2.28** まで(確認済み)で、`provided.al2023` の
  2.34 で動く。
- 実際に**起動できるか**は staging で確認する(下の P5 の E2E と同じ扱い)。
- 同じターゲットは on-prem のバイナリにも使える(`cargo build --target aarch64-unknown-linux-gnu`、
  ただし rkv/LMDB も C なので同じクロスツールチェーンが要る)。

### 起動時の rustls プロバイダ(2026-09 修正)

`reqwest` は rustls の暗号プロバイダを自分で選ばない(`rustls-no-provider` で aws-lc-rs の CMake を
避けている)ため、**HTTPS クライアントを作る前にプロセスが 1 つ据える**必要がある。据えていたのは
Webhook の notifier だけで、Webhook を使わない配備では `HttpJwks` のクライアント生成時に
パニックしていた。`install_crypto_provider()` を `HttpJwks::new` と AWS バイナリの起動時に呼ぶ。

### 別トピック(今回は対象外)

- **UI の多言語化**: 方針と用語の決定リストは [`docs/i18n.md`](i18n.md)。コンテンツの
  多言語化はその §4 に TODO として分けてある(UI 文言とは別規模)。

- 他 CMS からの移行スクリプト(別プロジェクトで HTTP を叩く方針で合意済み)。
  件数が多くて耐えられない場合のみ、**一括作成エンドポイントを CMS 側に足す**。
- ~~`docs/swagger.yaml` の更新~~ → **削除した**(実装の一部しか載っておらず、Petstore の
  サンプル文が残っていた。契約は `docs/content-api.md` と契約テスト)。

### 資格情報は「エンドポイント上書き」で判断する(2026-09 修正)

`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` が在るだけでエミュレータ用の資格情報を作っていたため、
**Lambda では実行ロールの一時資格情報を同じ変数名で受け取りながら、セッショントークンを落とした
固定資格情報で署名する**状態だった(全リクエストが失敗し、ロールのローテーションも無視される)。
判断材料を **エンドポイント上書きの有無**に変え、`AWS_SESSION_TOKEN` があれば一緒に載せる。
エミュレータ向けの資格情報は `emulator_credentials` に閉じており、`cargo test -p tsubame-aws` の
`credential_tests` が「上書きが無ければ使わない」「上書きがあればトークンごと使う」を固定する。

### S3 の「無い」は権限しだいで 403 になる(2026-09 修正)

`ImageService::replace_image` は差し替えの適用前に **HeadObject** で実体の存在を確かめ、404 を
「まだアップロードされていない」として拒否する。しかし **`s3:ListBucket` が無いと、S3 は存在
しないキーにも 403 を返す**(存在を明かさないため)ので、その拒否に到達せず 500 になっていた。
さらに実行ロールには `s3:GetObject` が無く、**公開バケットポリシーの `s3:GetObject`(principal `*`)
で偶然通っていた** — バケットを CloudFront の背後に置いた時点で HeadObject ごと失敗する。
`infra/lambda.tf` に `s3:GetObject`(HeadObject 用)と `s3:ListBucket`(404 を取り戻すため)を明示。

**エミュレータでも再現できるようにした**: **配備と同じポリシーを持つユーザー**を作り
(`user_with_the_deployment_policy`。いまは versitygw のユーザー + そのユーザーを principal にした
バケットポリシー)、その資格情報で `image_bytes_exist` を呼ぶ。配備ポリシーに無い権限を
コードが使えば 403 になるので、root 資格情報で偶然通っていた状態(上の `s3:GetObject` の件)は
もう隠れない。**このテストはしばらく一度も走っていなかった**(compose ファイルのパスが 1 階層
ずれていて、`run_in_emulator` が `None` を返し、テストが「スキップ」として通り抜けていた)。
当時の `run_in_emulator` は**失敗と「エミュレータが無い」を同じ `None` に畳んで**いたので、
ポリシーをエミュレータが拒否しても、ユーザーを作れなくても緑のままだった。いまは:

- **スキップは「エミュレータが無い」だけ**(`emulator_reachable` が見て `None`)。compose の
  パスが解決しない、`docker` が失敗する、ゲートウェイが拒否するのは**すべてテストの失敗**。
- したがって `cargo test` の "24 passed" を**カバレッジとして読むときは注意**: エミュレータを
  起動していない実行では、この系統のテストはスキップしたまま緑になる(それは意図した動作で、
  `scripts/test-rust.sh` はそもそも 8000/9000 が開いていなければ AWS 側を走らせない)。

**エミュレータでは再現できない部分**: 実 S3 は `s3:ListBucket` の無い呼び出しに「存在しない
キー」も **403** と答える(不在と権限不足を区別させない)が、**エミュレータは ListBucket の有無に
かかわらず 404 を返す**。`ListBucket` を外したユーザーで差が出ないことをテストは明示的に固定し、
「403 になるので 404 が必要」という理由の側は `infra/lambda.tf` と `deployment_s3_policy` の
コメントが担う。

そのほか、この経路で踏んだエミュレータの癖: **DynamoDB Local はアクセスキー ID に英数字以外を
許さない**(`cms-...` は `UnrecognizedClientException`)。制限ユーザーの名前を英数字だけにして
あるのは、同じ資格情報を DynamoDB クライアントにも渡すテストがあるため。

残る穴は **Terraform とテストのポリシーが二重管理**であること: `deployment_s3_policy` を変える
ときは `infra/lambda.tf` も見る、というコメントでつないでいる(片方だけ変えてもテストは気づかない)。

### 画像レコードは読んだまま書き戻さない(2026-09 修正)

画像レコードは DynamoDB では 1 つの JSON 文字列(`data`)なので、「この 1 フィールドだけ更新」が
できない。差し替え要求が `pending_replacement` を記録するために**読んだレコード全体を無条件で
書き戻していた**ため、その間に差し替え適用が完了すると、適用が保存した新しい `file_name` を
**古い値で上書き**しうる — 適用は旧オブジェクトを削除済みなので、画像が表示できなくなる。

`write_if_unchanged`(`#data = :expected` の条件付き `put_item`)を足し、**読んだレコードと一致
するときだけ書く**ようにした。条件に弾かれたら読み直してやり直す(`RECORD_ATTEMPTS = 5`)。
`rename_image` と `set_image_deleted_at` も同じ read-modify-write なので、同じ経路
(`change_image`)を通した。適用側も**待ちの確認と消費を 1 回の条件付き書き込み**で行い、
「今配っているファイルの再適用」は何も書かずに 200 を返して**待ちを消さない**。
オンプレは初めから `Repository::begin()` の 1 ロックの中で読んで書いている。

## 3. テスト方針(要約)

| 層 | ローカル | 実 AWS |
|---|---|---|
| ルーター / ドメイン | `oneshot` + **共有契約スイート**(両バックエンド) | — |
| DynamoDB | DynamoDB Local(`endpoint_url` 差し替え) | GSI の反映遅延、スロットリング、上限 |
| S3 | **versitygw**(presign → PUT → GET。検証済み。不在の 403 化は再現しない) | IAM、CloudFront、転送 |
| Lambda 起動点 | localhost で `serve` して E2E / `cargo lambda watch`。CI が **arm64 の成果物**をビルドして中身のアーキテクチャまで確認 | イベント形状、コールドスタート、凍結 |
| Cognito | 鍵を生成して JWKS を注入(単体) | 実プールの設定 |
| 全体 | — | 使い捨てスタック + 既存 E2E |

エミュレータは **API を真似るが性能特性は真似ない**。だから緑になったローカルスイートを
「AWS で動く証明」とは扱わず、staging の煙テストを残す。

## 4. やらないこと

- オンプレ用の rkv 実装を DynamoDB に合わせて作り替えること(両方は残す。フィーチャーで選ぶ)。
- 移行スクリプトをこのリポジトリの内部 API に結合させること(HTTP のみ)。
- エミュレータだけで完結させること(実 AWS の確認を省く)。
