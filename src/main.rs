mod projects;
mod shell;
mod ui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Switch projects with ranked search and Git context")]
struct Cli {
    /// Maximum results shown in the picker
    #[arg(long, global = true, env = "ZORT_LIMIT", default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..))]
    limit: u16,
    #[command(subcommand)]
    command: Option<Commands>,
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
    /// Print shell integration for eval
    Init {
        #[arg(value_parser = ["zsh"])]
        shell: String,
        #[arg(long, default_value = "j")]
        cmd: String,
        /// Shortcut such as ctrl-g; use none to disable
        #[arg(long, default_value = "ctrl-g")]
        bind: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Pick {
        query: false,
        terms: vec![],
    }) {
        Commands::Init { cmd, bind, .. } => print!("{}", shell::init(&cmd, &bind, cli.limit)?),
        Commands::List { json, terms } => {
            let rows = projects::load()?;
            let filtered = projects::matching(&rows, &terms.join(" "));
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
                projects::matching(&rows, &search)
                    .first()
                    .map(|&i| rows[i].path.clone())
            } else {
                ui::pick(&rows, search, usize::from(cli.limit))?
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
