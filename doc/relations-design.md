# コンテンツの関係(リレーション)の設計

**状態: 第 3 段階(公開の整合)まで実装済み**。§9 の段階でいう 1〜3 が入っており、4 以降は
未実装(第 5 段階のピッカーと、複合定義の中の relation は実装済み)。最初の利用者は Strapi からの移行(`scripts/migrate-from-strapi/`)で、同ツールは現在
リレーションを既定で落としている。

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
(逆側はインデックスから自動)、参照の並び順(集合なので)。

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
| 配列の要素型 | **なれない**。複数持つのは `has_many` だけ | relation は既に集合なので、配列にすると「集合の集合」という 2 つ目の言い方になる。編集も配信もできない(検証で拒否) |
| 置ける場所 | **フィールドと複合フィールド定義の中**。配列の要素型にはできない | 定義は独立して保存され再利用されるので、対象はサイトのコレクション / 単一ページで決まる(埋め込む側に依存しない)。値は複合の中に入るが、**参照を持つのはアイテム**なので索引の持ち主は変わらない |

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
  - **対象の存在**はコレクションと単一ページの一覧が要るので、スキーマ保存のサービスで見る
    (`RelationTargetSource` が両方の名前を答える)。**複合定義の中の relation も同じ対象**なので、
    `referenced_relation_targets` は複合定義の中まで歩く(定義は 1 回だけ。配列を通して自分自身に
    戻る定義があるため)。見る場所は 2 つ: **定義の保存時**(`CompositeFieldService`。誰も埋め込んで
    いない定義も保存できてしまう)と、**埋め込む側のスキーマ保存時**(コレクション / 単一ページ)。
  - **`inverse_name` の一意性**はまだ見ていない(第 5 段階。逆引きの展開が入る時)。
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
| `referenced_images` の隣 | `referenced_items`(値の木を走査。配列・複合の中も見る。`without_reference` も同じ) |
| フロント `value-field` | 型ごとの分岐(§6) |

**第 1 段階で実装した形**(`FieldValueResponse` は参照の配列をそのまま返す。展開は第 4 段階):

- `RelationTarget` は `{ "kind": "collection" | "single_page", "name": "..." }` の内部タグ付き。
  同じ名前でも種類が違えば別の対象なので、検証は種類ごとの一覧と突き合わせる。
- 単一ページの値は `{ "target": "home" }`(`item` は付かない)。`item` を送ると拒否する。
- 配列の要素型にはできない(`validate_field_type` が拒否)。読み出しも `validate_at` が
  「配列の要素が relation だった」を型不一致として返す。

### 逆引きインデックス

画像参照と同じ**両方向**のキー族を、汎用の `ItemOwner` で持つ:

**所有者も対象も同じ形**(`ItemOwner`: `collection:<name>:<id>` / `page:<name>`)で書く。対象とは
「参照先のコンテンツ」そのものなので、所有者と同じ名前の付け方ができる。

| 問い | on-prem(rkv) | DynamoDB |
|---|---|---|
| このコンテンツは何を参照しているか | `<owner>|rel|<target>` | `refs#<owner>` / `rel#<target>` |
| このコンテンツを誰が参照しているか | `rel|<target>|<owner>` | `rel#<target>` / `ref#<owner>` |

- 値は所有者の storage key(画像索引と同じ。読むときに使うのはキーだけ)。
- 画像索引の順方向も `refs#<owner>` を partition に使う(`image#` の並び)ので、AWS では
  1 つの所有者の「出ていく参照」が同じ partition に並ぶ。

- 保存・公開・削除のすべての書き込み経路で、**保存後の値**から差分を計算して更新する
  (`set_image_references` と同じ。ただし本体と同一トランザクション)。
  - 差分の計算は**アダプタの中で、書き込みロック/トランザクションの中**で行う。どのコピーを書いて
    いるか(`Written::Published` / `Written::Draft`)を渡し、もう一方のコピーはその場で読む。
    サービスは関与しない: 値の書き込みを通る限り、どの経路でも索引が付いてくる。
  - **スキーマ保存の再索引は要らない**。`referenced_items` はスキーマではなく**保存されている値**を
    見るので、フィールドを消したスキーマで保存し直せば値と一緒に索引も消え、追加したフィールドは
    その保存で入る。
- 逆引きは**両コピー(draft / published)の和**を数える(画像と同じ保守的な規則)。

### 削除(2026-09 実装)

- `DELETE …/items/{id}` / `DELETE …/single_pages/{name}` は、**参照されているなら 409**
  (`still_referenced`)。メッセージは参照元を最大 3 件名指しする。
- **`?detach=true`** で参照を全部外してから削除する。外すのは**参照元の両コピー**(配信される
  公開コピーと、編集中の作業コピー)で、書き込みは通常の保存経路を通るので索引も一緒に動く。
  全部外してから削除するので、途中で失敗しても「参照が残ったまま消える」方向にはならない。
