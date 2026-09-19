# コンテンツの関係(リレーション)の設計

**状態: 設計(未実装)**。最初の利用者は Strapi からの移行(`scripts/migrate-from-strapi/`)で、
同ツールは現在リレーションを既定で落としている。

Strapi の relation に相当するもの — コレクションのアイテムが、別のコレクションのアイテムを
参照する — を、この CMS の設計に合わせて入れる。**画像参照が既にその原型**なので、新しい概念を
持ち込むのではなく、画像で動いている仕組みを一般化する。

## 1. 何を作るか

- コレクションのアイテムに**参照フィールド**を定義できる(対象は他のコレクション、または単一ページ)
- 値は参照先の**集合**(順序は持たない)。1 件(単一)か複数(`has_many`)
- **逆引き**(このアイテムを参照しているコンテンツ)を API と画面で見られる。**配信 API でも引ける**
  — 「カテゴリー 3 の記事一覧」がページング付きで取れる(フィルタ)、「このカテゴリーとその記事」が
  1 段で取れる(逆引きの展開)
- 公開 API では、明示したときだけ**1 段だけ展開**する(公開済みの相手のみ)
- 参照されているアイテムの**削除は拒否**(理由と参照元を示す。`?detach=true` で外して削除)
- 移行ツールが Strapi の relation を取り込める(`--relations=relation`)

**作らないもの**: ユーザーへの参照(権限は別の話)、参照の深い再帰展開、参照の双方向の手動管理
(逆側はインデックスから自動)、参照の並び順(集合なので)、複合フィールドの定義を跨いだ対象指定。

## 2. 決めたこと

| 論点 | 決定 | 理由 |
|---|---|---|
| 定義する側 | **片面だけ**。フィールドを持つ側が「持つ側」、逆側はインデックスから引く | Strapi の `mappedBy`/`inversedBy` の二重管理をなくす。画像参照と同じ |
| カーディナリティ | **`has_many: bool` の 2 種類だけ** | Strapi の oneToOne / oneToMany / manyToOne はこの 2 つに畳める(多側に単一参照を置けば oneToMany) |
| 値の形 | `{ "target": "<collection or page>", "id"?: n }` の**集合**。順序は持たない | Strapi と同じ。順序が要るなら別のフィールドで表現する。集合は保存時に正準化(整列)して、比較と差分計算を安定させる |
| 対象 | **コレクションと単一ページの両方**。単一ページは `has_many: false` のみ | ページは id を持たないので「参照しているか」だけが値になる(複数持つ意味が無い) |
| 逆引き | **インデックス**(スキーマには呼び名だけ)。管理は `GET …/references`、配信はフィルタと展開で見る | 画像の `GET /models/images/{id}/references` と同じ。**引くのに再計算は要らない**(書き込み時に索引が更新される) |
| 逆側の呼び名 | `inverse_name: Option<String>` をスキーマに書ける。**データは持たない** | 画面の見出し(「このカテゴリーの記事」)と配信の `?populate=<inverse_name>` に使う。Strapi の `mappedBy` と違って二重管理にならない |
| インデックスの書き込み | **本体と同時**(DynamoDB は `TransactWriteItems`、オンプレは同じロック) | 削除の判断に使うので、画像参照より厳しくする(画像は本体→インデックスの 2 手で、ずれても削除が保守的になるだけ) |
| 未公開の相手を参照したまま公開 | **`required` な relation が公開時に空になるなら拒否**。任意の relation は配信で落とす | 保存時の検証が既に「required は空を許さない」なので、公開時も同じ規則にする。配列で一部が未公開なら残りが出るので通る |
| 公開中の参照元があるアイテムの unpublish | **同じ規則**: 下げた結果、`required` な参照元が空になるなら拒否 | 公開のときと同じ 1 つの規則で説明できる |
| 削除される側 | **拒否が既定**。`?detach=true` で参照を外してから削除(参照は全部外れる) | 画像の「ゴミ箱 → 参照確認 → 完全削除」と同じ思想 |
| 公開 API の展開 | **既定は id のみ**、`?populate=<field>` で 1 段展開 | 画像は常に展開しているが、多対多では応答が重くなり、循環・未公開の説明も要る。明示が安全 |
| 循環・自己参照 | **許す** | 値であって定義ではないので作成順の問題が無い。展開は 1 段なので再帰しない |

## 3. データの形

### スキーマ(`core/src/models/schema.rs`)

