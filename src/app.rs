use eframe::egui;
use std::path::PathBuf;
use crate::config::{AppConfig, WindowRect};
use crate::parser::{LogParser, LogEvent};
use crate::zone::ZoneManager;
use crate::exp::{detailed_exp_status, ExpStatus};

/// Get the "Custom Notes" directory path next to the executable.
fn custom_notes_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Custom Notes")
}

/// List custom note type folder names found in "Custom Notes/" next to the exe.
fn list_custom_note_types() -> Vec<String> {
    let dir = custom_notes_dir();
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut types = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    types.push(name.to_string());
                }
            }
        }
    }
    types.sort();
    types
}

/// Create a custom notes template by copying the embedded Default notes to disk.
fn create_custom_notes_template(name: &str) -> std::io::Result<()> {
    let base = custom_notes_dir().join(name);
    // Iterate all embedded files under Default/
    for path in crate::assets::Builds::iter() {
        let path_str = path.as_ref();
        if let Some(rest) = path_str.strip_prefix("Default/") {
            let dest = base.join(rest);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if let Some(file) = crate::assets::Builds::get(path_str) {
                std::fs::write(&dest, file.data.as_ref())?;
            }
        }
    }
    Ok(())
}

/// Try to read a note file from Custom Notes on disk. Returns None if not found.
fn read_custom_note(note_type: &str, act: &str, filename: &str) -> Option<String> {
    let path = custom_notes_dir().join(note_type).join(act).join(filename);
    std::fs::read_to_string(&path).ok()
}


#[cfg(windows)]
fn apply_transparency_to_all_windows() {
    use winapi::um::dwmapi::{DwmEnableBlurBehindWindow, DWM_BLURBEHIND};
    use winapi::um::processthreadsapi::GetCurrentThreadId;
    use winapi::um::wingdi::CreateRectRgn;
    use winapi::um::winuser::EnumThreadWindows;
    use winapi::shared::windef::HWND;
    use winapi::shared::minwindef::{BOOL, LPARAM, TRUE};

    unsafe extern "system" fn enum_callback(hwnd: HWND, _: LPARAM) -> BOOL {
        unsafe {
            let region = CreateRectRgn(0, 0, -1, -1);
            let bb = DWM_BLURBEHIND {
                dwFlags: 0x1, // DWM_BB_ENABLE
                fEnable: 1,
                hRgnBlur: region,
                fTransitionOnMaximized: 0,
            };
            DwmEnableBlurBehindWindow(hwnd, &bb);
        }
        TRUE
    }

    unsafe {
        let thread_id = GetCurrentThreadId();
        EnumThreadWindows(thread_id, Some(enum_callback), 0);
    }
}

/// Compute whether cursor is over any panel rect.
#[cfg(windows)]
fn check_cursor_over_panels(ctx: &egui::Context, panel_rects: &[egui::Rect]) -> bool {
    use winapi::um::winuser::GetCursorPos;
    use winapi::shared::windef::POINT;

    let ppp = ctx.pixels_per_point();
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let cursor_x = pt.x as f32 / ppp;
        let cursor_y = pt.y as f32 / ppp;
        let cursor_pos = egui::pos2(cursor_x, cursor_y);
        panel_rects.iter().any(|r| r.contains(cursor_pos))
    }
}

/// Schedule a WS_EX_TRANSPARENT style change from a background thread.
/// SetWindowLongPtrW triggers synchronous WM_SIZE messages. If called from
/// within the winit event loop (even via SetTimer), this re-enters the GL
/// paint loop and crashes at glow_integration.rs:909.
///
/// By spawning a short-lived thread that sleeps briefly, we guarantee the
/// style change happens completely outside the rendering pipeline. The sleep
/// ensures the current paint frame completes before the style is modified.

/// Check if Path of Exile (or this overlay) is the foreground window.
#[cfg(windows)]
fn is_poe_or_self_focused() -> bool {
    use winapi::um::winuser::{GetForegroundWindow, GetWindowTextW};
    use winapi::um::processthreadsapi::GetCurrentThreadId;
    use winapi::um::winuser::GetWindowThreadProcessId;

    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        // Check if it's our own window
        let fg_thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        let our_thread = GetCurrentThreadId();
        if fg_thread == our_thread {
            return true;
        }
        // Check window title for PoE
        let mut title = [0u16; 256];
        let len = GetWindowTextW(fg, title.as_mut_ptr(), 256);
        if len > 0 {
            let title_str = String::from_utf16_lossy(&title[..len as usize]);
            title_str.contains("Path of Exile")
        } else {
            false
        }
    }
}

