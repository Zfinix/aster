//! Shared interactive multi-select, the `npx skills`-style widget used by
//! `skills add` and the import flows.

use std::io;

use anyhow::Result;
use console::{Key, Term, style};

/// Rows shown at once; longer lists scroll under the cursor.
const MAX_VISIBLE: usize = 12;

pub struct Item {
    pub name: String,
    pub detail: String,
}

/// Arrows or j/k move, space toggles, `a` toggles all, `i` inverts, enter confirms,
/// esc cancels (`None`). Custom because cliclack's own multiselect has no
/// select-all and cannot be extended.
pub fn multi_select(title: &str, items: &[Item], preselect: bool) -> Result<Option<Vec<usize>>> {
    let term = Term::stderr();
    let visible = items.len().min(MAX_VISIBLE);
    let rows = visible + 2;
    let mut cursor = 0usize;
    let mut top = 0usize;
    let mut checked = vec![preselect; items.len()];

    term.hide_cursor()?;
    let mut drawn = false;
    let result = loop {
        if cursor < top {
            top = cursor;
        }
        if cursor >= top + visible {
            top = cursor + 1 - visible;
        }
        if drawn {
            term.clear_last_lines(rows)?;
        }
        render(&term, title, items, &checked, cursor, top, visible)?;
        drawn = true;

        match term.read_key()? {
            Key::ArrowUp | Key::Char('k') => {
                cursor = cursor.checked_sub(1).unwrap_or(items.len() - 1);
            }
            Key::ArrowDown | Key::Char('j') => {
                cursor = (cursor + 1) % items.len();
            }
            Key::Char(' ') => checked[cursor] = !checked[cursor],
            Key::Char('a') | Key::Char('A') => {
                let all = checked.iter().all(|&c| c);
                checked.iter_mut().for_each(|c| *c = !all);
            }
            Key::Char('i') | Key::Char('I') => checked.iter_mut().for_each(|c| *c = !*c),
            Key::Enter => {
                let chosen = (0..items.len()).filter(|&i| checked[i]).collect();
                break Some(chosen);
            }
            Key::Escape | Key::CtrlC => break None,
            _ => {}
        }
    };
    term.clear_last_lines(rows)?;
    term.show_cursor()?;
    Ok(result)
}

pub enum Action {
    Open,
    Delete,
}

/// Single-select sibling of `multi_select`: arrows or j/k move, enter opens
/// the highlighted row, `d` deletes it, esc closes (`None`).
pub fn select_action(title: &str, items: &[Item]) -> Result<Option<(usize, Action)>> {
    let term = Term::stderr();
    let visible = items.len().min(MAX_VISIBLE);
    let rows = visible + 2;
    let mut cursor = 0usize;
    let mut top = 0usize;

    term.hide_cursor()?;
    let mut drawn = false;
    let result = loop {
        if cursor < top {
            top = cursor;
        }
        if cursor >= top + visible {
            top = cursor + 1 - visible;
        }
        if drawn {
            term.clear_last_lines(rows)?;
        }
        render_single(&term, title, items, cursor, top, visible)?;
        drawn = true;

        match term.read_key()? {
            Key::ArrowUp | Key::Char('k') => {
                cursor = cursor.checked_sub(1).unwrap_or(items.len() - 1);
            }
            Key::ArrowDown | Key::Char('j') => {
                cursor = (cursor + 1) % items.len();
            }
            Key::Enter => break Some((cursor, Action::Open)),
            Key::Char('d') | Key::Char('D') => break Some((cursor, Action::Delete)),
            Key::Escape | Key::CtrlC => break None,
            _ => {}
        }
    };
    term.clear_last_lines(rows)?;
    term.show_cursor()?;
    Ok(result)
}

fn render_single(
    term: &Term,
    title: &str,
    items: &[Item],
    cursor: usize,
    top: usize,
    visible: usize,
) -> io::Result<()> {
    let width = term.size().1 as usize;
    term.write_line(&format!(
        "{}  {}",
        style(title).bold(),
        style(format!("({})", items.len())).dim()
    ))?;
    for (i, item) in items.iter().enumerate().skip(top).take(visible) {
        let pointer = if i == cursor {
            style("❯").cyan().to_string()
        } else {
            " ".to_string()
        };
        let name = if i == cursor {
            style(&item.name).cyan().bold().to_string()
        } else {
            item.name.clone()
        };
        let head = format!("{pointer} {name}  ");
        let room = width.saturating_sub(console::measure_text_width(&head) + 1);
        let detail = console::truncate_str(first_line(&item.detail), room, "…");
        term.write_line(&format!("{head}{}", style(detail).dim()))?;
    }
    let scroll = if items.len() > visible {
        format!("{}-{} of {} · ", top + 1, top + visible, items.len())
    } else {
        String::new()
    };
    term.write_line(
        &style(format!("{scroll}enter open · d delete · esc close"))
            .dim()
            .to_string(),
    )?;
    Ok(())
}

fn render(
    term: &Term,
    title: &str,
    items: &[Item],
    checked: &[bool],
    cursor: usize,
    top: usize,
    visible: usize,
) -> io::Result<()> {
    let width = term.size().1 as usize;
    let selected = checked.iter().filter(|&&c| c).count();
    term.write_line(&format!(
        "{}  {}",
        style(title).bold(),
        style(format!("({selected}/{} selected)", items.len())).dim()
    ))?;
    for (i, item) in items.iter().enumerate().skip(top).take(visible) {
        let pointer = if i == cursor {
            style("❯").cyan().to_string()
        } else {
            " ".to_string()
        };
        let mark = if checked[i] {
            style("◼").green().to_string()
        } else {
            style("◻").dim().to_string()
        };
        let name = if i == cursor {
            style(&item.name).cyan().bold().to_string()
        } else {
            item.name.clone()
        };
        let head = format!("{pointer} {mark} {name}  ");
        let room = width.saturating_sub(console::measure_text_width(&head) + 1);
        let detail = console::truncate_str(first_line(&item.detail), room, "…");
        term.write_line(&format!("{head}{}", style(detail).dim()))?;
    }
    let scroll = if items.len() > visible {
        format!("{}-{} of {} · ", top + 1, top + visible, items.len())
    } else {
        String::new()
    };
    term.write_line(
        &style(format!(
            "{scroll}space toggle · a all · i invert · enter confirm · esc cancel"
        ))
        .dim()
        .to_string(),
    )?;
    Ok(())
}

pub fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

/// True when an interactive prompt can run: both ends are a terminal and the
/// run was not asked for machine output.
pub fn is_tty() -> bool {
    use std::io::IsTerminal;
    !crate::json_mode() && io::stdout().is_terminal() && io::stdin().is_terminal()
}
