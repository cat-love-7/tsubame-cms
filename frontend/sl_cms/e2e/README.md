# 管理画面の E2E チェック

実ブラウザで管理画面を操作し、**実際の API と組み合わせて**動くことを確認する。

`npm test`(Vitest のコンポーネントテスト)はサービスをスタブするので、DOM とロジックは見られるが
通信の形は見えない。ここで捕まえるのはその隙間で、実際に次の 3 件の不具合はこのチェックで見つかった。

- 一覧・状態・ナビゲーションを同時に取りに行くと 500 になる(LMDB のトランザクション制約)
- 変更系 API が本文をテキストで返すため、Angular がパースに失敗して「保存/削除に失敗した」と誤判定する
- 一覧の状態を通常フィールドで持っていたため `ExpressionChangedAfterItHasBeenCheckedError` が起き、
  最終ページの 1 行が空で描画される

## 前提

1. バックエンド: `cargo run`(127.0.0.1:8080、データは `./data` に作られる)
2. 開発サーバ: `cd frontend/sl_cms && npm start`(localhost:4200、`/api` をバックエンドへ転送)
3. Chromium(初回のみ): `npx playwright install chromium`

ブラウザは既定で `~/.cache/ms-playwright` に入る。**この開発環境ではホームディレクトリに
書き込めない**ため、置き場所を指定して導入する。

```bash
export PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers
npx playwright install chromium
```

実行時も同じ `PLAYWRIGHT_BROWSERS_PATH` が必要(未設定だと Playwright は `~/.cache/ms-playwright`
を探し、「Executable doesn't exist」で失敗する)。通常の開発機ではこの指定は不要。

## 実行

```bash
cd frontend/sl_cms
PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers npm run e2e
```

環境変数で調整できる。

| 変数 | 既定値 | 意味 |
|---|---|---|
| `BASE_URL` | `http://localhost:4200` | 開発サーバ |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | `admin@example.com` / `admin-password` | データ投入とログインに使う管理者 |
| `COLLECTION` | `e2e_blog` | ページングを見るコレクション |
| `TOTAL` | `60` | その件数(50 件ページの確認があるため 51 以上が必要) |
| `LAST_PAGE_COLLECTION` | `e2e_small` | 最終ページの削除を見るコレクション |
| `LAST_PAGE_TOTAL` | `26` | その件数(25 件ページ + 1 件になる想定) |
| `IMAGE_COLLECTION` | `e2e_images` | 画像フィールド(単一 + 配列)を持つコレクション |
| `COMPOSITE_COLLECTION` | `e2e_composite` | 複合フィールドの中に画像配列を持つコレクション |
| `COMPOSITE_ID` | `e2e_gallery_block` | その複合フィールド定義の id |

## 何を確認するか

- ログインフォームからのサインイン
- 1 ページ目が既定の 25 件、id 昇順、ページャに総件数が出る
- 状態バッジと Updated 列が描画される
- 次ページ・ページサイズ変更がサーバへ届いている
- 一覧から公開できる(Draft → Published)
- 最終ページの最後の 1 件を削除すると前のページへ戻る
- 画像ライブラリ: アップロードでき、名前つきで並び、実体が配信され、削除できる
- コンテンツ編集中に既存画像を選んで保存できる(選んだ id がアイテムに残る)
- 画像配列に**複数まとめて**追加でき、**その場でアップロード**もでき、その順序で保存される
- **複合フィールドの中の画像配列**でも同じことができる
- その間ブラウザのコンソールエラーが出ない

## データ

実行のたびに `e2e_blog` / `e2e_small` / `e2e_images` / `e2e_composite` と複合フィールド定義
`e2e_gallery_block` を作り直す(既にあれば削除する)。開発用のデータには触れず、何度実行しても
同じ結果になる。アップロードした画像も実行の最後に削除する。
