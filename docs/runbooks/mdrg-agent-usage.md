# Searching Markdown documents with mdrg

Run these commands from the workspace root. Follow project-specific instructions. When a question spans two or more Markdown documents, search for the relevant sections before reading whole files.

```sh
mdrg search --q "what to look up" --top-k 5
mdrg get "docs/example.md:10-24"
```

Pass the `path` and `lines` of a search result to `get`. You may request an END beyond the document; if START is inside it, output stops at the last line. Check the real file before you base an edit or a decision on a result. When you need all of one file, read that file directly.

To search or read another directory without changing the working directory, pass `--root DIR`; `--paths` patterns are then relative to that directory. A root that resolves to home or an ancestor stops all three commands with `root_too_broad`. When a failure includes `error.next`, follow it. Argument parsing and configuration loading errors are plain text; correct the indicated argument or TOML location.

To list or count every matching line, use `rg -n -F "text" -g '*.md'` or `rg -c -F "text" -g '*.md'`. This also applies across two or more Markdown documents. `mdrg search --mode exact` returns sections and stops at the result count and output budget; it does not list or count every matching line.

For hidden directories such as `.claude/`, pass `--hidden` on both `search` and `get`, unless `mdrg.toml` sets `hidden = true` or names the directory in `roots` (hidden entries inside a named directory still need `--hidden`). Below the working directory or `--root`, mdrg never searches paths with a component named `.git`, `node_modules`, `.ssh`, `.aws`, `.gnupg`, or `.env`, regardless of case.

```sh
mdrg search --hidden --paths ".claude/**" --q "the rule to look up" --top-k 5
mdrg get --hidden ".claude/rules/example.md:1-10"
```

Zero results are not a success. Read `no_results` and any `skipped` lines on standard error, then check the query, the target paths, the ignore rules, and the documents that could not be read. If mdrg is not installed, say so and use the text search and file reading tools you have.

mdrg needs no index and no setup step. It does not read `mdq.toml`. Set the search scope with `--paths` for one command, or with `roots` and `exclude` in `mdrg.toml` at the workspace root. Follow the approval process of the project before you change that file.

mdrg searches files with the lowercase `.md` extension. It skips hidden files by default.

Logs that Git ignores, such as `docs/logs/`, do not appear in results unless `roots` in `mdrg.toml` names their directory. To search them without that setting, run `rg -n "term" docs/logs` and read the file directly.

Git ignore rules apply only inside a Git working tree; outside one, documents your `.gitignore` would exclude appear in results. Do not use `--no-ignore` without the user's permission, and check first that the target contains no secrets and no generated files.