```rust
FieldType::Relation(RelationOptions {
    target: RelationTarget,       // Collection(name) | SinglePage(name)
    has_many: bool,               // 単一ページの対象では false のみ(検証で拒否)
    inverse_name: Option<String>, // 逆側の呼び名。表示と ?populate= にだけ使う
})
```

- 検証は**複合フィールド参照と同じ場所**で: 対象が存在すること、単一ページなら `has_many: false` で
  あること、`inverse_name` が対象側で一意であること(`referenced_composite_ids` の隣に
  `referenced_targets` を足す)。
- `inverse_name` は**呼び名だけ**でデータを持たない。逆引きは常にインデックスから引く。
- 定義の作成順に制約が出る(対象が先)。Strapi の相互参照は片側だけ採用して回避する(§7)。

### 値(`core/src/models/values.rs`)

```rust
// 値は「対象の名前」と「アイテムの id」だけ。どちらの種類かはスキーマが知っている。
FieldValue::Relation(Vec<RelationRef>)    // RelationRef { target: String, item: Option<u64> }
FieldValueResponse::Relation(Vec<RelationResponse>)   // §5 の形
```

- コレクションの対象なら `{ "target": "authors", "item": 7 }`、単一ページなら
  `{ "target": "home", "item": null }`(ページは id を持たない)。値の形がスキーマの種類と
  食い違うものは `from_untyped` が拒否する。
- **集合として保存する**: 保存時に `(target, item)` で整列・重複除去する。順序を持たない以上、
  比較(公開コピーとの差分)、インデックスの差分計算、テストの断言を安定させる必要がある。

追加が要るのは、既存の型が通っているすべての分岐:

| 場所 | 中身 |
|---|---|
| `FieldValue` / `FieldValueResponse` | 新しい variant |
| `from_untyped` | `[{"target": "...", "item": n}, ...]` を受理(配列の要素型検証と同じ形) |
| `test_required` | `has_many: false` は 1 件以上、`true` は空を許す |
| `FieldSchema::get_default_value`(schema.rs L150) | 空の配列 |
| `FieldValue::to_response`(values.rs L721) | 画像と同じく**解決用の情報を渡す**形にする(今は `&HashMap<ImageId, Image>` を取る) |
| `referenced_images` の隣 | `referenced_items`(値の木を走査。配列・複合の中も見る) |
| フロント `value-field` | 型ごとの分岐(§6) |

### 逆引きインデックス

画像参照と同じ**両方向**のキー族を、汎用の `ItemOwner` で持つ:

| 問い | on-prem(rkv) | DynamoDB |
|---|---|---|
| このアイテムは何を参照しているか | `<owner>|<target>#<id>` | `refs#<owner>` / `rel#<target>#<id>` |
| このアイテムを誰が参照しているか | `target#<collection>#<id>|<owner>` | `rel#<collection>#<id>` / `ref#<owner>` |

- 保存・公開・削除のすべての書き込み経路で、**保存後の値**から差分を計算して更新する
  (`set_image_references` と同じ。ただし本体と同一トランザクション)。
- 逆引きは**両コピー(draft / published)の和**を数える(画像と同じ保守的な規則)。

## 4. 公開との関係(この設計の中心)

規則は **1 つだけ**にする: **公開した結果、`required` な relation が「公開されている相手」を
1 つも持たなくなるなら、その公開を拒否する**(409 + どのフィールドがどの相手を指しているか)。

- 配列(`has_many: true`)で一部が未公開 → 残りが出るので**通る**(サイトは 1 件減るだけ)。
- 配列で全部が未公開 → 空になるので、`required` なら拒否、任意なら通る。
- 単一(`has_many: false`)で相手が未公開 → 空になるので、`required` なら拒否、任意なら通る。
- **任意の relation は黙って落とす**(配信 API が「公開されている相手」だけを返す)。
  配列なら「その参照が無い」、単一なら「未設定」としてサイトに出る。

保存時の検証が既に「required は空を許さない」なので、**公開時も同じ意味の規則**になる。
「単一なら常に拒否」にしない理由: 任意の単一参照まで拒否することになり、相手を下げただけで
参照元が公開できなくなる(公開の自由を失う)。

- **`unpublish` も同じ規則**で見る: 下げた結果、`required` な参照元が空になるなら拒否する。
  そうでなければ下げてよく、配信 API からはその参照が消える。
