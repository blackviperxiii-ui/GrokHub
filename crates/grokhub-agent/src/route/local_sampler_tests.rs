use super::*;
use grokhub_core::local_profile::PowerInfo;

#[test]
fn the_shells_readers_fill_game_power_heat_and_memory() {
    let slow = SlowRead {
        power: Some(PowerInfo {
            battery: true,
            on_ac: false,
            percent: Some(55),
            power_saver: false,
        }),
        thermal: Thermal::Serious,
        free_ram_mb: Some(6_000),
    };
    let s = signals(Some(true), &slow, Some(32_000), false);
    assert_eq!(s.game, Some(true));
    assert_eq!(s.thermal, Thermal::Serious);
    assert_eq!(s.power.map(|p| p.percent), Some(Some(55)));
    assert_eq!(
        s.memory,
        Some(Memory {
            avail_mb: 6_000,
            total_mb: 32_000,
        })
    );
    assert_eq!(s.fullscreen, None);
    let unread = signals(None, &SlowRead::default(), None, true);
    assert_eq!(unread.memory, None);
    assert!(unread.needed);
}

#[test]
fn a_game_pauses_the_model_at_its_first_sample() {
    local_gov::reset_for_test();
    let mut sampler = Sampler::new();
    let game = Signals {
        game: Some(true),
        ..Signals::default()
    };
    let want = sampler.observe(&game, 1_000);
    assert_eq!(want.map(|w| w.level), Some(Level::Unload));
    assert_eq!(local_gov::level(), Level::Unload);
    assert_eq!(local_gov::reason(), "a game is running");
    assert!(local_gov::gpu_contended());
    local_gov::reset_for_test();
}
