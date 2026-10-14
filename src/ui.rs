use crate::projects::{self, Project};
use anyhow::{Result, ensure};
use crossterm::{
    cursor::{MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use filedescriptor::{FileDescriptor, StdioDescriptor};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

pub fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

struct Restore {
    stdout: Option<FileDescriptor>,
    top: Option<u16>,
}
impl Drop for Restore {
    fn drop(&mut self) {
        if let Some(y) = self.top {
            let _ = execute!(
                io::stderr(),
                MoveTo(0, y),
                Clear(ClearType::FromCursorDown),
                Show
            );
        }
        let _ = disable_raw_mode();
        if let Some(saved) = &self.stdout {
            let _ = FileDescriptor::redirect_stdio(saved, StdioDescriptor::Stdout);
        }
    }
}

pub fn pick(projects: &[Project], mut query: String, limit: usize) -> Result<Option<PathBuf>> {
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "the picker needs a terminal; use zort list or zort pick --query instead"
    );
    ensure!(
        !projects.is_empty(),
        "no projects found; visit directories with zoxide enabled first"
    );
    let mut restore = Restore {
        stdout: None,
        top: None,
    };
    enable_raw_mode()?;
    // Crossterm sends cursor-position queries to stdout; keep picker output on the tty.
    restore.stdout = Some(FileDescriptor::redirect_stdio(
        &io::stderr(),
        StdioDescriptor::Stdout,
    )?);
    let height = limit
        .min(projects.len())
        .saturating_add(5)
        .min(usize::from(crossterm::terminal::size()?.1)) as u16;
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(io::stderr()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;
    restore.top = Some(terminal.get_frame().area().y);
    let mut state = TableState::default();
    let mut previous_query = None;
    loop {
        let mut matches = projects::matching(projects, &query);
        let matching_count = matches.len();
        matches.truncate(limit);
        matches.reverse();
        if previous_query.as_ref() != Some(&query) {
            state.select(matches.len().checked_sub(1));
            previous_query = Some(query.clone());
        }
        terminal.draw(|f| {
            let areas = Layout::vertical([
                Constraint::Length(2),
                Constraint::Min(2),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(f.area());
            let cyan = Style::default().fg(Color::Cyan);
            let heading = Line::from(vec![
                Span::styled(
                    format!(" zort v{}", env!("CARGO_PKG_VERSION")),
                    cyan.add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("   {} directories", projects.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            f.render_widget(Paragraph::new(heading), areas[0]);
            let rows = matches.iter().map(|&i| {
                let p = &projects[i];
                Row::new(vec![
                    Cell::from(safe(&p.name)).style(Style::default().fg(Color::Green)),
                    Cell::from(projects::relative_time(p.last_accessed, projects::now()))
                        .style(Style::default().fg(Color::Blue)),
                    Cell::from(safe(p.branch.as_deref().unwrap_or("—"))).style(
                        Style::default().fg(if p.branch.is_some() {
                            Color::Magenta
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Cell::from(safe(&projects::display_path(&p.path)))
                        .style(Style::default().fg(Color::Cyan)),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(22),
                    Constraint::Length(18),
                    Constraint::Percentage(26),
                    Constraint::Min(12),
                ],
            )
            .header(
                Row::new(["DIRECTORY", "LAST ACCESSED", "BRANCH", "PATH"])
                    .style(Style::default().fg(Color::DarkGray)),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol("> ");
            let mut table_area = areas[1];
            let height = (matches.len().saturating_add(1) as u16).min(table_area.height);
            table_area.y += table_area.height - height;
            table_area.height = height;
            f.render_stateful_widget(table, table_area, &mut state);
            let footer = format!(
                " {} shown · {} matching   ↑↓ / Ctrl+N/P navigate · Enter jump · Esc cancel{}",
                matches.len(),
                matching_count,
                if matches.is_empty() {
                    " · No matches"
                } else {
                    ""
                }
            );
            f.render_widget(
                Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
                areas[2],
            );
            let prompt = format!(" Search › {}", safe(&query));
            let cursor = Line::from(prompt.as_str()).width();
            f.render_widget(Paragraph::new(prompt).style(cyan), areas[3]);
            if areas[3].width > 0 && areas[3].height > 0 {
                f.set_cursor_position((
                    areas[3].x + cursor.min(usize::from(areas[3].width - 1)) as u16,
                    areas[3].y,
                ));
            }
        })?;
        restore.top = Some(terminal.get_frame().area().y);
        if !event::poll(std::time::Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Char('c' | 'g') if control => return Ok(None),
            KeyCode::Enter => {
                if let Some(&i) = state.selected().and_then(|s| matches.get(s)) {
                    return Ok(Some(projects[i].path.clone()));
                }
            }
            KeyCode::Down | KeyCode::Tab => move_selection(&mut state, matches.len(), true),
            KeyCode::Up | KeyCode::BackTab => move_selection(&mut state, matches.len(), false),
            KeyCode::Char('n') if control => move_selection(&mut state, matches.len(), true),
            KeyCode::Char('p') if control => move_selection(&mut state, matches.len(), false),
            KeyCode::Char('u') if control => {
                query.clear();
            }
            KeyCode::Backspace => {
                query.pop();
            }
            KeyCode::Char(c) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                query.push(c);
            }
            _ => {}
        }
    }
}

fn move_selection(state: &mut TableState, count: usize, forward: bool) {
    if count == 0 {
        state.select(None);
        return;
    }
    let i = state.selected().unwrap_or(0).min(count - 1);
    state.select(Some(if forward {
        (i + 1) % count
    } else {
        (i + count - 1) % count
    }));
}