- 値は 2 コピーにそれぞれ入る。**公開コピーの値**が上の規則の対象で、作業コピーは自由
  (未公開の相手を指していてよい。画面はそれを「未公開」と表示する)。
- 相手を先に公開すればよい、という**順序の問題**でもある。移行スクリプトは依存順に publish する
  (§7)。

## 5. API

### 管理 API

| メソッド | パス | 中身 |
|---|---|---|
| (既存) | `POST/PUT …/items` | 値として `values: { "author": [{"target":"authors","item":7}] }` を送る |
| GET | `/api/models/collections/{name}/items/{id}/references` | このアイテムを参照しているコンテンツ(逆引き) |
| GET | `/api/models/collections/{name}/items/{id}/related?field=<name>` | ピッカー用の候補(検索・ページング) |
| DELETE | `…/items/{id}?detach=true` | 参照を外して削除(既定は 409) |

- 管理 API の値は**未公開の相手も返す**。編集には見えている必要がある。未公開かどうかは
  逆引き(と候補一覧)が印を付ける。

### 公開 API(配信)

```jsonc
// 1) 順方向: 既定は対象と id だけ(公開されている相手のみ)
"category": [{ "target": "categories", "item": 3 }]

// 2) 順方向の展開: ?populate=category(公開済みの相手だけ)
"category": [{ "target": "categories", "item": 3, "values": { "name": "技術" } }]

// 3) フィルタ: 逆側から引く(「カテゴリー 3 の記事」)。ページングは既存の limit/offset
//    GET /api/content/collections/articles?where=category:3&limit=25&offset=0

// 4) 逆引きの展開: ?populate=<inverse_name>(「このカテゴリーとその記事」)
"articles": [{ "target": "articles", "item": 12, "values": { … } }]
```

- 展開の深さは 1 段固定(`populate` はフィールド名か `inverse_name` の列挙のみ。再帰指定は無し)。
- **フィルタは relation の等値だけ**: `?where=<field>:<id>`(複数はカンマ区切り)。それ以上の
  クエリ言語は作らない(「カテゴリーで絞る」が実際に要る唯一の形で、既存のページングがそのまま使える)。
- 逆引きの展開は**件数の上限**を持つ(既定 25、`?limit=` で変更可)。大きな集合はフィルタで
  ページングして読む。
- 未公開の相手は**配信では落ちる**(§4 の規則)。`required` な relation がそれで空になる公開は、
  そもそも拒否されている。
- 画像は今までどおり常に `{id, url}` に展開(参照の一種だが、URL 解決という別の意味を持つ)。

## 6. 画面(コストの見積り)

型を 1 つ足すのは**追加的**で、触る場所は集中している(調査済み):

| 場所 | 追加するもの |
|---|---|
| `models/schema/fields.ts` | `RelationFieldSchema` + `FieldTypeMap`(L59–71)/`FieldDefaults`(L73–85)、型ガード(L181–201)、`FieldTypeStringPipe`(L282–298)の分岐 |
| `shared/field/field.ts` | `loadCompositeIds()`(L184–195)と同じ形で、コレクション一覧を遅延取得して選択肢にする |
| `shared/field/field.html`(L72–118) | 型ごとのオプション欄に「対象」+「複数」 |
| `shared/relation-field/`(新規) | 対象(コレクション / 単一ページ)の選択(複合フィールドの選択と同じ形) |
| `models/values/fields.ts`(L82–98) | 既定値(空の配列) |
| `shared/value-field/value-field.{ts,html}` | `FieldKind`(L45–57) + `kind()`(L191–203) + `@switch` の `@case` + **ピッカー**(画像ピッカー L559–609 / L410–453 と同じ形: 遅延一覧 + 単一/複数 + 選択。集合なので並べ替えは無し) |
| `assets/i18n/{en,ja}.json` | `schema.relation*` / `content.*`(ピッカーの文言)。カタログの整合は `catalog.spec.ts` と `keys.spec.ts` が見ている |
| spec 4 本 | `field.spec` / `edit-schema.spec` / `fields.spec` / `value-field.spec` |

- 型のドロップダウンは `Object.keys(FieldDefaults)` から作られるので、**一覧の編集は要らない**。
- 型名はどれも翻訳していない(画面には生の `Text` / `CompositeField` が出る)。`Relation` も同じ扱いに
  なる。ここを直すなら別の小さな作業として分ける。
- **型の一覧を列挙しているテストは無い**ので、追加は 4 本の spec に集中する。
- 参照元パネル(逆引き)はアイテム画面に足す。画像の「使用している場所」が前例。

