//! User themes: drop a YAML file into `~/.aster/themes/` (or the repo's
//! `.aster/themes/`) and `/theme` picks it up. Missing fields fall back to the
//! `base` builtin, so a one-line accent override is a valid theme.

use std::path::{Path, PathBuf};

use ratatui::style::Color;

use super::theme::Theme;

/// Hard cap so a directory of theme files cannot balloon the picker.
const MAX_USER_THEMES: usize = 64;

/// Every user theme, project files shadowing global ones of the same name.
pub(crate) fn discover() -> Vec<super::theme::ThemeEntry> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".aster/themes"));
    }
    if let Ok(home) = crate::persist::home() {
        roots.push(home.join("themes"));
    }
    let mut seen = Vec::new();
    for root in roots {
        for entry in load_dir(&root) {
            if !seen
                .iter()
                .any(|t: &super::theme::ThemeEntry| t.name == entry.name)
            {
                seen.push(entry);
            }
        }
    }
    seen.sort_by(|a, b| a.name.cmp(&b.name));
    seen.truncate(MAX_USER_THEMES);
    seen
}

fn load_dir(dir: &Path) -> Vec<super::theme::ThemeEntry> {
    let Ok(files) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for file in files.flatten() {
        let path = file.path();
        if path.extension().is_none_or(|e| e == "yaml" || e == "yml") {
            continue;
        }
        if let Some(entry) = load_file(&path) {
            entries.push(entry);
        }
    }
    entries
}

fn load_file(path: &Path) -> Option<super::theme::ThemeEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let raw: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
    let map = raw.as_mapping()?;
    let stem = path
        .file_stem()?
        .to_string_lossy()
        .to_lowercase()
        .replace(' ', "-");
    let name = match map.get(serde_yaml::Value::String("name".into())) {
        Some(v) => v.as_str()?.to_string(),
        None => stem,
    };
    if name.is_empty() {
        return None;
    }
    let base = match map.get(serde_yaml::Value::String("base".into())) {
        Some(v) => v.as_str().and_then(base_palette).unwrap_or(Theme::DEFAULT),
        None => Theme::DEFAULT,
    };
    let color = |key: &str| -> Option<Color> {
        map.get(serde_yaml::Value::String(key.into()))
            .and_then(|v| v.as_str())
            .and_then(parse_color)
    };
    let description = map
        .get(serde_yaml::Value::String("description".into()))
        .and_then(|v| v.as_str())
        .unwrap_or("custom theme")
        .to_string();
    let mut theme = base;
    for (key, field) in COLOR_FIELDS {
        if let Some(c) = color(key) {
            *field(&mut theme) = c;
        }
    }
    if let Some(stops) = map
        .get(serde_yaml::Value::String("mark".into()))
        .and_then(|v| v.as_sequence())
    {
        for (i, stop) in stops.iter().take(10).enumerate() {
            if let Some(c) = stop.as_str().and_then(parse_color) {
                theme.mark[i] = c;
            }
        }
    }
    Some(super::theme::ThemeEntry {
        name,
        description,
        theme,
    })
}

/// `base:` names a builtin whose palette fills every field the file leaves out.
fn base_palette(name: &str) -> Option<Theme> {
    match name {
        "dark" => Some(Theme::DEFAULT),
        "light" => Some(Theme::LIGHT),
        "midnight" => Some(Theme::MIDNIGHT),
        "forest" => Some(Theme::FOREST),
        "dracula" => Some(super::palettes::DRACULA),
        "catppuccin" => Some(super::palettes::CATPPUCCIN),
        "nord" => Some(super::palettes::NORD),
        "gruvbox" => Some(super::palettes::GRUVBOX),
        "solarized" => Some(super::palettes::SOLARIZED),
        "synthwave" => Some(super::palettes::SYNTHWAVE),
        "github-dark" => Some(super::palettes::GITHUB_DARK),
        "monokai" => Some(super::palettes::MONOKAI_PRO),
        "one-dark" => Some(super::palettes::ONE_DARK),
        "aster-ocean" => Some(super::palettes::ASTER_OCEAN),
        "aster-ember" => Some(super::palettes::ASTER_EMBER),
        "aster-orchid" => Some(super::palettes::ASTER_ORCHID),
        _ => None,
    }
}

fn parse_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// One themeable field: its YAML key and a setter into the palette. `mark`
/// is handled separately since it is a list.
type ColorField = (&'static str, fn(&mut Theme) -> &mut Color);

const COLOR_FIELDS: [ColorField; 30] = [
    ("text", |t| &mut t.text),
    ("dim", |t| &mut t.dim),
    ("dimmer", |t| &mut t.dimmer),
    ("faint", |t| &mut t.faint),
    ("accent", |t| &mut t.accent),
    ("error", |t| &mut t.error),
    ("rail_bg", |t| &mut t.rail_bg),
    ("pane_bg", |t| &mut t.pane_bg),
    ("sel_bg", |t| &mut t.sel_bg),
    ("amber", |t| &mut t.amber),
    ("blue", |t| &mut t.blue),
    ("purple", |t| &mut t.purple),
    ("add_bg", |t| &mut t.add_bg),
    ("add_fg", |t| &mut t.add_fg),
    ("add_mark", |t| &mut t.add_mark),
    ("del_bg", |t| &mut t.del_bg),
    ("del_fg", |t| &mut t.del_fg),
    ("del_mark", |t| &mut t.del_mark),
    ("inline_code_bg", |t| &mut t.inline_code_bg),
    ("inline_code_fg", |t| &mut t.inline_code_fg),
    ("heading_fg", |t| &mut t.heading_fg),
    ("link_fg", |t| &mut t.link_fg),
    ("placeholder", |t| &mut t.placeholder),
    ("success", |t| &mut t.success),
    ("warning", |t| &mut t.warning),
    ("severity_critical", |t| &mut t.severity_critical),
    ("severity_high", |t| &mut t.severity_high),
    ("severity_medium", |t| &mut t.severity_medium),
    ("severity_low", |t| &mut t.severity_low),
    ("severity_info", |t| &mut t.severity_info),
];
