<div align="center">
  <h1>zurf</h1>

  <p><strong>Switch projects with ranked search and Git context</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
    <img alt="Edition" src="https://img.shields.io/badge/edition-2024-blue">
    <a href="https://crates.io/crates/zsurf-cli"><img alt="crates.io" src="https://img.shields.io/crates/v/zsurf-cli.svg"></a>
  </p>

  <p>
    <a href="#install">Install</a>
    &nbsp;·&nbsp;
    <a href="#quickstart">Quickstart</a>
  </p>
</div>

---

## Install

Requires [Rust](https://rustup.rs) **1.85+** and `~/.cargo/bin` on your `PATH`.

```sh
cargo install zsurf-cli --locked
```

The crate installs the `zurf` executable.

Verify:

```bash
zurf --help
```

<details>
<summary><strong>Install from a local checkout</strong></summary>

```sh
cargo install --path . --locked
```

</details>

<a id="quickstart"></a>
## Quickstart

Requires [zoxide](https://github.com/ajeetdsouza/zoxide). Keep its shell hook enabled so every directory visit is recorded. Add these lines to your `~/.zshrc`, in this order:

```sh
eval "$(zoxide init zsh)"
eval "$(zurf init zsh --cmd zs --bind ctrl-g --limit 10)"
```

Press **Ctrl+G** or run `zs` to open the picker. Type to fuzzy-search, navigate with arrows or Ctrl+N/P, and press Enter to change directory. Escape cancels. The shortcut preserves any command already typed at the prompt.

```sh
zs                  # Open the picker
zs website            # Start with a search
zs --query website    # Jump to the best match
zs --limit 5        # Show up to five results
```

By default, zurf shows your top **10 directories**, ranked by frequency and recency. The best match is selected at the bottom of the list. Typing searches your full history, including directories outside the initial ten.

Search directory names and Git branches, or include `/` to search paths. Each result opens the exact directory you visited, with its current Git branch and the last visit recorded by zoxide.

To change the command, shortcut, or default result limit, adjust the `zurf init` line above. Shell integration currently supports **zsh**.

## License

MIT