- **`DELETE …/collections/{name}`(コレクションごと)は今のところ参照を見ない**。対象の
  コレクションを丸ごと消すと、参照元の値は消えたアイテムを指したままになる(配信は第 4 段階で
  それを落とす)。§9 の残りとして扱う。
- 画像索引は削除の判断には使われない(ゴミ箱→完全削除の 2 段階と参照表示)ので、ここだけ規則が
  違う: **リレーションは拒否が既定**、画像は「見せるだけ」。

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

型を 1 つ足すのは**追加的**で、触る場所は集中している(調査済み)。**第 1 段階で入ったのは
「対象 + 複数 + 呼び名」の選択まで**で、値は JSON 欄のまま(ピッカーは第 5 段階):

| 場所 | 追加するもの | 第 1 段階 |
|---|---|---|
| `models/schema/fields.ts` | `RelationFieldSchema` + `FieldTypeMap`(L59–71)/`FieldDefaults`(L73–85)、型ガード(L181–201)、`FieldTypeStringPipe`(L282–298)の分岐 | 済 |
| `dashboard/settings/shared/field/field.ts` | `loadCompositeIds()`(L184–195)と同じ形で、コレクションと単一ページの一覧を遅延取得して選択肢にする | 済(専用コンポーネントは作らず、`field` の中で完結した) |
| `dashboard/settings/shared/field/field.html`(L72–118) | 型ごとのオプション欄に「対象」+「複数」+「逆側での呼び名」 | 済 |
| `shared/relation-field/`(新規) | 対象(コレクション / 単一ページ)の選択(複合フィールドの選択と同じ形) | 作らなかった(選択肢が 2 つだけなので `field` の中の 3 つのコントロールで足りる) |
| `models/values/fields.ts`(L82–98) | 既定値(空の配列) | 済 |
| `shared/value-field/value-field.{ts,html}` | `FieldKind`(L45–57) + `kind()`(L191–203) + `@switch` の `@case` + **ピッカー**(画像ピッカー L559–609 / L410–453 と同じ形: 遅延一覧 + 単一/複数 + 選択。集合なので並べ替えは無し) | `@case` と JSON 欄の検証まで。ピッカーは第 5 段階 |
| `assets/i18n/{en,ja}.json` | `schema.relation*` / `content.relation*`。カタログの整合は `catalog.spec.ts` と `keys.spec.ts` が見ている | 済 |
| spec 4 本 | `field.spec` / `edit-schema.spec` / `fields.spec` / `value-field.spec` | `fields.spec` / `field.spec` / `value-field.spec` に追加した(`edit-schema.spec` は型を列挙していないので変更が要らなかった) |

- 型のドロップダウンは `Object.keys(FieldDefaults)` から作られるので、**一覧の編集は要らない**。
- 型名はどれも翻訳していない(画面には生の `Text` / `CompositeField` が出る)。`Relation` も同じ扱いに
  なる。ここを直すなら別の小さな作業として分ける。
- **型の一覧を列挙しているテストは無い**ので、追加は 4 本の spec に集中する。
- 参照元パネル(逆引き)はアイテム画面に足す。画像の「使用している場所」が前例。
- JSON 欄は relation の値が**配列**であることを使って、配列の JSON 編集と同じ欄・同じバッファで
  編集する。違うのは検証だけで、配列の要素型ではなくフィールド自身の対象と突き合わせる
  (対象名・アイテム id の有無・単一か複数か)。サーバーが正準化するので、欄の中身は送ったまま
  保存され、次に読み出したときに整列済みで戻る。

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

**第 1・2 段階で入ったもの**:

- unit(`core/src/models/{schema,values}.rs`): 対象の検証、単一ページの `has_many` 拒否、配列要素の拒否、
  複合定義が relation を持てること、複合定義の中まで歩く対象収集(自分自身に配列で戻る定義でも止まる)、
  `validate_relation_targets` の種類違い、値の正準化(整列・重複除去)、
  対象の食い違い・アイテム id の欠落・単一に複数・単一ページの `item` の拒否、`required` の空
- service(`core/src/services/{collection,single_page,composite_field}_service.rs`): 存在しない対象を
  名指すスキーマ保存は 400(種類違いも 400)。**複合定義の中の relation** は、定義の保存でも、
  それを埋め込むスキーマの保存でも 400
- 契約スイート(`crates/tests/suite/relations.rs`、両アダプタ): 集合の往復、単一ページの値の形、
  スキーマ保存の 400 4 種(複合定義の保存を含む)、値の 400 4 種、`required` な relation が空のままの公開は 400、
  **複合の中の参照**(複合フィールドとその配列の両方)が索引に載り、削除が 409 で拒否され、
  `?detach=true` が複合の中まで外すこと
- 公開の整合(2026-09、`repositories/relation_rules.rs` の unit + 契約スイート): 未公開の相手しか
  持たない `required` な relation を持つ公開は 409 `relation_unpublished`、相手を先に公開すれば通る、
  公開中の参照元がいる `unpublish` は 409 `relation_required_by`、参照元が未公開なら通る、
  同じフィールドに公開済みの参照が残るなら通る、任意の relation はどちらも通る、
  複合・配列の中の `required` は経路つきで拒否、消えた相手は未公開として数える
