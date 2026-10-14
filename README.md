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
zs website                         # Start with a search
zs --limit 5                     # Show at most five results
zs --query website                 # Jump to the best match directly
zurf                            # Open picker; prints the chosen path
zurf list                       # Inspect projects without opening the picker
zurf list --json                 # Include numeric scores and Unix timestamps
zurf init zsh --cmd p --bind ctrl-o
zurf init zsh --cmd zs --bind none # Define the command without a shortcut
```

The picker uses a compact inline panel with up to **10 results** by default. The highest-ranked match is selected at the bottom of the list, nearest the search field. Press Up or Ctrl+P to reach the next-best match. Empty search shows your highest-ranked directories, and typing searches your entire history before applying the limit. The shell setup above explicitly sets the default limit to 10. To customize it, pass a different `--limit` to `zurf init` or set `ZURF_LIMIT`. `zurf list` always includes the full history. Search matches project names and Git branches; include `/` or start with `~` to search paths. Each visited directory keeps its exact path, zoxide score, and access time. Git metadata is detected from its containing repository without changing the jump destination. Git worktrees stay separate. Detached HEAD shows a short commit identifier; non-Git directories have no branch.

Last access means the latest visit recorded by zoxide, for that exact directory. Both the released binary database and upstream text database are supported. Unknown metadata formats show `unknown` with a diagnostic; ranking still comes from `zoxide query`. `_ZO_DATA_DIR` and zoxide's configured exclusions are respected.

Shell integration currently supports **zsh**. `zurf init` prints shell code; it does not edit your shell configuration. The picker draws on stderr, leaving stdout available for the selected path. Cancellation returns status 1 and never changes directory.

## License

MIT
