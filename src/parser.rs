use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use log::{info, error, debug};

pub struct LogParser {
    reader: Option<BufReader<File>>,
    last_pos: u64,
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogEvent {
    LevelUp { character: String, level: u32 },
    ZoneEntered { zone_name: String },
    MonsterLevel { level: u32 },
}

impl LogParser {
    pub fn new(path: PathBuf) -> Self {
        let mut parser = Self {
            reader: None,
            last_pos: 0,
            path: path.clone(),
        };
        parser.open(&path);
        parser
    }

    pub fn set_path(&mut self, path: PathBuf) {
        if self.path != path {
            self.path = path.clone();
            self.open(&path);
        }
    }

    fn open(&mut self, path: &Path) {
        match File::open(path) {
            Ok(mut file) => {
                // Seek to end so we don't process old logs
                if let Ok(pos) = file.seek(SeekFrom::End(0)) {
                    self.last_pos = pos;
                }
                self.reader = Some(BufReader::new(file));
                info!("Opened log file successfully: {:?}", path);
            }
            Err(e) => {
                debug!("Failed to open log file (might not exist yet): {}", e);
                self.reader = None;
            }
        }
    }

    pub fn poll_events(&mut self) -> Vec<LogEvent> {
        // Try opening file if it wasn't open
        if self.reader.is_none() {
            self.open(&self.path.clone());
        }

        let mut events = Vec::new();
        if let Some(reader) = &mut self.reader {
            let mut line = String::new();
            loop {
                let current_pos = reader.stream_position().unwrap_or(self.last_pos);
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        // EOF
                        break;
                    }
                    Ok(_) => {
                        self.last_pos = reader.stream_position().unwrap_or(current_pos + line.len() as u64);
                        if let Some(event) = Self::parse_line(&line) {
                            events.push(event);
                        }
                        line.clear();
                    }
                    Err(e) => {
                        error!("Error reading log: {}", e);
                        // Try to recover by clearing reader
                        self.reader = None;
                        break;
                    }
                }
            }
        }
        events
    }

    fn parse_line(line: &str) -> Option<LogEvent> {
        if let Some(idx) = line.find("is now level") {
            let parts: Vec<&str> = line[..idx].trim().rsplitn(2, ' ').collect();
            let char_name = if !parts.is_empty() { parts[0].trim_end_matches(':') } else { "Unknown" };
            
            let level_str = line[idx + "is now level".len()..].trim().trim_end_matches('.');
            if let Ok(level) = level_str.parse::<u32>() {
                log::debug!("Parsed level up -> Character: {}, Level: {}", char_name, level);
                return Some(LogEvent::LevelUp {
                    character: char_name.to_string(),
                    level,
                });
            }
        }

        if let Some(idx) = line.find("Generating level") {
            let level_str = line[idx + "Generating level ".len()..].trim();
            let level_str = level_str.split(':').next().unwrap_or("").trim();
            if let Ok(level) = level_str.parse::<u32>() {
                log::debug!("Parsed monster level -> Level: {}", level);
                return Some(LogEvent::MonsterLevel { level });
            }
        }

        if let Some(idx) = line.find("You have entered") {
            let zone_name = line[idx + "You have entered ".len()..].trim().trim_end_matches('.');
            log::debug!("Parsed zone entered -> Zone: {}", zone_name);
            return Some(LogEvent::ZoneEntered {
                zone_name: zone_name.to_string(),
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_parsing() {
        let level_event = LogParser::parse_line("2023/10/25 10:00:00 123456 [INFO Client 12345] : CharacterName is now level 2");
        assert_eq!(level_event, Some(LogEvent::LevelUp { character: "CharacterName".to_string(), level: 2 }));

        let zone_event = LogParser::parse_line("2023/10/25 10:00:00 123456 [INFO Client 12345] : You have entered Lioneye's Watch.");
        assert_eq!(zone_event, Some(LogEvent::ZoneEntered { zone_name: "Lioneye's Watch".to_string() }));

        let monster_event = LogParser::parse_line("2023/10/25 10:00:00 123456 [INFO Client 12345] : Generating level 2: The Coast");
        assert_eq!(monster_event, Some(LogEvent::MonsterLevel { level: 2 }));
    }
}
