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

You'll need an [OpenAI API key](https://platform.openai.com/account/api-keys), set in the environment variable: `OPENAI_API_KEY`.

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

## OpenAI Privacy

[Managing Images](https://platform.openai.com/docs/guides/vision/managing-images) on the OpenAI API documentation:

```text
After an image has been processed by the model, it is deleted from OpenAI servers and not retained.
We do not use data uploaded via the OpenAI API to train our models.
````
