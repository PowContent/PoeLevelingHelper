use serde::{Deserialize, Serialize};
use log::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneGroup {
    pub part: String,
    pub act: String,
    pub default: String,
    pub list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppData {
    pub p1acts: Vec<String>,
    pub p2acts: Vec<String>,
    pub parts: Vec<String>,
    pub zones: Vec<ZoneGroup>,
}

#[derive(Debug, Clone)]
pub struct ZoneManager {
    pub data: Option<AppData>,
    pub highest_act: u32,
    pub current_part: String,
    pub current_act: String,
    pub current_zone: String,
}

impl ZoneManager {
    pub fn new() -> Self {
        Self {
            data: None,
            highest_act: 1,
            current_part: "Part 1".to_string(),
            current_act: "Act 1".to_string(),
            current_zone: "01 The Twilight Strand".to_string(),
        }
    }

    pub fn load_data_from_str(&mut self, json_str: &str) {
        match serde_json::from_str::<AppData>(json_str) {
            Ok(data) => {
                info!("Loaded data.json successfully with {} zones.", data.zones.len());
                self.data = Some(data);
            }
            Err(e) => error!("Failed to parse bundled data.json: {}", e),
        }
    }

    fn extract_act_number(act_str: &str) -> u32 {
        act_str.replace("Act ", "").trim().parse::<u32>().unwrap_or(1)
    }

    pub fn transition_to_zone(&mut self, zone_name: &str) {
        log::debug!("Attempting to transition to zone: '{}'", zone_name);
        if let Some(data) = &self.data {
            let zone_name_lower = zone_name.to_lowercase();
            
            // Intelligent zone selection using highest_act
            let mut ordered_groups: Vec<&ZoneGroup> = Vec::new();

            let is_part2 = self.highest_act >= 6;
            
            // 1. Groups matching the current Part (Act 1-5 vs 6-10)
            ordered_groups.extend(data.zones.iter().filter(|g| {
                let act_num = Self::extract_act_number(&g.act);
                if is_part2 { act_num >= 6 } else { act_num < 6 }
            }));
            
            // 2. The remaining groups as fallback
            ordered_groups.extend(data.zones.iter().filter(|g| {
                let act_num = Self::extract_act_number(&g.act);
                if is_part2 { act_num < 6 } else { act_num >= 6 }
            }));

            for group in ordered_groups {
                for zone in &group.list {
                    // Strip leading digits and space from JSON zone name
                    let clean_json_zone = zone.trim_start_matches(|c: char| c.is_ascii_digit() || c.is_whitespace()).to_lowercase();
                    
                    // Check if either contains the other
                    if clean_json_zone.contains(&zone_name_lower) || zone_name_lower.contains(&clean_json_zone) {
                        log::info!("Matched zone! Changing from '{}' to '{}' (Act: {})", self.current_zone, zone, group.act);
                        self.current_part = group.part.clone();
                        self.current_act = group.act.clone();
                        self.current_zone = zone.clone();
                        
                        let act_num = Self::extract_act_number(&self.current_act);
                        if act_num > self.highest_act {
                            self.highest_act = act_num;
                        }
                        
                        return;
                    }
                }
            }
            log::warn!("Could not find a match in data.json for zone: '{}'", zone_name);
        }
    }
}
