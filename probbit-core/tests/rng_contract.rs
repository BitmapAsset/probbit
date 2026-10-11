use probbit_core::{Philox4x32, SplitMix64};

#[test]
fn philox_matches_published_random123_zero_vector() {
    // Random123's Philox4x32-10 known-answer test: counter and key all zero.
    let mut rng = Philox4x32::new(0, 0);
    assert_eq!(
        [
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32()
        ],
        [0x6627e8d5, 0xe169c58d, 0xbc57ac4c, 0x9b00dbd8]
    );
}

#[test]
fn splitmix_and_philox_stream_contracts_are_pinned() {
    let mut r = SplitMix64(0);
    assert_eq!(r.next_u64(), 0xe220a8397b1dcdaf);
    assert_eq!(r.next_u64(), 0x6e789e6aa1b965f4);
    let mut whole = Philox4x32::new(7, 11);
    let mut words = whole.clone();
    for _ in 0..1000 {
        assert_eq!(
            whole.next_u64(),
            ((words.next_u32() as u64) << 32) | words.next_u32() as u64
        );
    }
    let mut a = Philox4x32::new(13, 19);
    let mut b = a.clone();
    for _ in 0..1000 {
        let x = a.f64();
        assert!((0.0..1.0).contains(&x));
        assert_eq!(x, (b.next_u64() >> 11) as f64 / 9007199254740992.0);
    }
}