pub struct PoELevelingGuideApp {
    config: AppConfig,
    zone_manager: ZoneManager,
    player_level: u32,
    monster_level: u32,
    zone_detected: bool,
    level_source: &'static str,
    show_settings: bool,
    parser: LogParser,
    #[cfg(windows)]
    last_over_panel: Option<bool>,
    #[cfg(windows)]
    transparency_applied: bool,
    reset_positions_pending: bool,
    ocr_worker: crate::ocr::OcrWorker,
}

fn overlay_frame(opacity: f32, bg_color: [u8; 3]) -> egui::Frame {
    let alpha = (opacity * 255.0) as u8;
    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(bg_color[0], bg_color[1], bg_color[2], alpha))
        .corner_radius(4.0)
        .inner_margin(8.0)
}

/// Render text with color codes. Supports two prefix formats:
/// - Letter-comma: "R,", "G,", "B,", "Y,", "W,"
/// - Symbol-space: "< " (red), "+ " (green), "> " (blue), "- " (yellow)
/// Lines without a recognized prefix render in default white.
fn render_colored_text(ui: &mut egui::Ui, text: &str, font_size: f32) {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }
        if trimmed.len() >= 2 {
            let bytes = trimmed.as_bytes();
            let (color, skip) = match (bytes[0], bytes[1]) {
                // Letter-comma format: R, G, B, Y, W,
                (b'R', b',') => (Some(egui::Color32::from_rgb(255, 80, 80)), 2),
                (b'G', b',') => (Some(egui::Color32::from_rgb(80, 255, 80)), 2),
                (b'B', b',') => (Some(egui::Color32::from_rgb(100, 150, 255)), 2),
                (b'Y', b',') => (Some(egui::Color32::from_rgb(255, 255, 80)), 2),
                (b'W', b',') => (None, 2),
                // Symbol-space format: < + > -
                (b'<', b' ') => (Some(egui::Color32::from_rgb(255, 80, 80)), 2),
                (b'+', b' ') => (Some(egui::Color32::from_rgb(80, 255, 80)), 2),
                (b'>', b' ') => (Some(egui::Color32::from_rgb(100, 150, 255)), 2),
                (b'-', b' ') => (Some(egui::Color32::from_rgb(255, 255, 80)), 2),
                _ => (None, 0),
            };
            if skip > 0 {
                let content = trimmed[skip..].trim();
                let rt = egui::RichText::new(content).size(font_size);
                if let Some(c) = color {
                    ui.label(rt.color(c));
                } else {
                    ui.label(rt);
                }
                continue;
            }
        }
        ui.label(egui::RichText::new(trimmed).size(font_size));
    }
}

/// Load note text: try custom notes on disk first, then fall back to embedded assets.
fn load_note_text(note_type: &str, act: &str, filename: &str) -> Option<String> {
    // Try custom notes on disk first
    if let Some(text) = read_custom_note(note_type, act, filename) {
        return Some(text);
    }
    // Fall back to embedded assets
    let path = format!("{}/{}/{}", note_type, act, filename);
    if let Some(file) = crate::assets::Builds::get(&path) {
        if let Ok(text) = std::str::from_utf8(file.data.as_ref()) {
            return Some(text.to_string());
        }
    }
    None
}

fn extract_zone_text(full_text: &str, zone: &str) -> Option<String> {
    let clean_zone = zone.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace());
    let search_tag = format!("zone:{}", clean_zone);
    if !full_text.contains("zone:") {
        return Some(full_text.to_string());
    }
    let mut result = String::new();
    let mut reading = false;
    for line in full_text.lines() {
        let trimmed = line.trim();
        if trimmed == search_tag {
            reading = true;
        } else if trimmed.starts_with("zone:") {
            reading = false;
        } else if reading {
            result.push_str(line);
            result.push('\n');
        }
    }
    if result.trim().is_empty() { None } else { Some(result) }
}

