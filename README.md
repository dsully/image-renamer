# OpenAI Vision Image Renamer

Rename images and screen shots using OpenAI Vision

## Installation

```shell
brew install dsully/tap/image-renamer
```

Or from source:

```shell
cargo install --git https://github.com/dsully/image-renamer
```

## Getting Started

```shell
Usage: image-renamer [OPTIONS] [PATHS]...

Arguments:
  [PATHS]...

Options:
  -p, --prompt   Prompt to rename or revert each file
  -r, --revert   Revert file(s) to the original name(s)
  -h, --help     Print help
  -V, --version  Print version
```

## Revert State

A mapping of new files to original names is in`$XDG_DATA_HOME/image-renamer/revert-mappings.json`
