# Contributing

Any and all contributions are entirely welcomed! Before you contribute though, there are
some things you should know.

> [!NOTE]
> Making public contributions to this repo means you accept the [LICENSE](LICENSE) agreement, and you're contributing
> code that also respects the [LICENSE](LICENSE) agreement.

## AI Policy

AI use is totally fine, but make sure it adheres to our [AI policy](AI_POLICY.md).

## Developing

We use [cargo-make](https://crates.io/crates/cargo-make) for our scripts.

Ensure you have it installed, if you don't already.

```sh
cargo install --force cargo-make
```

### Building

Use the `build` command to build the source files.

```sh
cargo make build
```

### Running Tests

Use the `test` command to run the unit tests and doc tests.

```sh
cargo make test
```

### API Docs

Use the `docs` command to build the API docs.

```sh
cargo make docs
```

## Making changes

Before opening any PRs, ensure your code is following the proper practices outlined below.

### Changelogs

We use [release-plz](https://github.com/release-plz/release-plz) for our release notes and version bumping.

When submitting a change, please ensure you PR follows
proper [conventional commits](https://www.conventionalcommits.org/en/v1.0.0/).

This is not required for individual commits in your PR itself, as all PRs are squashed before merged.

### Code Formatting

Code in this repo is formatted according to `rustfmt`, which you can run with the `format` command.

```sh
cargo make format
```

### Licensing

We apply license headers via the [add-notice](https://github.com/ameknite/add-notice) crate.

You can invoke this automatically with the `license` command.

```sh
cargo make license
```

