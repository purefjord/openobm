//! Property tests: the parsers must never panic, overflow, or read out of
//! bounds on arbitrary input (GOAL.md section 9). With `overflow-checks = true`
//! in the test profile, a silent Java-style wraparound that wasn't made explicit
//! would surface here as a panic.
//!
//! This is the cheap, in-process complement to `cargo fuzz`; the same entry
//! points (`parse_jtm`, `parse_lang`) are the libfuzzer targets under `fuzz/`.

use formats::{parse_jtm, parse_lang, parse_scr, ScriptVm};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn parse_jtm_never_panics(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = parse_jtm(&data);
    }

    #[test]
    fn parse_lang_never_panics(
        data in proptest::collection::vec(any::<u8>(), 0..512),
        count in 0usize..400,
    ) {
        let _ = parse_lang(&data, count);
    }

    #[test]
    fn parse_scr_and_vm_never_panic(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(program) = parse_scr(&data) {
            // Tracing every entry must never panic or loop unboundedly.
            let mut vm = ScriptVm::new(&program);
            for id in 0u8..=255 {
                let _ = vm.trace_entry(id, 256);
            }
        }
    }
}

// Small dimensions with adversarial RLE payloads — the region most likely to
// expose an index or run-length bug.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn parse_jtm_small_dims(
        w in 0u8..8,
        h in 0u8..8,
        payload in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut data = vec![w, h];
        data.extend_from_slice(&payload);
        if let Ok(map) = parse_jtm(&data) {
            prop_assert_eq!(map.width, usize::from(w));
            prop_assert_eq!(map.height, usize::from(h));
            for layer in &map.layers {
                prop_assert_eq!(layer.len(), usize::from(w) * usize::from(h));
            }
        }
    }
}
