# mdrg guide

## Install and update

Homebrew installation on Apple Silicon macOS:

```sh
brew tap akira993/tap
brew trust --tap akira993/tap
brew install mdq-rg
```

Updates use `brew upgrade mdq-rg`; Cargo installs and updates use `cargo install mdq-rg --locked`. The executable is `mdrg`. README.md covers source builds.

## First search

The examples use a workspace with this five-line `docs/start.md`:

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

Search returns Start in `docs/start.md`, lines `[1,3]`. `get` accepts that `path` and `lines`. `list` returns it with `level: 1`.

## Commands

`mdrg --help` and command `--help` describe scope, options, and JSON Lines.

- `search` requires `--q`. Default `--mode rank` ranks sections. `--top-k` must be positive (default 5); `--max-tokens` defaults to 1300 estimated JSON tokens. `--timing` writes measurements as one plain-text stderr line.
- `get PATH:START-END` reads original lines, numbered from 1, including endpoints. END beyond the document is capped if START is inside; `lines` is the returned range.
- `list` lists sections. `--heading-level N` keeps levels at most N; `--limit N` caps rows and must be positive. Both default to unlimited.

`--mode exact` also returns sections. Both modes stop at the count or output budget; the first result may exceed the budget. They do not list every matching line.

`get` through a symbolic link matches its final target; successful `path` names that target. Cycles return `read_error` or `not_found`.

```sh
mdrg search --q install --mode exact --max-tokens 200 --timing
```

ripgrep lists all matching lines or counts them per file:

```sh
rg -n -F "text" -g '*.md'
rg -c -F "text" -g '*.md'
```

## Scope

The root defaults to cwd. `--root DIR` before or after the command selects an absolute or cwd-relative directory. Only its `mdrg.toml` is read, not a parent's. Parent `.gitignore` rules apply inside Git.

```sh
mdrg --root docs get 'start.md:1-3'
```

`search` and `list` use root-relative `--paths` patterns; quotes prevent shell expansion. Patterns form a union. In `--paths` and `exclude`, `*` matches `/` too. Outside paths cause `paths_outside_root` (exit 2), even alongside valid patterns. `error.next` suggests a common parent as `--root` and relative patterns. Absolute-only patterns inside the root give `paths_matched_nothing` with relative patterns in `next`, or `no_documents` if nothing is selected before `--paths`.

Only lowercase `.md` files qualify. All commands require `--hidden` for hidden files, except with `hidden = true` or a hidden directory named in `roots`; hidden entries inside it still need `--hidden`. `.ignore` and `.rgignore` apply inside and outside Git; `.gitignore` only inside. All commands' `--no-ignore` bypasses ignore rules and requires project permission. Configuration `exclude` still applies. Components below the root named `.git`, `node_modules`, `.ssh`, `.aws`, `.gnupg`, or `.env` are always excluded regardless of case.

## Output

Stdout is JSON Lines: one object per line. `path` is root-relative; `lines` includes both endpoints. `heading_path` joins headings with ` > `. Before the first heading, `heading_path` is `"(preface)"`; `list` has `level: 0`. Without headings or body, documents appear once in `list` with `lines: [0,0]`.

Search adds `score` and `snippet`; exact mode adds `match` (`phrase` or `words`). `get` adds original `text`; `list` adds heading `level`.

First `get` output:

```json
{"path":"docs/start.md","lines":[1,3],"heading_path":"Start","text":"# Start\nInstall the tool.\n\n"}
```

## Troubleshooting

Zero results leave stdout empty; stderr has `no_results` with `cause`, `detail`, and `next` hints:

- `no_documents`: no searchable documents under the root and filters.
- `paths_matched_nothing`: no documents match `--paths` after hidden, ignore, and exclude rules.
- `empty_query`: the query has no searchable words.
- `no_match`: no sections match words or mode, or documents have no text; `list` also filters by `--heading-level`. Exact search adds per-word section counts in `words`.

`get` guidance keeps arguments except a replaced `--root` (with an absolute document path), a symbolic link's PATH, an added `--hidden`, or the full-file range for `out_of_range`.

Stderr errors have `error.code` and `error.detail`. These add string array `error.next`:

- `outside_root` (`get`, exit 1): the path is outside the root. `next` uses the parent as `--root`; a missing parent causes `invalid_root` (exit 2). Home or ancestor parents get the guidance below. `outside_root` and `root_too_broad` prioritize always-excluded names: guidance says the path contains such a name and gives no reading method.
- `root_too_broad` (exit 2): all commands stop before walking if the resolved root is home or an ancestor. `next` gives `search`/`list` a project directory or `--root` below home, and `get` its document's parent as `--root`. For home or ancestor parents, it refuses under this scope and suggests a plain-text reader.
- `hidden` (`get`, exit 1): hidden Markdown would qualify with hidden enabled and required ignore rules bypassed. `next` adds `--hidden`. Without `--no-ignore`, matching ignore rules make that command return `ignored`.
- `out_of_range` (`get`, exit 1): START is outside the document. `detail` gives its line count; `next` is a full-file command. For empty documents, they report emptiness and no lines to read.
- `unsupported_type` (`get`, exit 1): the file is not a lowercase `.md` file.
- `ignored` (`get`, exit 1): an ignore rule excludes the file; `--no-ignore` (with the project's permission) permits reading it.
- `paths_outside_root` (exit 2): as in Scope. `invalid_root` (exit 2): the root is not an accessible directory.

Other `get` errors: `not_found` (exit 1: absent or excluded), `invalid_range` (exit 2: malformed or reversed bounds), `read_error` (exit 1: unreadable text or excessive link depth/cycles; link errors include `next`).

Stderr `skipped` names skipped paths and reasons: `invalid_utf8`, `contains_nul`, or `read_error`. `warning` has `code` and `detail`; README.md covers `usage_log_failed`.

Exit 0 includes zero results; 1 means execution failure; 2 means an argument or configuration error. Argument parsing and configuration loading errors use plain-text stderr; the range and scope errors above use JSON.

## Configuration

```toml
roots = ["docs"]
exclude = ["docs/drafts/**"]
hidden = false
```

Defaults: empty `roots` and `exclude`, `hidden = false`. Roots narrow selection and must be existing root-relative paths; named hidden or ignored roots are included. Unknown keys, including `no_ignore`, fail. `mdq.toml` is not read. README.md defines `usage_log` defaults and controls.

## AI instructions

[The agent template](runbooks/mdrg-agent-usage.md) supplies project agent instructions.