- spec(`fields.spec` / `field.spec` / `value-field.spec`): 型の分岐、送信前の正規化、
  一覧の遅延取得、値の JSON 欄の検証
- 索引(`RelationIndexChanges::between` の集合差、`referenced_items` の走査、`without_reference`)。
  契約スイートでは、保存・公開・削除のそれぞれで索引が追いつくこと、両コピーの和を数えること、
  `?detach=true` が両コピーから参照を外すこと、参照されている削除の 409 と `still_referenced`。
  参照される側(アイテム・単一ページ)と参照する側(アイテム・単一ページ)の 4 通り。

**これから要るもの**:

- **契約スイート**(両アダプタ):
  - **`required` な relation が公開時に空になるなら公開は 409**、任意なら通って配信から落とす
  - 公開 API: 既定は対象と id のみ / `?populate=` で 1 段展開 / 未公開の相手は出ない
  - **逆引きのフィルタ**(`?where=category:3`): 公開済みだけが返り、ページングが効き、公開されて
    いない参照元は出ない
  - **逆引きの展開**(`?populate=<inverse_name>`): 1 段、公開済みのみ、件数の上限
  - **コレクションごとの削除**が参照を見るかどうか(§4 の残り)
- **E2E**: スキーマ編集で relation を定義 → ピッカーで選ぶ → 保存 → 公開 → 配信 API で展開。
  参照されているアイテムの削除が画面でどう見えるか(参照元パネル)も。
- **移行ツール**: relation のマッピング(片側採用・2 パス・順序の警告)
- 新しい拒否コード(`detach` が要る / `required` が空になる)は **3 点契約**(Rust ↔
  `error-codes.json` ↔ en/ja カタログ)に追加する。

## 9. 段階(コミット分割)

1. ✅ **型と値**: `FieldType::Relation`(コレクション + 単一ページ)+ `FieldValue::Relation` +
   検証 + 管理 API での往復(フロントは対象と id を並べる JSON 欄。既存の配列編集がそのまま使える)
   — **実装済み**。対象の存在はスキーマ保存で、値の形は `from_untyped` で見る。単一ページの
   `item` と配列要素の relation は拒否。契約スイートは `crates/tests/suite/relations.rs`。
   `required` な relation が空のままの公開は、公開時の required 検査(既存の規則)がそのまま拒否する。
2. ✅ **逆引きと安全**: インデックス(本体と同一トランザクション)+ `references` + 削除の拒否と detach
   — **実装済み**。索引は各アダプタの書き込み経路の中で差分を取って本体と同じトランザクションで
   書く。`GET …/items/{id}/references` と `GET …/single_pages/{name}/references`、
   `?detach=true` つきの削除。契約スイートは `crates/tests/suite/relations.rs`。
3. ✅ **公開の整合**: 公開時の検査(`required` が空になるなら 409)+ unpublish の扱い
   — **実装済み**(2026-09)。規則は §4 の 1 つだけで、公開と unpublish を同じ質問の両方向として見る
   (`repositories/relation_rules.rs`)。公開時は公開コピーの `required` な relation について
   **公開されている参照先**が 1 つ以上あることを確かめ、無ければ 409 `relation_unpublished`。
   unpublish 時は**索引が答える参照元**(公開中のものだけ)について、そのフィールドが他に公開済みの
   参照を持つかを確かめ、無ければ 409 `relation_required_by`。任意の relation はどちらも自由。
   複合の中・配列の要素の中の `required` な relation も同じ規則で見て、拒否は
   `cta.author` / `blocks[0].author` のような経路を返す。参照先の状態は `ContentReader`
   (アイテムでもページでも同じ質問ができる)が読み、両サービスに配線した。
4. **配信**: `?populate=` の 1 段展開 + 逆引きのフィルタ(`?where=`)と逆引きの展開(`inverse_name`)
5. **UI**: スキーマ編集の relation 型 + ピッカー + 参照元パネル(見出しは `inverse_name`。
   `inverse_name` の一意性もここで見る)
   — **ピッカーだけ実装済み**(2026-09)。内容編集画面の relation は
   **選んだ項目をチップで持ち**、`app-relation-picker` が対象のアイテム(コレクションなら
   `items` + `items/titles`、単一ページなら `single_pages` + `single_pages/titles`)を**タイトルで**
   並べて選ばせる。JSON 欄は「JSON で編集」の裏に残す(移行や、ピッカーが表せない値のため)。
   候補は 100 件まで(絞り込みは画面側)で、開いたときだけ取りに行く。参照元パネル(`inverse_name`)は
   未実装。
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
| relation を置ける場所 | フィールドと複合フィールド定義の中(配列の要素型にはできない)。索引・削除の拒否・ detach は複合と配列の中まで届く |