impl PoELevelingGuideApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load_or_default("config.ini");
        let mut zone_manager = ZoneManager::new();
        if let Some(file) = crate::assets::LibData::get("data.json") {
            if let Ok(json_str) = std::str::from_utf8(file.data.as_ref()) {
                zone_manager.load_data_from_str(json_str);
            }
        }

        // Restore crash recovery state
        let player_level = if config.last_player_level > 0 { config.last_player_level } else { 1 };
        // Extract area level from saved zone name prefix (e.g. "13 Southern Forest" → 13)
        let monster_level = if config.last_monster_level > 0 {
            config.last_monster_level
        } else {
            let lvl_str: String = config.last_zone.chars().take_while(|c| c.is_ascii_digit()).collect();
            lvl_str.parse::<u32>().unwrap_or(1)
        };
        if !config.last_act.is_empty() {
            zone_manager.current_act = config.last_act.clone();
        }
        if !config.last_zone.is_empty() {
            zone_manager.current_zone = config.last_zone.clone();
        }

        let parser = LogParser::new(PathBuf::from(&config.client_txt_path));
        let ocr_worker = crate::ocr::OcrWorker::spawn();

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::TRANSPARENT;
        visuals.window_fill = egui::Color32::TRANSPARENT;
        visuals.extreme_bg_color = egui::Color32::TRANSPARENT;
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.window_stroke = egui::Stroke::NONE;
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|style| {
            style.interaction.selectable_labels = false;
        });

        Self {
            config,
            zone_manager,
            player_level,
            monster_level,
            zone_detected: true,
            level_source: "config",
            show_settings: false,
            parser,
            #[cfg(windows)]
            last_over_panel: None,
            #[cfg(windows)]
            transparency_applied: false,
            reset_positions_pending: false,
            ocr_worker,
        }
    }

    fn default_rect(name: &str) -> egui::Rect {
        match name {
            "main" => egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(600.0, 40.0)),
            "settings" => egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(300.0, 250.0)),
            "guide" => egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(300.0, 200.0)),
            "notes" => egui::Rect::from_min_size(egui::pos2(420.0, 100.0), egui::vec2(300.0, 200.0)),
            "images_bar" => egui::Rect::from_min_size(egui::pos2(100.0, 320.0), egui::vec2(400.0, 30.0)),
            _ => egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(200.0, 200.0)),
        }
    }

    fn win_rect(saved: &Option<WindowRect>, name: &str) -> egui::Rect {
        if let Some(r) = saved {
            egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.w, r.h))
        } else {
            Self::default_rect(name)
        }
    }

    fn save_rect(rect: egui::Rect) -> WindowRect {
        WindowRect::new(rect.min.x, rect.min.y, rect.width(), rect.height())
    }
}

