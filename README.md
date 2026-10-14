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
zs website          # Start with a search
zs --query website  # Jump to the best match
zs --limit 5        # Show up to five results
```

By default, zurf shows your top **10 directories**, ranked by frequency and recency. The best match is selected at the bottom of the list. Typing searches your full history, including directories outside the initial ten.

Search directory names and Git branches, or include `/` to search paths. Each result opens the exact directory you visited, with its current Git branch and the last visit recorded by zoxide.

To change the command, shortcut, or default result limit, adjust the `zurf init` line above. Shell integration currently supports **zsh**.

## Shortcuts

| Key | Action |
| --- | --- |
| **Alt+1–9** | Jump directly to a numbered result; **1** is the best match at the bottom |
| **Ctrl+S** | Toggle Frequent / Recent ordering |
| **Ctrl+B** | Pin or unpin the selected directory (`★`) |
| **Ctrl+X** | Hide the selected directory |
| **Ctrl+Z** | Undo the last hide, restoring its pin if it had one |

Search matches are highlighted. Long paths shorten from the left so the destination stays visible. Pins stay available even after a directory drops out of zoxide's history. Hidden entries can be restored; they remain in zoxide.

```sh
zs -                   # Return to the previous directory; repeat to switch back
zurf pin /path/to/app   # Pin a directory (omit the path for the current one)
zurf unpin /path/to/app
zurf hidden            # List hidden directories
zurf unhide /path/to/app
```

For a persistent Recent sort, add `--sort recent` to the setup line. Frequent is the default, and pins come first in either mode. Pins and hidden entries are saved locally in zurf's configuration directory.

Git status is optional and loaded in the background for the selected repository:

```sh
eval "$(zurf init zsh --cmd zs --bind ctrl-g --limit 10 --git-status)"
```

`●` means uncommitted changes (including untracked files); `↑2` / `↓1` mean commits ahead / behind the upstream branch. Git checks time out after two seconds and are cancelled when the picker closes.

## License

MIT
