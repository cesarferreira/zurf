mod git_status;
mod preferences;
mod projects;
mod shell;
mod ui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "zurf",
    version,
    about = "Switch projects with ranked search and Git context"
)]
struct Cli {
    /// Maximum results shown in the picker
    #[arg(long, global = true, env = "ZURF_LIMIT", default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..))]
    limit: u16,
    /// Result ordering
    #[arg(long, global = true, env = "ZURF_SORT", value_enum, default_value_t = Sort::Frequent)]
    sort: Sort,
    /// Load Git dirty/ahead/behind indicators in the background
    #[arg(long, global = true, env = "ZURF_GIT_STATUS")]
    git_status: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum Sort {
    Frequent,
    Recent,
}
impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::Frequent => "frequent",
            Self::Recent => "recent",
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Open the interactive project picker (also the default action)
    Pick {
        /// Print the best match without opening the picker
        #[arg(long)]
        query: bool,
        /// Initial fuzzy search
        terms: Vec<String>,
    },
    /// List projects with their Git branch and last access time
    List {
        #[arg(long)]
        json: bool,
        terms: Vec<String>,
    },
    /// Pin a directory (defaults to the current directory)
    Pin {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Remove a directory's pin
    Unpin {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Hide a directory from results
    Hide {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Restore a hidden directory
    Unhide {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// List hidden directories
    Hidden,
    /// Print shell integration for eval
    Init {
        #[arg(value_parser = ["zsh"])]
        shell: String,
        #[arg(long, default_value = "zs")]
        cmd: String,
        /// Shortcut such as ctrl-g; use none to disable
        #[arg(long, default_value = "ctrl-g")]
        bind: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(command) = &cli.command {
        let change = match command {
            Commands::Pin { path } => Some((path, true, true)),
            Commands::Unpin { path } => Some((path, true, false)),
            Commands::Hide { path } => Some((path, false, true)),
            Commands::Unhide { path } => Some((path, false, false)),
            _ => None,
        };
        if let Some((path, pin, enabled)) = change {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()?.join(path)
            };
            let path = std::fs::canonicalize(&path).unwrap_or(path);
            if pin {
                preferences::Preferences::set_pin(&path, enabled)?;
            } else {
                preferences::Preferences::set_hidden(&path, enabled)?;
            }
            return Ok(());
        }
    }
    match cli.command.unwrap_or(Commands::Pick {
        query: false,
        terms: vec![],
    }) {
        Commands::Init { cmd, bind, .. } => print!(
            "{}",
            shell::init(&cmd, &bind, cli.limit, cli.sort.label(), cli.git_status)?
        ),
        Commands::Pin { .. }
        | Commands::Unpin { .. }
        | Commands::Hide { .. }
        | Commands::Unhide { .. } => unreachable!("handled before dispatch"),
        Commands::Hidden => {
            for path in preferences::Preferences::load()?.hidden {
                println!("{}", path.display());
            }
        }
        Commands::List { json, terms } => {
            let rows = projects::load()?;
            let filtered = ui::ordered(
                &rows,
                &terms.join(" "),
                &preferences::Preferences::load()?,
                cli.sort,
            );
            if json {
                let rows: Vec<_> = filtered.iter().map(|&i| &rows[i]).collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                println!(
                    "{:<24} {:<24} {:<18} PATH",
                    "PROJECT", "BRANCH", "LAST ACCESSED"
                );
                for i in filtered {
                    let p = &rows[i];
                    println!(
                        "{:<24} {:<24} {:<18} {}",
                        ui::safe(&p.name),
                        ui::safe(p.branch.as_deref().unwrap_or("—")),
                        projects::relative_time(p.last_accessed, projects::now()),
                        ui::safe(&projects::display_path(&p.path))
                    );
                }
            }
        }
        Commands::Pick { query, terms } => {
            let rows = projects::load()?;
            let search = terms.join(" ");
            let selected = if query {
                ui::ordered(&rows, &search, &preferences::Preferences::load()?, cli.sort)
                    .first()
                    .map(|&i| rows[i].path.clone())
            } else {
                ui::pick(
                    &rows,
                    search,
                    usize::from(cli.limit),
                    cli.sort,
                    cli.git_status,
                )?
            };
            match selected {
                Some(path) => println!("{}", path.display()),
                None if !query => std::process::exit(1),
                None => bail!("no project matched the query"),
            }
        }
    }
    Ok(())
}
