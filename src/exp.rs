pub fn get_effective_monster_level(level: u32) -> f64 {
    match level {
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
        _ => level as f64,
    }
}

pub fn calculate_exp_penalty(player_level: u32, raw_monster_level: u32) -> (f64, i32) {
    let monster_level = get_effective_monster_level(raw_monster_level);
    let safe_zone = (3.0 + (player_level as f64 / 16.0)).floor();
    
    let over = player_level as i32 - (monster_level.floor() as i32 - safe_zone as i32);

    let level_diff = (player_level as f64 - monster_level).abs();
    let effective_diff = (level_diff - safe_zone).max(0.0);
    
    let exp_penalty = (player_level as f64 + 5.0) / (player_level as f64 + 5.0 + (effective_diff.powi(5)).sqrt());
    let mut exp_multi = (exp_penalty.powi(3)).sqrt();

    if player_level >= 95 {
        exp_multi *= 1.0 / (1.0 + (0.1 * (player_level as f64 - 94.0)));
    }

    if exp_multi < 0.01 {
        exp_multi = 0.01;
    }

    (exp_multi, over)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exp_penalty() {
        let (pct, over) = calculate_exp_penalty(10, 10);
        // Level 10, Monster 10. Safe zone = 3 + 10/16 = 3.
        // effective_diff = 0
        // Penalty = 1.0.
        assert!((pct - 1.0).abs() < 0.001);
        assert_eq!(over, 3);
    }
}
