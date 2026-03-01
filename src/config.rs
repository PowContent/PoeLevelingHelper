use ini::Ini;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::io;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl WindowRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub client_txt_path: String,
    pub overlay_opacity: f32,
    pub show_guide: bool,
    pub show_notes: bool,
    pub show_images: bool,
    pub note_type: String,
    pub main_hidden: bool,
    pub image_width: f32,
    pub image_spacing: f32,
    pub hide_when_unfocused: bool,
    pub text_max_width: f32,
    // Crash recovery state
    pub last_zone: String,
    pub last_act: String,
    pub last_player_level: u32,
    pub last_monster_level: u32,
    pub win_main: Option<WindowRect>,
    pub win_settings: Option<WindowRect>,
    pub win_guide: Option<WindowRect>,
    pub win_notes: Option<WindowRect>,
    pub win_images_bar: Option<WindowRect>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            client_txt_path: r#"C:\Program Files (x86)\Grinding Gear Games\Path of Exile\logs\Client.txt"#.to_string(),
            overlay_opacity: 0.8,
            show_guide: true,
            show_notes: true,
            show_images: true,
            note_type: "Default".to_string(),
            main_hidden: false,
            image_width: 200.0,
            image_spacing: 2.0,
            hide_when_unfocused: true,
            text_max_width: 400.0,
            last_zone: String::new(),
            last_act: String::new(),
            last_player_level: 1,
            last_monster_level: 1,
            win_main: None,
            win_settings: None,
            win_guide: None,
            win_notes: None,
            win_images_bar: None,
        }
    }
}

impl AppConfig {
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        let mut config = Self::default();
        if let Ok(ini) = Ini::load_from_file(path) {
            if let Some(section) = ini.section(Some("Settings")) {
                if let Some(path) = section.get("ClientTxtPath") {
                    config.client_txt_path = path.to_string();
                }
                if let Some(opacity) = section.get("OverlayOpacity") {
                    if let Ok(val) = opacity.parse() {
                        config.overlay_opacity = val;
                    }
                }
                if let Some(show) = section.get("ShowGuide") {
                    config.show_guide = show == "true";
                }
                if let Some(show) = section.get("ShowNotes") {
                    config.show_notes = show == "true";
                }
                if let Some(show) = section.get("ShowImages") {
                    config.show_images = show == "true";
                }
                if let Some(note_type) = section.get("NoteType") {
                    config.note_type = note_type.to_string();
                }
                if let Some(hidden) = section.get("MainHidden") {
                    config.main_hidden = hidden == "true";
                }
                if let Some(width) = section.get("ImageWidth") {
                    if let Ok(w) = width.parse::<f32>() {
                        config.image_width = w;
                    }
                }
                if let Some(spacing) = section.get("ImageSpacing") {
                    if let Ok(s) = spacing.parse::<f32>() {
                        config.image_spacing = s;
                    }
                }
                if let Some(hide) = section.get("HideWhenUnfocused") {
                    config.hide_when_unfocused = hide == "true";
                }
                if let Some(w) = section.get("TextMaxWidth") {
                    if let Ok(v) = w.parse::<f32>() {
                        config.text_max_width = v;
                    }
                }
            }
            if let Some(section) = ini.section(Some("State")) {
                if let Some(v) = section.get("LastZone") {
                    config.last_zone = v.to_string();
                }
                if let Some(v) = section.get("LastAct") {
                    config.last_act = v.to_string();
                }
                if let Some(v) = section.get("LastPlayerLevel") {
                    if let Ok(lvl) = v.parse() { config.last_player_level = lvl; }
                }
                if let Some(v) = section.get("LastMonsterLevel") {
                    if let Ok(lvl) = v.parse() { config.last_monster_level = lvl; }
                }
            }
            // Load window positions
            for (key, field) in [
                ("WinMain", &mut config.win_main as &mut Option<WindowRect>),
                ("WinSettings", &mut config.win_settings),
                ("WinGuide", &mut config.win_guide),
                ("WinNotes", &mut config.win_notes),
                ("WinImagesBar", &mut config.win_images_bar),
            ] {
                if let Some(section) = ini.section(Some(key)) {
                    if let (Some(x), Some(y), Some(w), Some(h)) = (
                        section.get("X").and_then(|v| v.parse::<f32>().ok()),
                        section.get("Y").and_then(|v| v.parse::<f32>().ok()),
                        section.get("W").and_then(|v| v.parse::<f32>().ok()),
                        section.get("H").and_then(|v| v.parse::<f32>().ok()),
                    ) {
                        *field = Some(WindowRect::new(x, y, w, h));
                    }
                }
            }
        }
        config
    }

    pub fn save(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let mut ini = Ini::new();
        ini.with_section(Some("Settings"))
            .set("ClientTxtPath", &self.client_txt_path)
            .set("OverlayOpacity", self.overlay_opacity.to_string())
            .set("ShowGuide", if self.show_guide { "true" } else { "false" })
            .set("ShowNotes", if self.show_notes { "true" } else { "false" })
            .set("ShowImages", if self.show_images { "true" } else { "false" })
            .set("NoteType", &self.note_type)
            .set("MainHidden", if self.main_hidden { "true" } else { "false" })
            .set("ImageWidth", self.image_width.to_string())
            .set("ImageSpacing", self.image_spacing.to_string())
            .set("HideWhenUnfocused", if self.hide_when_unfocused { "true" } else { "false" })
            .set("TextMaxWidth", self.text_max_width.to_string());
        ini.with_section(Some("State"))
            .set("LastZone", &self.last_zone)
            .set("LastAct", &self.last_act)
            .set("LastPlayerLevel", self.last_player_level.to_string())
            .set("LastMonsterLevel", self.last_monster_level.to_string());
        for (key, rect) in [
            ("WinMain", &self.win_main),
            ("WinSettings", &self.win_settings),
            ("WinGuide", &self.win_guide),
            ("WinNotes", &self.win_notes),
            ("WinImagesBar", &self.win_images_bar),
        ] {
            if let Some(r) = rect {
                ini.with_section(Some(key))
                    .set("X", r.x.to_string())
                    .set("Y", r.y.to_string())
                    .set("W", r.w.to_string())
                    .set("H", r.h.to_string());
            }
        }
        ini.write_to_file(path)
    }
}
