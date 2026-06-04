use super::*;

pub(super) fn prefs_path() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("Genesis").join("prefs.json");
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(home)
            .join(".config")
            .join("genesis")
            .join("prefs.json");
    }
    PathBuf::from(".genesis-prefs.json")
}

pub(super) fn load_gui_prefs() -> GuiPrefs {
    let Ok(text) = fs::read_to_string(prefs_path()) else {
        return GuiPrefs::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return GuiPrefs::default();
    };
    let browser_mode = match value.get("browser_mode").and_then(Value::as_str) {
        Some("groups") => BrowserMode::Groups,
        _ => BrowserMode::Folders,
    };
    GuiPrefs {
        browser_mode,
        show_browser_prefixes: value
            .get("show_browser_prefixes")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        expert_mode: value
            .get("expert_mode")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dark_mode: value
            .get("dark_mode")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        blender_path: value
            .get("blender_path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from),
    }
}

pub(super) fn save_gui_prefs(
    prefs: &GuiPrefs,
    terminal_open_games: &HashSet<String>,
) -> Result<(), String> {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create preferences folder: {error}"))?;
    }
    let mut games: Vec<&String> = terminal_open_games.iter().collect();
    games.sort();
    let value = json!({
        "browser_mode": match prefs.browser_mode {
            BrowserMode::Folders => "folders",
            BrowserMode::Groups => "groups",
        },
        "show_browser_prefixes": prefs.show_browser_prefixes,
        "expert_mode": prefs.expert_mode,
        "dark_mode": prefs.dark_mode,
        "blender_path": prefs.blender_path.as_ref().map(|path| path.display().to_string()),
        "terminal_open_games": games,
    });
    let text = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("Could not encode preferences: {error}"))?;
    fs::write(path, text).map_err(|error| format!("Could not save preferences: {error}"))
}

/// Load the set of game identifiers for which the terminal should auto-open.
/// Reads the same prefs.json as `load_gui_prefs`.
pub(super) fn load_terminal_open_games() -> HashSet<String> {
    let Ok(text) = fs::read_to_string(prefs_path()) else {
        return HashSet::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return HashSet::new();
    };
    value
        .get("terminal_open_games")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
