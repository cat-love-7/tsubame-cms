# 管理画面の E2E チェック

実ブラウザで管理画面を操作し、**実際の API と組み合わせて**動くことを確認する。

`npm test`(Vitest のコンポーネントテスト)はサービスをスタブするので、DOM とロジックは見られるが
通信の形は見えない。ここで捕まえるのはその隙間で、実際に次の 4 件の不具合はこのチェックで見つかった。

- 一覧・状態・ナビゲーションを同時に取りに行くと 500 になる(LMDB のトランザクション制約)
- 変更系 API が本文をテキストで返すため、Angular がパースに失敗して「保存/削除に失敗した」と誤判定する
- 一覧の状態を通常フィールドで持っていたため `ExpressionChangedAfterItHasBeenCheckedError` が起き、
  最終ページの 1 行が空で描画される
- 並行リクエストで 500 になる(rkv が **ストアを開くたびに** `mdb_dbi_open` を呼び、LMDB は
  他のトランザクションが動いている間それを拒否する)。読み取り系はストレージのロックで
  直したが、**パスワード照合だけがロックの外**に残っていた。パスワードリセット直後の
  サインインが、リセット画面のダッシュボード読み込みと重なると 500 になっていた

## 前提

1. バックエンド: `cargo run --manifest-path backend/Cargo.toml`(127.0.0.1:8080、データは
   `DATA_ROOT` の下。未設定なら起動したディレクトリの `./data`)。`--manifest-path` が指すのは
   ワークスペースなので、`default-members` の指定で on-premises のバイナリ(`sl-cms`)が起動する。
   `scripts/test-e2e.sh` はこれに `DATA_ROOT` と `JWT_SECRET` を渡して起動する。
2. 開発サーバ: `cd frontend && npm start`(localhost:4200、`/api` をバックエンドへ転送)
3. Chromium(初回のみ): `npx playwright install chromium`

ブラウザは既定で `~/.cache/ms-playwright` に入る。**この開発環境ではホームディレクトリに
書き込めない**ため、置き場所を指定して導入する。

```bash
export PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers
npx playwright install chromium
# Chromium needs the system libraries it links against. A freshly built container
# usually lacks them, and the browser fails with "error while loading shared
# libraries: libglib-2.0.so.0". This one needs root (apt).
npx playwright install-deps chromium
```

