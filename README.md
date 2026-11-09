# mdq-rg

mdq-rg is a local, read-only CLI to search Markdown by heading and return small excerpts with `path:lines` source references for AI agents.

## Status

Released.

For installation, updates, and usage, see the [English guide](docs/GUIDE.md) or [Japanese guide](docs/GUIDE.ja.md).

## Features

The CLI provides:

- Search Markdown sections, retrieve excerpts, and list documents.
- Read source files on each search without a persistent index.
- Rank results using character bigrams for CJK text and words for Latin text.
- Return compact excerpts with source paths and line numbers.
- Record local usage logs by default without sending them externally.

The CLI does not edit documents or make network requests. Compatibility with the existing mdq command, output, configuration, and index formats is outside the scope.

## Usage logs

Usage logging is enabled by default for `search`, `get`, and `list`. Each command appends one JSON line to `$XDG_STATE_HOME/mdrg/usage.jsonl`. If `XDG_STATE_HOME` is unset or relative, the location is `$HOME/.local/state/mdrg/usage.jsonl`.

Each entry contains a timestamp, the CLI version, command name, working directory path, command arguments (including the full search query), result counts or a found flag (an empty result object on search execution failure), elapsed time, and exit status. Document bodies and search excerpts are not logged. Logs stay on your machine and are not sent externally. Argument parsing and configuration errors are not logged.

On Unix, new log directories use mode `0700` and new log files use mode `0600`, subject to the process umask. Existing permissions are preserved. Logging failures produce a `usage_log_failed` warning on standard error without changing the command's exit status.

To disable logging, set `usage_log = false` in `mdrg.toml` at the workspace root, or set the environment variable `MDRG_NO_USAGE_LOG=1`. When the root is home or an ancestor and causes `root_too_broad`, `mdrg.toml` is not read, so only `MDRG_NO_USAGE_LOG=1` disables that log entry.

## Supported platforms

- Apple Silicon macOS: the guaranteed target platform.
- Linux: tested in local arm64 and amd64 containers, without a support guarantee.
- Windows: out of scope.

## Building from source

With the Rust toolchain installed, run:

```sh
cargo build --release
```

The binary is `target/release/mdrg`.

## License

MIT OR Apache-2.0.

`src/data/variants.tsv` contains canonical decompositions of CJK compatibility ideographs derived from UnicodeData.txt in the Unicode Character Database 17.0.0, used under Unicode License v3. Its additional personal-name mappings were selected by this project, not derived from Unihan; some also appear in Unihan. The project's MIT OR Apache-2.0 license does not replace the Unicode terms. The permission notice from https://www.unicode.org/license.txt follows verbatim:

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2026 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

## Acknowledgements

This new design builds on the ideas of dahatake's [markdown-query](https://github.com/dahatake/skills). No code has been ported from that project.

Components from [ripgrep](https://github.com/BurntSushi/ripgrep), including `ignore`, are used.
