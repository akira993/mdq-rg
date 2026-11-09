# mdrg 手引き

## 導入と更新

Apple Silicon macOS の Homebrew 導入命令:

```sh
brew tap akira993/tap
brew install mdq-rg
```

`brew upgrade mdq-rg` は更新、`cargo install mdq-rg --locked` は Cargo での導入・更新を行う。実行する命令は `mdrg`。README.md にソースからの組み立ての説明がある。

## 最初の検索

例の実行場所は文書の作業フォルダで、`docs/start.md` の内容は次の5行。

```markdown
# Start
Install the tool.

## Update
Update the tool.
```

```sh
mdrg search --q install --top-k 5
mdrg get 'docs/start.md:1-3'
mdrg list --paths 'docs/**' --heading-level 1 --limit 10
```

検索は `docs/start.md` の Start 節の `[1,3]` 行を返す。`get` は返された `path` と `lines` を受け取る。一覧の例はこの節を `level: 1` で返す。

## 命令

`mdrg --help` と各命令の `--help` は範囲・引数・JSON Lines を説明する。

- `search`: `--q` は必須。既定の `--mode rank` は節を順位付けする。`--top-k` は1以上で既定は5件。`--max-tokens` の既定は JSON 出力の推定トークン数1300で、先頭1件は上限を超えても返す。`--timing` は計測値を標準エラーに通常文の1行で出す。
- `get PATH:START-END`: 原文の行を読む。行番号は1から始まり、両端を含む。
- `list`: 節を一覧にする。`--heading-level N` は深さ N 以下、`--limit N` は出力行数の上限で、1以上でなければ誤り。既定は両方とも無制限。

`--mode exact` も節を返す。両方式とも件数か出力予算で打ち切られ、一致行を全件列挙しない。

```sh
mdrg search --q install --mode exact --max-tokens 200 --timing
```

次の ripgrep 命令は一致行を全件列挙するか、ファイルごとの一致行数を数える。

```sh
rg -n -F "text" -g '*.md'
rg -c -F "text" -g '*.md'
```

## 探す場所

既定は現在の作業フォルダ（cwd）。命令の前後の `--root DIR` は絶対パスか cwd からの相対パスでフォルダを選ぶ。その直下の `mdrg.toml` だけを読む。サブフォルダで実行しても親の `mdrg.toml` は読まない。Git の作業ツリー内では親の `.gitignore` は働く。

```sh
mdrg --root docs get 'start.md:1-3'
```

`search` と `list` の `--paths` は選んだフォルダからの相対パターンで、引用符はシェルの展開を防ぐ。複数の指定は和集合。`--paths` と設定の `exclude` では `*` が `/` にも一致する。範囲外のパスは有効な指定と混在しても `paths_outside_root`（終了値2）になる。`error.next` は、共通の親を `--root` にして相対パターンで再実行する案内。

小文字の `.md` が対象。隠しファイルは3命令とも `--hidden` が必要。ただし `hidden = true` の場合と、`roots` で名指した隠しフォルダ自体は不要（その中の隠し項目には必要）。`.ignore` と `.rgignore` は Git の作業ツリーの内外で働き、`.gitignore` は内側だけで働く。3命令の `--no-ignore` は無視規則を外す。`--no-ignore` はプロジェクトの許可が必要。設定の `exclude` による除外は残る。選んだフォルダからのパスの構成要素が `.git`、`node_modules`、`.ssh`、`.aws`、`.gnupg`、`.env` なら、大文字小文字を問わず常に除外する。

## 出力

3命令の標準出力は JSON Lines 形式で、配列ではなく1行に1つの JSON オブジェクト。`path` は選んだフォルダからの相対名、`lines` は両端を含む行範囲。`heading_path` は見出しを ` > ` で区切る。最初の見出しより前は `heading_path: "(preface)"`、`list` の `level` は0。見出しも本文も無い文書も `list` に `lines: [0,0]` の1行が出る。

検索には `score` と `snippet` が付き、`--mode exact` は `match`（`phrase` または `words`）を足す。取得には原文の `text`、一覧には見出しの深さ `level` が付く。

最初の `get` の例は次を出す。

```json
{"path":"docs/start.md","lines":[1,3],"heading_path":"Start","text":"# Start\nInstall the tool.\n\n"}
```

## 困ったとき

0件では標準出力は空になり、標準エラーの `no_results` に原因 `cause`、説明 `detail`、次の操作 `next` が入る。

- `no_documents`: 探すフォルダと絞り込み条件で検索できる文書がない。
- `paths_matched_nothing`: 隠し・無視・`exclude` の規則を通った文書に、`--paths` に一致するものがない。
- `empty_query`: 検索できる語が検索文にない。
- `no_match`: 語・検索方式に一致する節がないか、文書に本文がない。`list` は `--heading-level` でも絞る。完全一致は語ごとの該当節数 `words` を足す。

標準エラーの構造化された誤りには `error.code` と `error.detail` が入る。次の誤りには文字列の配列 `error.next` も付く。

- `outside_root`（`get`、終了値1）: 選んだ範囲外。`next` の命令はその親フォルダを `--root` にして `get` を再実行し、元の `--hidden` と `--no-ignore` を引き継ぐ。親が存在しないと `invalid_root`（終了値2）になる。解決後の絶対パスに常に除く名前を含むと、命令の代わりに説明が出る。
- `unsupported_type`（`get`、終了値1）: 小文字の `.md` 以外のファイル。
- `ignored`（`get`、終了値1）: 無視規則による除外。`--no-ignore`（プロジェクトの許可が必要）で読める。
- `paths_outside_root`（終了値2）: 「探す場所」のとおり。`invalid_root`（終了値2）: 選んだ場所が移れるフォルダでない。

ほかの `get` の誤りは `not_found`（終了値1、未存在・隠し・除外対象）、`out_of_range`（終了値1、文書外の行）、`invalid_range`（終了値2、範囲の書式不正・逆転）、`read_error`（終了値1、読めない文書）。

標準エラーの `skipped` は飛ばしたパスと理由 `invalid_utf8`、`contains_nul`、`read_error` を示す。`warning` オブジェクトには `code` と `detail` が入る。README.md に `usage_log_failed` の説明がある。

終了値0は0件も含む。1は実行失敗、2は引数・設定の誤り。引数の解析・設定の読み込みの誤りは通常文で標準エラーに出る。上記の行範囲と探す場所の誤りは JSON で出る。

## 設定

```toml
roots = ["docs"]
exclude = ["docs/drafts/**"]
hidden = false
```

既定の `roots` と `exclude` は空、`hidden = false`。`roots` は選んだフォルダ内に実在する相対名でなければならず、範囲をそこに絞る。隠し場所や無視対象を名指すと対象に含む。`no_ignore` を含む未知のキーは誤りになる。`mdq.toml` は読まない。README.md に `usage_log` の既定と制御方法がある。

## AI 担当への指示

[AI 担当向けの指示文の雛形](runbooks/mdrg-agent-usage.md) はプロジェクトの担当向け指示の文面。