実行時も同じ `PLAYWRIGHT_BROWSERS_PATH` が必要(未設定だと Playwright は `~/.cache/ms-playwright`
を探し、「Executable doesn't exist」で失敗する)。通常の開発機ではこの指定は不要。

## 実行

```bash
cd frontend
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers npm run e2e
```

環境変数で調整できる。

| 変数 | 既定値 | 意味 |
|---|---|---|
| `BASE_URL` | `http://localhost:4200` | 開発サーバ |
| `ADMIN_USERNAME` / `ADMIN_PASSWORD` | `admin@example.com` / `admin-password` | データ投入とログインに使う管理者の ID(サーバ側は `ADMIN_EMAIL` でも可) |
| `COLLECTION` | `e2e_blog` | ページングを見るコレクション |
| `TOTAL` | `60` | その件数(50 件ページの確認があるため 51 以上が必要) |
| `LAST_PAGE_COLLECTION` | `e2e_small` | 最終ページの削除を見るコレクション |
| `LAST_PAGE_TOTAL` | `26` | その件数(25 件ページ + 1 件になる想定) |
| `IMAGE_COLLECTION` | `e2e_images` | 画像フィールド(単一 + 配列)を持つコレクション |
| `COMPOSITE_COLLECTION` | `e2e_composite` | 複合フィールドの中に画像配列を持つコレクション |
| `COMPOSITE_ID` | `e2e_gallery_block` | その複合フィールド定義の id |
| `SCHEMA_COLLECTION` | `e2e_schema_editor` | スキーマ編集画面から組み立てるコレクション(実行の最後に削除する) |
| `SCHEMA_BLOCK` | `e2e_block` | その配列フィールドが要素として持つ複合フィールド定義 |

## 言語

ハーネスは各ブラウザコンテキストで `localStorage['sl_cms.language'] = 'en'` を仕込んでから
ページを開く(`newContext()`)。画面の文言はカタログから来るので、固定しないと
**ブラウザの言語設定しだいで落ちる**チェックが出る(実際、日本語環境では「閲覧のみ」の
案内文を探すチェックが落ちる)。切り替え機能そのものは、この保存値の上から `use()` して
確認する。文言を変えたときは、この README の下の一覧と `check-ui.mjs` の
`has-text` / `aria-label` も一緒に直す。

## 何を確認するか

- ログインフォームからのサインイン
- 1 ページ目が既定の 25 件、id 昇順、ページャに総件数が出る
- 状態バッジと Updated 列が描画される
- 次ページ・ページサイズ変更がサーバへ届いている
- 一覧から公開できる(Draft → Published)
- 最終ページの最後の 1 件を削除すると前のページへ戻る
- **保存だけでは公開されない**(配信 API は公開コピーだけを見る)
- **公開すると配信 API に反映される**
- 画像ライブラリ: アップロードでき、名前つきで並び、実体が配信され、削除できる
- コンテンツ編集中に既存画像を選んで保存できる(選んだ id がアイテムに残る)
- 画像配列に**複数まとめて**追加でき、**その場でアップロード**もでき、その順序で保存される
- **複合フィールドの中の画像配列**でも同じことができる
- **スキーマ編集を画面から**通す: コレクションを作り、フィールドを足し(名前・型・幅プリセット)、
  Enum の値をチップで追加・削除し、**複合フィールドを配列の要素型に選び**、保存して API で読み戻す
  (他のコレクションは API で作っているので、ここがこの画面をブラウザで通す唯一の経路)
- そうやって定義した Enum フィールドを、**コンテンツ編集画面で選んで保存**できる
- そうやって定義した**複合の配列**を、要素ごとに追加・入力・並べ替えして保存でき、開き直すと
  保存した順で出る
- その複合定義は**自分自身の配列**を持てる(ブロックがブロックを持つ)。要素の中の配列は
  空のところで止まり、1 つ足せば 1 段深くなる
- **ロールごとに出し分け**: 編集ロールには公開・削除・アカウント管理を出さず、閲覧ロールには保存も出さない
- 一覧から公開すると、**誰が公開したか**がその行に出る
- **自分のパスワードを変更**すると、変更前のトークンは 401 になり、変更した本人のセッションは
  続き(新しいトークンを引き継ぐ)、新しいパスワードでサインインできる
- **共有プレビュー URL**: 保存だけした下書きが、トークン無しで開くリンクで見え(配信 API は古い
  公開コピーのまま)、リンクの宛先を書き換えると 401 になる
- **リソース単位の権限**: アカウント画面からコレクションごとの許可を保存でき、その許可は一覧の
  絞り込みと API の両方に効く(拒否したコレクションは一覧に出ず、直接叩いても 403)
- **パスワードリセット**: アカウント画面から発行したリンクを (別ブラウザで) 開いて新しい
  パスワードを設定でき、リセット前のセッションは切れ、同じリンクは二度使えない
- **総当たりはロックされる**: 6 回目の失敗は 429 になり、`Retry-After` が付き、ロック中は
  正しいパスワードでも通らず、他のアカウントは影響を受けない
- その間ブラウザのコンソールエラーが出ない

## デプロイ先のサインイン(AWS)

`check-ui.mjs` はローカルのパスワードフォームと `/api/auth/login` を叩く。Cognito がサインインを
持つデプロイには**そのどちらも無い**(エンドポイントは 501、画面にはホステッドページへのボタン)ので、
あちらの suite ではどうしても届かない。`hosted-signin.mjs` がそこだけを見る。

```bash
cd frontend
APP_URL=https://cms.example.com \
ADMIN_USERNAME=cat ADMIN_PASSWORD=... \
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers node e2e/hosted-signin.mjs
```

確認するのは、PKCE の受け渡し(`login_url` に `code_challenge` と `redirect_uri` が付く)、
**登録済みコールバック**へ戻ってくること、`/api/auth/callback` でのコード交換、CMS 自身のトークンが
保存されること、管理画面が出ること、その間コンソールエラーが出ないこと。`curl` で見る
`scripts/smoke-test.sh` はここまで届かない —— 画面は正しく描かれているのにサインインだけが
壊れている、という状態は起こりうる。

前提: サインインするアカウントが先に存在すること(管理者が `AdminCreateUser` +
`AdminSetUserPassword` で作り、その名前が `BOOTSTRAP_ADMIN_USERNAMES` に入っている)。

## デプロイ先のアカウント管理(AWS)

Cognito のエミュレータは無いので、`crates/aws/src/provisioner.rs` の SDK 呼び出しは手元では
**実行されない**(継ぎ目の向こうの対応付けだけが unit テストで見られる)。`hosted-accounts.mjs` が
実機でそこを通す。サインインそのものは共通の `hosted-login.mjs` が担う(ホステッドページの 2 つの
フォーム、PKCE、そして初回の一時パスワードに対する「新しいパスワードを決めさせる」ステップ)。

```bash
cd frontend
APP_URL=https://cms.example.com \
ADMIN_USERNAME=cat ADMIN_PASSWORD=... \
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers node e2e/hosted-accounts.mjs
```

1. 管理者としてサインイン
2. `POST /api/auth/users` でアカウント作成(アカウント画面と同じ呼び出し)
3. `POST /api/auth/users/{id}/password-reset` → `kind: "temporary"` と一時パスワード
4. **その一時パスワードで本人としてサインイン** → プロバイダが新パスワードを要求するので決めて入る
   ——ここが `external_id` の検証でもある: 作成時に Cognito の `sub` を記録していないと、本人は
   `not_provisioned` で弾かれる
5. 管理者に戻って削除
6. 削除後はサインインできない(プロバイダが拒否する)

途中で失敗してもアカウントは残らない(`finally` で削除する)。

## データ

実行のたびに `e2e_blog` / `e2e_small` / `e2e_images` / `e2e_composite` と複合フィールド定義
`e2e_gallery_block`、ロックを見るための使い捨てアカウント `e2e-throttle-<時刻>@example.com` を作り直す。
スキーマ編集の確認に使う `e2e_schema_editor` と、その配列が要素として持つ複合フィールド定義
`e2e_block`(**自分自身の配列を持つ**ブロック定義)は、その場で作って最後に消す(前回の実行が途中で落ちていた場合に備えて、開始時にも消す)
(既にあれば削除する)。開発用のデータには触れず、何度実行しても
同じ結果になる。アップロードした画像も実行の最後に削除する。
