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

The examples run from a workspace containing five lines in `docs/start.md`:

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

Search returns Start in `docs/start.md`, lines `[1,3]`. `get` accepts this `path` and `lines`. The list returns it with `level: 1`.

## Commands

`mdrg --help` and command `--help` describe scope, options, and JSON Lines.

- `search` requires `--q`. Default `--mode rank` ranks sections. `--top-k` must be positive (default 5); `--max-tokens` defaults to 1300 estimated JSON tokens. The first result can exceed that budget. `--timing` writes measurements as one plain-text stderr line.
- `get PATH:START-END` reads original lines, numbered from 1, including endpoints. END beyond the document is capped if START is inside; `lines` is the returned range.
- `list` lists sections. `--heading-level N` keeps levels at most N; `--limit N` caps rows and must be positive. Both default to unlimited.

`--mode exact` also returns sections. Both modes stop at the count or output budget, without listing every matching line.

```sh
mdrg search --q install --mode exact --max-tokens 200 --timing
```

These ripgrep commands list all matching lines or count them per file:

```sh
rg -n -F "text" -g '*.md'
rg -c -F "text" -g '*.md'
```

## Scope

The root defaults to the current working directory (cwd). `--root DIR` before or after the command selects an absolute or cwd-relative directory. Only its `mdrg.toml` is read, not a parent's. Parent `.gitignore` rules still apply inside Git.

```sh
mdrg --root docs get 'start.md:1-3'
```

`search` and `list` use root-relative `--paths` patterns; quotes prevent shell expansion. Patterns form a union. In `--paths` and `exclude`, `*` matches `/` too. Outside paths cause `paths_outside_root` (exit 2), even when mixed with valid patterns. `error.next` directs a retry with a common parent as `--root` and relative patterns. Absolute `--paths` inside the root give zero results, `paths_matched_nothing`, and relative patterns in `next`.

Only lowercase `.md` files qualify. All three commands require `--hidden` for hidden files, except with `hidden = true` or for a hidden directory named in `roots`; hidden entries inside it still need `--hidden`. `.ignore` and `.rgignore` apply inside and outside Git; `.gitignore` only inside. All three commands' `--no-ignore` bypasses ignore rules and requires the project's permission. Configuration `exclude` still applies. Components below the root named `.git`, `node_modules`, `.ssh`, `.aws`, `.gnupg`, or `.env` are always excluded, regardless of case.

## Output

The commands emit one JSON object per stdout line (JSON Lines), not an array. `path` is root-relative; `lines` includes both endpoints. `heading_path` joins headings with ` > `. Text before the first heading has `heading_path: "(preface)"` and `level: 0` in `list`. Documents without headings or body appear once in `list` with `lines: [0,0]`.

Search adds `score` and `snippet`; exact mode adds `match` (`phrase` or `words`). `get` adds original `text`; `list` adds heading `level`.

First `get` output:

```json
{"path":"docs/start.md","lines":[1,3],"heading_path":"Start","text":"# Start\nInstall the tool.\n\n"}
```

## Troubleshooting

Zero results leave stdout empty; stderr has `no_results` with `cause`, `detail`, and `next` hints:

- `no_documents`: no searchable documents under the root and filters.
- `paths_matched_nothing`: no selected documents (after hidden, ignore, and exclude rules) match `--paths`.
- `empty_query`: the query has no searchable words.
- `no_match`: no sections match words or mode, or documents have no text; `list` also filters by `--heading-level`. Exact search adds per-word section counts in `words`.

`get` guidance commands preserve the original arguments except for a replaced `--root` (with an absolute document path) and the full-document range for `out_of_range`.

Structured stderr has `error.code` and `error.detail`. These add string array `error.next`:

- `outside_root` (`get`, exit 1): the path is outside the root. `next` uses the parent as `--root`; a missing parent causes `invalid_root` (exit 2). Home or ancestor parents get the guidance below. For `outside_root` and `root_too_broad`, always-excluded components take priority: guidance states that the path contains an always-excluded name, with no reading method.
- `root_too_broad` (exit 2): all commands stop before walking if the resolved root is home or an ancestor. `next` gives `search`/`list` a project directory or `--root` below home, and `get` its document's parent as `--root`. If that parent is home or an ancestor, it reports refusal under this selection and suggests a plain-text reader.
- `hidden` (`get`, exit 1): hidden Markdown would qualify with hidden enabled and required ignore rules bypassed. `next` adds `--hidden`. Without `--no-ignore`, ignore-rule matches make the guidance command return `ignored`.
- `out_of_range` (`get`, exit 1): START is outside the document. `detail` gives its line count; `next` is a full-file command. For empty documents, they report emptiness and no lines to read.
- `unsupported_type` (`get`, exit 1): the file is not a lowercase `.md` file.
- `ignored` (`get`, exit 1): an ignore rule excludes the file; `--no-ignore` (with the project's permission) permits reading it.
- `paths_outside_root` (exit 2): as in Scope. `invalid_root` (exit 2): the root is not an accessible directory.

Other `get` errors are `not_found` (exit 1: absent or excluded), `invalid_range` (exit 2: malformed or reversed bounds), and `read_error` (exit 1: unreadable text).

Stderr `skipped` names skipped paths and reasons: `invalid_utf8`, `contains_nul`, or `read_error`. `warning` has `code` and `detail`; README.md covers `usage_log_failed`.

Exit 0 includes zero results; 1 means execution failure; 2 means an argument or configuration error. Argument parsing and configuration loading errors are plain text on stderr. The range and scope errors above use JSON.

## Configuration

```toml
roots = ["docs"]
exclude = ["docs/drafts/**"]
hidden = false
```

Defaults: empty `roots` and `exclude`, `hidden = false`. Roots narrow the selection and must be existing relative paths within the root; named hidden or ignored roots are included. Unknown keys, including `no_ignore`, fail. `mdq.toml` is not read. README.md defines `usage_log` defaults and controls.

## AI instructions

[The agent template](runbooks/mdrg-agent-usage.md) supplies project agent instructions.