## 7. 移行(Strapi)

- Strapi v3 の `oneToOne` / `oneToMany` / `manyToMany` / `oneWay` / `manyWay` を
  **「片面 + `has_many`」に畳む**。両側に定義がある場合は**片方だけ採用**し、もう片方は警告に出す
  (CMS の逆引きは自動なので情報は失われない)。
- `single type`(単一ページ)への参照も移せる(2026-09 の設計変更で対象に含めた)。
- **2 パス必要**: 1 周目でアイテムを作り Strapi id → CMS id の対応表を作る(既に state が持っている)。
  2 周目で参照を設定する。参照先が未作成のものは警告して落とす。
- **順序は移さない**(CMS の値は集合)。Strapi 側で並び順に意味があるなら、移行時に警告する。
- **公開の順序**: 参照される側を先に publish する(依存順)。移行ツールは対応表から依存を辿れる。
- ツールの `--relations` に `relation` を追加(既定 `skip` は残す。移行前の安全策として
  `text` も残す)。
- **`created_at` / `published_at` は既に API から設定できる**(2026-09、`PUT …/metadata`)ので、
  ツールの README の「日時はサーバーが打つので移行後の日時になる」という記述は更新が要る。

## 8. テスト

- **契約スイート**(両アダプタ。インデックスの実装が別なので特に重要):
  - 値の往復(単一・複数・集合の正準化)、存在しない相手は 400、参照先の種類と合わない値は 400
  - 逆引きが保存・公開・削除のそれぞれで正しい
  - **`required` な relation が公開時に空になるなら公開は 409**、任意なら通って配信から落ちる
  - **参照されているアイテムの削除 → 409**、`?detach=true` で成功(参照が外れる)
  - 公開 API: 既定は対象と id のみ / `?populate=` で 1 段展開 / 未公開の相手は出ない
  - **逆引きのフィルタ**(`?where=category:3`): 公開済みだけが返り、ページングが効き、公開されて
    いない参照元は出ない
  - **逆引きの展開**(`?populate=<inverse_name>`): 1 段、公開済みのみ、件数の上限
  - 単一ページを対象にした relation(値の形が違う)
- **unit**: 値の検証、`referenced_items` の走査(配列・複合の中)、インデックスの差分計算
- **E2E**: スキーマ編集で relation を定義 → ピッカーで選ぶ → 保存 → 公開 → 配信 API で展開
- **移行ツール**: relation のマッピング(片側採用・2 パス・順序の警告)
- 新しい拒否コード(`detach` が要る / `required` が空になる)は **3 点契約**(Rust ↔
  `error-codes.json` ↔ en/ja カタログ)に追加する。

## 9. 段階(コミット分割)

1. **型と値**: `FieldType::Relation`(コレクション + 単一ページ)+ `FieldValue::Relation` +
   検証 + 管理 API での往復(フロントは対象と id を並べる JSON 欄。既存の配列編集がそのまま使える)
2. **逆引きと安全**: インデックス(本体と同一トランザクション)+ `references` + 削除の拒否と detach
3. **公開の整合**: 公開時の検査(`required` が空になるなら 409)+ unpublish の扱い
4. **配信**: `?populate=` の 1 段展開 + 逆引きのフィルタ(`?where=`)と逆引きの展開(`inverse_name`)
5. **UI**: スキーマ編集の relation 型 + ピッカー + 参照元パネル(見出しは `inverse_name`)
6. **移行**: `--relations=relation`

1〜3 で「移行して壊れない」まで届く。4〜6 が「使える」。

## 10. この設計で確定(2026-09)

| 論点 | 決定 |
|---|---|
| 未公開の相手を参照したままの公開 | **`required` な relation が空になるなら拒否、任意なら配信で落とす**(§4)。保存時の「required は空を許さない」と同じ規則 |
| 片面定義 / カーディナリティ / 値の形 | 片面・`has_many` の 2 種類・`{target, item}` の集合 |
| 対象 | コレクション + 単一ページ(単一ページは `has_many: false` のみ) |
| 逆引き | インデックス(本体と同一トランザクション)。管理は `references` と画面、配信はフィルタと展開 |
| 逆側の呼び名 | `inverse_name`(表示と `?populate=` にだけ使う。データは持たない) |
| 配信の展開 | 明示 `?populate=` のみ、1 段 |
| 削除 | 拒否が既定、`?detach=true` で参照を全部外して削除 |
