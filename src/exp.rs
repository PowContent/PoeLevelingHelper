/// Get the effective monster level for XP penalty calculation.
/// Areas with monster level >= 70 use a reduced effective level.
/// Based on https://www.i-volve.net/jol/poe_xpdrop_en.php
fn get_effective_zone_level(zone_level: u32) -> f64 {
    if zone_level < 70 {
        return zone_level as f64;
    }
    match zone_level {
        70 => 70.0,
        71 => 70.94,
        72 => 71.82,
        73 => 72.64,
        74 => 73.40,
        75 => 74.10,
        76 => 74.74,
        77 => 75.32,
        78 => 75.84,
        79 => 76.30,
        80 => 76.70,
        81 => 77.04,
        82 => 77.32,
        83 => 77.54,
        84 => 77.70,
        // Fallback formula for levels beyond the table (added in 3.16)
        _ => {
            let z = zone_level as f64;
            -0.03 * z * z + 5.17 * z - 144.9
        }
    }
}

/// Calculate the XP penalty percentage and "over level" amount.
/// Returns (xp_penalty_percent, over_level).
/// xp_penalty_percent is 0.0 when no penalty, up to 99.0 max (1% XP minimum).
/// over_level is how many levels the player exceeds the zone's safe range.
pub fn calculate_exp_penalty(player_level: u32, zone_level: u32) -> (f64, i32) {
    let lvl = player_level as f64;
    let lvl_diff = get_effective_zone_level(zone_level);
    let safe_zone = 3.0 + (lvl / 16.0).floor();

    let effective_diff = ((lvl - lvl_diff).abs() - safe_zone).max(0.0);
    let over = (lvl - lvl_diff - safe_zone).ceil() as i32;

    // Base XP rate (out of 10000)
    let base = (lvl + 5.0).powf(1.5);
    let penalized = (lvl + 5.0 + effective_diff.powf(2.5)).powf(1.5);
    let mut xp_rate = ((base / penalized) * 10000.0).round();

    // Additional penalty for level 95+ (patch 2.4.0)
    if player_level >= 95 {
        xp_rate = (xp_rate * (1.0 / (1.0 + 0.1 * (lvl - 94.0)))).round();

        // High-level additional XP requirement multiplier (patch 3.1)
        let hl_modifier = match player_level {
            95 => 1.0 / 0.935,
            96 => 1.0 / 0.885,
            97 => 1.0 / 0.8125,
            98 => 1.0 / 0.7175,
            99 => 1.0 / 0.600,
            _ => 0.0,
        };
        if hl_modifier > 0.0 {
            xp_rate = (xp_rate / hl_modifier).round();
        }
    }

    // Convert to penalty percentage
    let mut penalty_pct = (10000.0 - xp_rate).round() / 100.0;

    // Since patch 2.0.0: minimum 1% XP (max 99% penalty)
    if penalty_pct > 99.0 {
        penalty_pct = 99.0;
    }
    if penalty_pct < 0.0 {
        penalty_pct = 0.0;
    }

    (penalty_pct, over)
}

/// Calculate the safe zone level range for a given player level.
/// Returns (min_safe_zone, max_safe_zone) — the zone levels that have 0% penalty.
pub fn safe_zone_range(player_level: u32) -> (u32, u32) {
    let lvl = player_level as f64;
    let safe = 3.0 + (lvl / 16.0).floor();
    let min = ((lvl - safe).max(1.0)) as u32;
    let max = (lvl + safe) as u32;
    (min, max)
}

/// How many more player levels can be gained before getting an XP penalty
/// in the current zone. Returns 0 if already penalized.
pub fn levels_until_penalty(player_level: u32, zone_level: u32) -> u32 {
    let (penalty, _) = calculate_exp_penalty(player_level, zone_level);
    if penalty > 0.0 {
        return 0;
    }
    // Binary search upward for when penalty kicks in
    for test_level in (player_level + 1)..=100 {
        let (p, _) = calculate_exp_penalty(test_level, zone_level);
        if p > 0.0 {
            return test_level - player_level - 1;
        }
    }
    100 - player_level
}