impl eframe::App for PoELevelingGuideApp {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        let _ = self.config.save("config.ini");
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(windows)]
        if !self.transparency_applied {
            apply_transparency_to_all_windows();
            self.transparency_applied = true;
        }

        // Process log events — save state on any change
        let mut state_changed = false;
        for event in self.parser.poll_events() {
            match event {
                LogEvent::LevelUp { level, .. } => {
                    log::info!("[Level] Player leveled up to {} (source: Client.txt)", level);
                    self.player_level = level;
                    state_changed = true;
                }
                LogEvent::MonsterLevel { level } => {
                    log::info!("[Level] Monster level {} detected (source: Client.txt 'Generating level')", level);
                    self.monster_level = level;
                    self.level_source = "log";
                    state_changed = true;
                }
                LogEvent::ZoneEntered { zone_name } => {
                    let found = self.zone_manager.transition_to_zone(&zone_name);
                    self.zone_detected = found;

                    if found {
                        // Zone recognized — set level from campaign zone data
                        let level_str: String = self.zone_manager.current_zone
                            .chars().take_while(|c| c.is_ascii_digit()).collect();
                        if let Ok(lvl) = level_str.parse::<u32>() {
                            log::info!("[Level] Area level {} set from zone data for '{}' (source: campaign data)", lvl, zone_name);
                            self.monster_level = lvl;
                            self.level_source = "zone_data";
                        }
                    } else {
                        // Zone not in campaign list — keep current level, mark undetected
                        log::warn!("[Zone] '{}' not found in campaign data — zone not detected", zone_name);
                        self.zone_manager.current_zone = zone_name.clone();
                    }

                    // Always trigger OCR to read the actual area level from the game screen
                    log::info!("[OCR] Triggering screen capture for zone '{}'", zone_name);
                    self.ocr_worker.trigger();
                    state_changed = true;
                }
            }
        }

        // Poll OCR results from background thread
        if let Some(level) = self.ocr_worker.poll_result() {
            log::info!("[Level] Area level {} detected (source: OCR screen capture)", level);
            self.monster_level = level;
            self.level_source = "ocr";
            self.zone_detected = true; // OCR confirmed a level, so the zone is valid
            state_changed = true;
        }

        if state_changed {
            self.config.last_player_level = self.player_level;
            self.config.last_monster_level = self.monster_level;
            self.config.last_act = self.zone_manager.current_act.clone();
            self.config.last_zone = self.zone_manager.current_zone.clone();
            let _ = self.config.save("config.ini");
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        ctx.style_mut(|style| style.animation_time = 0.0);

        // Transparent background panel that covers the whole screen
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |_ui| {});

        // Check if PoE (or self) is focused — used to hide panels when unfocused
        #[cfg(windows)]
        let poe_focused = is_poe_or_self_focused();
        #[cfg(not(windows))]
        let poe_focused = true;

        let show_all_panels = !self.config.hide_when_unfocused || poe_focused;

        let opacity = self.config.overlay_opacity;
        let image_opacity = self.config.image_opacity;
        let font_size = self.config.font_size;
        let frame = overlay_frame(opacity, self.config.bg_color);
        let mut input_rects: Vec<egui::Rect> = Vec::new();

        // === Main hotbar window ===
        let main_rect = Self::win_rect(&self.config.win_main, "main");
        let mut main_open = true;
        egui::Window::new("PoE Leveling Guide")
            .id(egui::Id::new("main_window"))
            .frame(frame)
            .title_bar(false)
            .default_rect(main_rect)
            .resizable(false)
            .open(&mut main_open)
            .show(ctx, |ui| {
                if !self.config.main_hidden {
                    ui.horizontal(|ui| {
                        ui.label("Level:");
                        if ui.add(egui::DragValue::new(&mut self.player_level).range(1..=100)).changed() {
                            log::info!("[Level] Player level manually set to {}", self.player_level);
                            self.config.last_player_level = self.player_level;
                            let _ = self.config.save("config.ini");
                        }

                        // XP penalty display based on zone detection
                        if !self.zone_detected {
                            ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "Zone not detected");
                        } else {
                            let status = detailed_exp_status(self.player_level, self.monster_level);
                            match status {
                                ExpStatus::UnderLeveled { levels_under, penalty_pct, max_safe_zone } => {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(100, 150, 255),
                                        format!("A{} | {} under | {:.1}% penalty | Lvl At: {}", self.monster_level, levels_under, penalty_pct, max_safe_zone),
                                    );
                                }
                                ExpStatus::NoPenalty { levels_over_min } => {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(80, 255, 80),
                                        format!("A{} | No penalty | +{} over min", self.monster_level, levels_over_min),
                                    );
                                }
                                ExpStatus::OverLeveled { penalty_pct } => {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(255, 80, 80),
                                        format!("A{} | Over-leveled | {:.1}% penalty", self.monster_level, penalty_pct),
                                    );
                                }
                            }
                        }

                        ui.separator();
                        ui.label(&self.zone_manager.current_act);
                        ui.label(&self.zone_manager.current_zone);

                        let part_text = if self.zone_manager.highest_act >= 6 { "Part 2" } else { "Part 1" };
                        if ui.button(format!("🔁 {}", part_text)).on_hover_text("Toggle Part 1 / Part 2").clicked() {
                            if self.zone_manager.highest_act >= 6 {
                                self.zone_manager.highest_act = 1;
                            } else {
                                self.zone_manager.highest_act = 6;
                            }
                            // Re-match current zone against the new part's act data
                            let current = self.zone_manager.current_zone.clone();
                            let clean = current.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace()).to_string();
                            let found = self.zone_manager.transition_to_zone(&clean);
                            self.zone_detected = found;
                            if found {
                                let level_str: String = self.zone_manager.current_zone
                                    .chars().take_while(|c| c.is_ascii_digit()).collect();
                                if let Ok(lvl) = level_str.parse::<u32>() {
                                    self.monster_level = lvl;
                                    self.level_source = "zone_data";
                                }
                            }
                        }

                        ui.separator();
                        if ui.button("⚙").clicked() {
                            self.show_settings = !self.show_settings;
                        }
                        if ui.button("➖").on_hover_text("Hide Hotbar").clicked() {
                            self.config.main_hidden = true;
                        }
                        if ui.button("X").on_hover_text("Close Application").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                } else if ui.button("➕").clicked() {
                    self.config.main_hidden = false;
                }
            });
        // Track main window position
        let main_pos = ctx.memory(|mem| {
            mem.area_rect(egui::Id::new("main_window"))
        });
        if let Some(rect) = main_pos {
            self.config.win_main = Some(Self::save_rect(rect));
            input_rects.push(rect);
        }

        // Handle reset positions: clear egui's stored area positions, set new defaults below main window
        if self.reset_positions_pending {
            self.reset_positions_pending = false;
            let base = main_pos.map(|r| r.min).unwrap_or(egui::pos2(100.0, 50.0));
            let below = egui::pos2(base.x, base.y + 50.0);

            // Clear all stored egui area positions so default_rect/default_pos take effect
            ctx.memory_mut(|mem| mem.reset_areas());

            // Set config positions relative to main window
            self.config.win_main = main_pos.map(|r| Self::save_rect(r));
            self.config.win_guide = Some(WindowRect::new(below.x, below.y, 300.0, 200.0));
            self.config.win_notes = Some(WindowRect::new(below.x + 320.0, below.y, 300.0, 200.0));
            self.config.win_images_bar = Some(WindowRect::new(below.x, below.y + 220.0, 400.0, 30.0));
            let _ = self.config.save("config.ini");
        }

        // === Settings window (anchored below main window) ===
        if self.show_settings && show_all_panels {
            let settings_pos = main_pos
                .map(|r| egui::pos2(r.min.x, r.max.y + 2.0))
                .unwrap_or(egui::pos2(100.0, 100.0));
            let mut settings_open = self.show_settings;
            egui::Window::new("Settings")
                .id(egui::Id::new("settings_window"))
                .frame(frame)
                .title_bar(false)
                .fixed_pos(settings_pos)
                .auto_sized()
                .resizable(false)
                .open(&mut settings_open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Client.txt Path:");
                        ui.text_edit_singleline(&mut self.config.client_txt_path);
                        if ui.button("Browse").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Text files", &["txt"])
                                .set_file_name("Client.txt")
                                .pick_file()
                            {
                                self.config.client_txt_path = path.display().to_string();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Note Type:");
                        let custom_types = list_custom_note_types();
                        let total_items = 3 + custom_types.len();
                        let dropdown_height = (total_items as f32) * 24.0 + 16.0;
                        egui::ComboBox::from_id_salt("note_type_combo")
                            .selected_text(&self.config.note_type)
                            .height(dropdown_height)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.config.note_type, "Abbreviated".to_string(), "Abbreviated");
                                ui.selectable_value(&mut self.config.note_type, "Default".to_string(), "Default");
                                ui.selectable_value(&mut self.config.note_type, "Detailed".to_string(), "Detailed");
                                if !custom_types.is_empty() {
                                    ui.separator();
                                    for ct in &custom_types {
                                        ui.selectable_value(&mut self.config.note_type, ct.clone(), ct);
                                    }
                                }
                            });
                        if custom_types.contains(&self.config.note_type) {
                            if ui.button("Open Folder").on_hover_text("Open custom notes folder in file explorer").clicked() {
                                let path = custom_notes_dir().join(&self.config.note_type);
                                #[cfg(windows)]
                                { let _ = std::process::Command::new("explorer").arg(&path).spawn(); }
                                #[cfg(target_os = "macos")]
                                { let _ = std::process::Command::new("open").arg(&path).spawn(); }
                                #[cfg(target_os = "linux")]
                                { let _ = std::process::Command::new("xdg-open").arg(&path).spawn(); }
                            }
                        }
                        if ui.button("Create Custom").on_hover_text("Create a custom notes template from Default").clicked() {
                            // Find a unique name
                            let mut name = "My Notes".to_string();
                            let mut i = 2;
                            while custom_notes_dir().join(&name).exists() {
                                name = format!("My Notes {}", i);
                                i += 1;
                            }
                            if let Err(e) = create_custom_notes_template(&name) {
                                log::error!("Failed to create custom notes: {}", e);
                            } else {
                                self.config.note_type = name;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Text Max Width:");
                        ui.add(egui::Slider::new(&mut self.config.text_max_width, 100.0..=800.0).suffix("px"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Font Size:");
                        ui.add(egui::Slider::new(&mut self.config.font_size, 8.0..=32.0).suffix("px"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Image Max Width:");
                        ui.add(egui::Slider::new(&mut self.config.image_width, 50.0..=600.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Image Spacing:");
                        ui.add(egui::Slider::new(&mut self.config.image_spacing, 0.0..=10.0).suffix("px"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Image Opacity:");
                        ui.add(egui::Slider::new(&mut self.config.image_opacity, 0.0..=1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("BG Color:");
                        let mut color = egui::Color32::from_rgb(
                            self.config.bg_color[0],
                            self.config.bg_color[1],
                            self.config.bg_color[2],
                        );
                        if ui.color_edit_button_srgba(&mut color).changed() {
                            self.config.bg_color = [color.r(), color.g(), color.b()];
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("UI Opacity:");
                        ui.add(egui::Slider::new(&mut self.config.overlay_opacity, 0.0..=1.0));
                    });
                    ui.checkbox(&mut self.config.show_guide, "Show Guide");
                    ui.checkbox(&mut self.config.show_notes, "Show Notes");
                    ui.checkbox(&mut self.config.show_images, "Show Images");
                    ui.checkbox(&mut self.config.hide_when_unfocused, "Hide panels when PoE unfocused");

                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let _ = self.config.save("config.ini");
                            self.show_settings = false;
                        }
                        if ui.button("Reset Positions").clicked() {
                            self.reset_positions_pending = true;
                        }
                    });
                });
            if !settings_open {
                self.show_settings = false;
            }
            if let Some(rect) = ctx.memory(|mem| {
                mem.area_rect(egui::Id::new("settings_window"))
            }) {
                input_rects.push(rect);
            }
        }

        // === Guide window ===
        if self.config.show_guide && show_all_panels {
            let guide_rect = Self::win_rect(&self.config.win_guide, "guide");
            egui::Window::new("Guide")
                .id(egui::Id::new("guide_window"))
                .frame(frame)
                .title_bar(false)
                .default_pos(guide_rect.min)
                .auto_sized()
                .show(ctx, |ui| {
                    ui.set_max_width(self.config.text_max_width);
                    let text = load_note_text(&self.config.note_type, &self.zone_manager.current_act, "guide.txt");
                    match text {
                        Some(text) => {
                            match extract_zone_text(&text, &self.zone_manager.current_zone) {
                                Some(content) => { render_colored_text(ui, &content, font_size); }
                                None => {
                                    let clean = self.zone_manager.current_zone.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace());
                                    ui.label(format!("Add 'zone:{}' to this file", clean));
                                }
                            }
                        }
                        None => {
                            ui.label(format!("No guide text found for {} / {}", self.config.note_type, self.zone_manager.current_act));
                        }
                    }
                });
            if let Some(rect) = ctx.memory(|mem| {
                mem.area_rect(egui::Id::new("guide_window"))
            }) {
                self.config.win_guide = Some(Self::save_rect(rect));
                input_rects.push(rect);
            }
        }

        // === Notes window ===
        if self.config.show_notes && show_all_panels {
            let notes_rect = Self::win_rect(&self.config.win_notes, "notes");
            egui::Window::new("Notes")
                .id(egui::Id::new("notes_window"))
                .frame(frame)
                .title_bar(false)
                .default_pos(notes_rect.min)
                .auto_sized()
                .show(ctx, |ui| {
                    ui.set_max_width(self.config.text_max_width);
                    let text = load_note_text(&self.config.note_type, &self.zone_manager.current_act, "notes.txt");
                    match text {
                        Some(text) => {
                            match extract_zone_text(&text, &self.zone_manager.current_zone) {
                                Some(content) => { render_colored_text(ui, &content, font_size); }
                                None => {
                                    let clean = self.zone_manager.current_zone.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace());
                                    ui.label(format!("Add 'zone:{}' to this file", clean));
                                }
                            }
                        }
                        None => {
                            ui.label(format!("No notes text found for {} / {}", self.config.note_type, self.zone_manager.current_act));
                        }
                    }
                });
            if let Some(rect) = ctx.memory(|mem| {
                mem.area_rect(egui::Id::new("notes_window"))
            }) {
                self.config.win_notes = Some(Self::save_rect(rect));
                input_rects.push(rect);
            }
        }

        // === Images bar + image grid ===
        if self.config.show_images && show_all_panels {
            let images_bar_rect = Self::win_rect(&self.config.win_images_bar, "images_bar");
            egui::Window::new("Images")
                .id(egui::Id::new("images_bar_window"))
                .frame(frame)
                .title_bar(false)
                .default_rect(images_bar_rect)
                .resizable([true, false])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Images: {} - {}", self.zone_manager.current_act, self.zone_manager.current_zone));
                        // Visual hint that this bar is horizontally resizable
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("<->").monospace().color(egui::Color32::GRAY));
                        });
                    });
                });

            // Track bar position for image grid layout
            let bar_rect = ctx.memory(|mem| {
                mem.area_rect(egui::Id::new("images_bar_window"))
            }).unwrap_or(images_bar_rect);
            self.config.win_images_bar = Some(Self::save_rect(bar_rect));
            input_rects.push(bar_rect);

            // Render images directly as painter images (no egui::Window = no spacing/margins)
            let act = &self.zone_manager.current_act;
            let zone = &self.zone_manager.current_zone;
            let image_width = self.config.image_width;
            let clean_zone = zone.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace());

            // Collect active image seeds with their aspect-ratio-corrected heights
            struct ImageInfo {
                seed: u32,
                height: f32,
            }
            let mut active_images: Vec<ImageInfo> = Vec::new();
            for seed in 1..=5 {
                let image_path = format!("{}/{}_Seed_{}.jpg", act, clean_zone, seed);
                if let Some(file) = crate::assets::Images::get(&image_path) {
                    // Compute actual display height based on image aspect ratio
                    let height = image::ImageReader::new(std::io::Cursor::new(file.data.as_ref()))
                        .with_guessed_format()
                        .ok()
                        .and_then(|r| r.into_dimensions().ok())
                        .map(|(w, h)| if w > 0 { (h as f32 / w as f32) * image_width } else { image_width })
                        .unwrap_or(image_width);
                    active_images.push(ImageInfo { seed, height });
                }
            }

            let spacing = self.config.image_spacing;
            let cols = (bar_rect.width() / (image_width + spacing)).floor().max(1.0) as usize;

            // Calculate row heights (max image height in each row)
            let num_rows = (active_images.len() + cols - 1) / cols;
            let mut row_heights = vec![0.0_f32; num_rows];
            for (i, img) in active_images.iter().enumerate() {
                let row = i / cols;
                row_heights[row] = row_heights[row].max(img.height);
            }
            // Cumulative Y offsets for each row (with spacing between rows)
            let mut row_y_offsets = vec![0.0_f32; num_rows];
            for r in 1..num_rows {
                row_y_offsets[r] = row_y_offsets[r - 1] + row_heights[r - 1] + spacing;
            }

            for (i, img_info) in active_images.iter().enumerate() {
                let col = i % cols;
                let row = i / cols;
                let img_x = bar_rect.min.x + (col as f32) * (image_width + spacing);
                let img_y = bar_rect.max.y + row_y_offsets[row];

                let image_path = format!("{}/{}_Seed_{}.jpg", act, clean_zone, img_info.seed);
                if let Some(file) = crate::assets::Images::get(&image_path) {
                    egui::Area::new(egui::Id::new(format!("image_area_{}", img_info.seed)))
                        .fixed_pos([img_x, img_y])
                        .order(egui::Order::Background)
                        .interactable(false)
                        .show(ctx, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            let image = egui::Image::from_bytes(
                                format!("bytes://{}", image_path),
                                file.data.into_owned(),
                            )
                            .fit_to_exact_size(egui::vec2(image_width, img_info.height))
                            .maintain_aspect_ratio(true)
                            .tint(egui::Color32::from_white_alpha((image_opacity * 255.0) as u8));
                            ui.add(image);
                        });
                }
            }
        }

        // Update parser path if changed
        self.parser.set_path(PathBuf::from(&self.config.client_txt_path));

        // Set mouse passthrough if cursor is NOT over any panel
        #[cfg(windows)]
        {
            let popup_open = egui::Popup::is_any_open(ctx);
            let cursor_over_panel = popup_open || check_cursor_over_panels(ctx, &input_rects);
            if self.last_over_panel != Some(cursor_over_panel) {
                self.last_over_panel = Some(cursor_over_panel);
                // Use eframe's ViewportCommand to safely change passthrough state outside the paint loop.
                // If cursor is over a panel, passthrough should be FALSE.
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(!cursor_over_panel));
            }
        }
    }
}

impl Drop for PoELevelingGuideApp {
    fn drop(&mut self) {
        self.ocr_worker.shutdown();
    }
}
