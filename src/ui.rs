use crate::{
    Sort, git_status,
    preferences::Preferences,
    projects::{self, Project},
};
use anyhow::{Result, ensure};
use crossterm::{
    cursor::{MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use filedescriptor::{FileDescriptor, StdioDescriptor};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState},
};
use std::collections::{HashMap, HashSet};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

pub fn ordered(projects: &[Project], query: &str, prefs: &Preferences, sort: Sort) -> Vec<usize> {
    let mut matches: Vec<_> = projects::matching(projects, query)
        .into_iter()
        .filter(|&i| !prefs.is_hidden(&projects[i].identity))
        .collect();
    matches.sort_by(|&a, &b| {
        prefs
            .is_pinned(&projects[b].path)
            .cmp(&prefs.is_pinned(&projects[a].identity))
            .then_with(|| {
                if sort == Sort::Recent {
                    projects[b].last_accessed.cmp(&projects[a].last_accessed)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
    });
    matches
}

fn highlighted(text: &str, query: &str, color: Color) -> Line<'static> {
    let text = safe(text);
    let indices = if query.is_empty() {
        vec![]
    } else {
        SkimMatcherV2::default()
            .ignore_case()
            .fuzzy_indices(&text, query)
            .map(|(_, indices)| indices)
            .unwrap_or_default()
    };
    Line::from(
        text.chars()
            .enumerate()
            .map(|(i, c)| {
                let style = if indices.contains(&i) {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    Style::default().fg(color)
                };
                Span::styled(c.to_string(), style)
            })
            .collect::<Vec<_>>(),
    )
}

fn shorten_path(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width < 2 {
        return if width == 0 {
            String::new()
        } else {
            "…".into()
        };
    }
    let mut suffix = String::new();
    for part in text.rsplit('/') {
        let candidate = if suffix.is_empty() {
            part.to_owned()
        } else {
            format!("{part}/{suffix}")
        };
        if candidate.width() + 2 > width {
            break;
        }
        suffix = candidate;
    }
    if suffix.is_empty() {
        let mut used = 0;
        suffix = text
            .chars()
            .rev()
            .take_while(|c| {
                used += c.width().unwrap_or(0);
                used <= width - 2
            })
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    format!("…/{suffix}")
}

pub fn pick(
    projects: &[Project],
    mut query: String,
    limit: usize,
    mut sort: Sort,
    show_git_status: bool,
) -> Result<Option<PathBuf>> {
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "the picker needs a terminal; use zurf list or zurf pick --query instead"
    );
    ensure!(
        !projects.is_empty(),
        "no projects found; visit directories with zoxide enabled first"
    );
    ensure!(
        crossterm::terminal::size()?.1 >= 6,
        "the picker needs at least 6 terminal rows"
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
    let mut selection_override = None;
    let mut prefs = Preferences::load()?;
    let worker = show_git_status.then(git_status::start);
    let mut git_cache = HashMap::new();
    let mut pending = HashSet::new();
    let mut undo_hidden = None;
    let mut notice = String::new();
    loop {
        if let Some(worker) = &worker {
            while let Ok((path, status)) = worker.results.try_recv() {
                pending.remove(&path);
                git_cache.insert(path, status);
            }
        }
        let mut matches = ordered(projects, &query, &prefs, sort);
        let matching_count = matches.len();
        let available = usize::from(crossterm::terminal::size()?.1.min(height)).saturating_sub(5);
        ensure!(available > 0, "the picker needs at least 6 terminal rows");
        matches.truncate(limit.min(available));
        matches.reverse();
        if previous_query.as_ref() != Some(&query) {
            state.select(matches.len().checked_sub(1));
            previous_query = Some(query.clone());
        }
        if let Some(path) = selection_override.take() {
            state.select(
                matches
                    .iter()
                    .position(|&i| projects[i].path == path)
                    .or_else(|| matches.len().checked_sub(1)),
            );
        }
        if let Some(worker) = &worker {
            if let Some(&i) = state.selected().and_then(|s| matches.get(s)) {
                if let Some(root) = projects[i].git_root.as_ref() {
                    if !git_cache.contains_key(root)
                        && !pending.contains(root)
                        && worker.requests.try_send(root.clone()).is_ok()
                    {
                        pending.insert(root.clone());
                    }
                }
            }
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
                    format!(" zurf v{}", env!("CARGO_PKG_VERSION")),
                    cyan.add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("   {} directories", projects.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            f.render_widget(Paragraph::new(heading), areas[0]);
            let constraints = [
                Constraint::Length(3), Constraint::Percentage(20), Constraint::Length(18),
                Constraint::Percentage(24), Constraint::Min(12),
            ];
            let columns = Layout::horizontal(constraints).spacing(1).split(ratatui::layout::Rect::new(0, 0, f.area().width.saturating_sub(2), 1));
            let path_width = usize::from(columns[4].width);
            let rows = matches.iter().enumerate().map(|(position, &i)| {
                let p = &projects[i];
                let mut name = highlighted(&p.name, &query, Color::Green);
                if prefs.is_pinned(&p.identity) { name.spans.insert(0, Span::styled("★ ", Style::default().fg(Color::Yellow))); }
                let status = p.git_root.as_ref().and_then(|root| git_cache.get(root)).and_then(|s| s.as_ref());
                let branch = format!("{}{}", status.map(|s| {
                    let s = s.to_string(); if s.is_empty() { s } else { format!("{s} ") }
                }).unwrap_or_default(), p.branch.as_deref().unwrap_or("—"));
                Row::new(vec![
                    Cell::from((matches.len() - position).to_string()).style(Style::default().fg(Color::DarkGray)),
                    Cell::from(name),
                    Cell::from(projects::relative_time(p.last_accessed, projects::now())).style(Style::default().fg(Color::Blue)),
                    Cell::from(highlighted(&branch, &query, if p.branch.is_some() { Color::Magenta } else { Color::DarkGray })),
                    Cell::from(highlighted(&shorten_path(&safe(&projects::display_path(&p.path)), path_width), &query, Color::Cyan)),
                ])
            });
            let table = Table::new(rows, constraints)
                .header(Row::new(["", "DIRECTORY", "LAST ACCESSED", "BRANCH", "PATH"]).style(Style::default().fg(Color::DarkGray)))
                .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
                .highlight_symbol("> ");
            let mut table_area = areas[1];
            let height = (matches.len().saturating_add(1) as u16).min(table_area.height);
            table_area.y += table_area.height - height;
            table_area.height = height;
            f.render_stateful_widget(table, table_area, &mut state);
            let footer = if notice.is_empty() {
                format!(" {} shown · {} matching  Enter jump · Alt+1…9 · ^S sort · ^B pin · ^X hide · Esc{}",
                    matches.len(), matching_count, if matches.is_empty() { " · No matches" } else { "" })
            } else { notice.clone() };
            f.render_widget(
                Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)),
                areas[2],
            );
            let prompt = format!(" Search [{}] › {}", if sort == Sort::Frequent { "Frequent" } else { "Recent" }, safe(&query));
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
        notice.clear();
        match key.code {
            KeyCode::Char(c)
                if key.modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() && c != '0' =>
            {
                let rank = c.to_digit(10).unwrap() as usize;
                if let Some(&i) = matches
                    .len()
                    .checked_sub(rank)
                    .and_then(|index| matches.get(index))
                {
                    return Ok(Some(projects[i].path.clone()));
                }
            }
            KeyCode::Char('s') if control => {
                sort = if sort == Sort::Frequent {
                    Sort::Recent
                } else {
                    Sort::Frequent
                };
                previous_query = None;
            }
            KeyCode::Char('b') if control => {
                if let Some(&i) = state.selected().and_then(|s| matches.get(s)) {
                    selection_override = Some(projects[i].path.clone());
                    prefs = Preferences::set_pin(
                        &projects[i].path,
                        !prefs.is_pinned(&projects[i].identity),
                    )?;
                    previous_query = None;
                }
            }
            KeyCode::Char('x') if control => {
                if let Some(&i) = state.selected().and_then(|s| matches.get(s)) {
                    let was_pinned = prefs.is_pinned(&projects[i].identity);
                    prefs = Preferences::set_hidden(&projects[i].path, true)?;
                    undo_hidden = Some((projects[i].path.clone(), was_pinned));
                    previous_query = None;
                    notice =
                        " Hidden directory · Ctrl+Z undo · zurf hidden lists hidden entries".into();
                }
            }
            KeyCode::Char('z') if control => {
                if let Some((path, pinned)) = undo_hidden.take() {
                    prefs = Preferences::set_hidden(&path, false)?;
                    if pinned {
                        prefs = Preferences::set_pin(&path, true)?;
                    }
                    previous_query = None;
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_paths_preserve_destination_and_terminal_width() {
        assert_eq!(shorten_path("/some/long/path/project", 12), "…/project");
        assert_eq!(shorten_path("/路径/项目", 7), "…/项目");
        assert_eq!(shorten_path("/a", 8), "/a");
        assert_eq!(shorten_path("/long/name", 1), "…");
        assert_eq!(shorten_path("/long/name", 0), "");
        assert!(shorten_path("/路径/很长项目", 5).width() <= 5);
    }
    #[test]
    fn fuzzy_highlighting_marks_matched_unicode_characters() {
        let line = highlighted("项目-alpha", "项a", Color::Green);
        let marked: String = line
            .spans
            .iter()
            .filter(|s| s.style.fg == Some(Color::Yellow))
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(marked, "项a");
        assert_eq!(line.to_string(), "项目-alpha");
    }
}