/// Detailed XP status for the UI.
pub enum ExpStatus {
    /// Player is under-leveled: X levels below zone, penalty%, highest penalty-free zone
    UnderLeveled {
        levels_under: u32,
        penalty_pct: f64,
        max_safe_zone: u32,
    },
    /// No penalty: how many levels above the zone's safe minimum the player is
    NoPenalty {
        levels_over_min: u32,
    },
    /// Player is over-leveled: penalty%
    OverLeveled {
        penalty_pct: f64,
    },
}

/// Compute the detailed XP status for the UI.
pub fn detailed_exp_status(player_level: u32, zone_level: u32) -> ExpStatus {
    let (penalty_pct, over) = calculate_exp_penalty(player_level, zone_level);

    if penalty_pct <= 0.0 {
        let (min_safe, _) = safe_zone_range(player_level);
        let levels_over_min = player_level.saturating_sub(min_safe);
        ExpStatus::NoPenalty { levels_over_min }
    } else if over > 0 {
        // Player level is above the zone's safe ceiling
        ExpStatus::OverLeveled { penalty_pct }
    } else {
        // Player level is below the zone's safe floor (under-leveled)
        let levels_under = (zone_level as i32 - player_level as i32).unsigned_abs();
        let (_, max_safe) = safe_zone_range(player_level);
        ExpStatus::UnderLeveled {
            levels_under,
            penalty_pct,
            max_safe_zone: max_safe,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_penalty_same_level() {
        let (pct, _) = calculate_exp_penalty(10, 10);
        assert!((pct - 0.0).abs() < 0.01, "Same level should have 0% penalty, got {}%", pct);
    }

    #[test]
    fn test_no_penalty_within_safe_zone() {
        // Level 16: safe zone = 3 + floor(16/16) = 4
        // Zone 20: diff = 4, safe = 4, effective_diff = 0
        let (pct, _) = calculate_exp_penalty(16, 20);
        assert!((pct - 0.0).abs() < 0.01, "Within safe zone should have 0% penalty, got {}%", pct);
    }

    #[test]
    fn test_penalty_outside_safe_zone() {
        // Level 50, Zone 30: big difference, should have penalty
        let (pct, over) = calculate_exp_penalty(50, 30);
        assert!(pct > 0.0, "Large level diff should have penalty");
        assert!(over > 0, "Should be over-leveled");
    }

    #[test]
    fn test_high_level_penalty() {
        // Level 95+ has additional penalties
        let (pct, _) = calculate_exp_penalty(99, 70);
        assert!(pct > 50.0, "Level 99 in zone 70 should have heavy penalty, got {}%", pct);
    }

    #[test]
    fn test_effective_zone_level() {
        assert_eq!(get_effective_zone_level(50), 50.0);
        assert_eq!(get_effective_zone_level(70), 70.0);
        assert!((get_effective_zone_level(71) - 70.94).abs() < 0.01);
        assert!((get_effective_zone_level(84) - 77.70).abs() < 0.01);
    }

    #[test]
    fn test_safe_zone_range() {
        // Level 10: safe = 3 + floor(10/16) = 3
        let (min, max) = safe_zone_range(10);
        assert_eq!(min, 7);
        assert_eq!(max, 13);
    }

    #[test]
    fn test_levels_until_penalty() {
        // At level 10, zone 10: should have several levels before penalty
        let remaining = levels_until_penalty(10, 10);
        assert!(remaining > 0, "Should have levels remaining at same level");
    }

    #[test]
    fn test_detailed_status_no_penalty() {
        let status = detailed_exp_status(10, 10);
        assert!(matches!(status, ExpStatus::NoPenalty { .. }));
    }

    #[test]
    fn test_detailed_status_over_leveled() {
        let status = detailed_exp_status(50, 20);
        assert!(matches!(status, ExpStatus::OverLeveled { .. }));
    }

    #[test]
    fn test_detailed_status_under_leveled() {
        let status = detailed_exp_status(10, 50);
        assert!(matches!(status, ExpStatus::UnderLeveled { .. }));
    }
}
