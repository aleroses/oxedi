use std::error::Error;

use proptest::prelude::*;

use super::*;

const KEY: &[u8] = b"0123456789abcdef";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn hmac_matches_rfc_4231_case_1() {
    let out = hmac_sha256(&[0x0b; 20], b"Hi There");
    assert_eq!(
        hex(&out),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
}

#[test]
fn hmac_matches_rfc_4231_case_2() {
    let out = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    assert_eq!(
        hex(&out),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn hmac_matches_rfc_4231_case_4() {
    let key: Vec<u8> = (1..=25).collect();
    let out = hmac_sha256(&key, &[0xcd; 50]);
    assert_eq!(
        hex(&out),
        "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b"
    );
}

#[test]
fn text_is_the_head_of_the_keyed_digest() {
    let mask = Mask::new(KEY, DateMode::Year).unwrap();
    let full = hmac_sha256(KEY, b"id\0PCN1001");
    assert_eq!(
        std::str::from_utf8(&mask.text(Phi::Id, b"PCN1001")).unwrap(),
        hex(&full[..8])
    );
    assert_ne!(
        mask.text(Phi::Name, b"PCN1001"),
        mask.text(Phi::Id, b"PCN1001")
    );
}

#[test]
fn text_is_deterministic_and_key_sensitive() {
    let a = Mask::new(KEY, DateMode::Year).unwrap();
    let b = Mask::new(b"fedcba9876543210", DateMode::Year).unwrap();
    assert_eq!(a.text(Phi::Id, b"x"), a.text(Phi::Id, b"x"));
    assert_ne!(a.text(Phi::Id, b"x"), b.text(Phi::Id, b"x"));
}

/// The shift worked out here from the formula, with the crates directly.
fn expected_shift(key: &[u8], seed: &[u8]) -> i32 {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).unwrap();
    mac.update(b"shift\0");
    mac.update(seed);
    let digest = mac.finalize().into_bytes();
    let head = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    i32::try_from(head % 365).unwrap() - 182
}

#[test]
fn shift_days_matches_the_formula_for_two_known_pairs() {
    let cases: [(&[u8], &[u8], i32); 2] =
        [(KEY, b"PAT1001", -26), (b"oxedi-test-key-1", b"CLM-2", 4)];
    for (key, seed, pinned) in cases {
        let mask = Mask::new(key, DateMode::Shift).unwrap();
        assert_eq!(expected_shift(key, seed), pinned);
        assert_eq!(mask.shift_days(seed), pinned);
    }
}

#[test]
fn year_start_has_no_value_before_the_i32_range() {
    assert_eq!(Mask::year_start(i32::MIN), None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]
    #[test]
    fn shift_stays_in_range(seed in proptest::collection::vec(any::<u8>(), 0..32)) {
        let mask = Mask::new(KEY, DateMode::Shift).unwrap();
        let d = mask.shift_days(&seed);
        prop_assert!((-182..=182).contains(&d));
        prop_assert_eq!(d, mask.shift_days(&seed));
    }
}

#[test]
fn year_start_known_dates() {
    assert_eq!(Mask::year_start(19_800), Some(19_723)); // 2024-03-18
    assert_eq!(Mask::year_start(10_956), Some(10_592)); // 1999-12-31
    assert_eq!(Mask::year_start(0), Some(0));
    assert_eq!(Mask::year_start(-165), Some(-365)); // 1969-07-20
    assert_eq!(Mask::year_start(11_016), Some(10_957)); // 2000-02-29
    assert_eq!(Mask::year_start(-25_508), Some(-25_567)); // 1900-03-01
}

#[test]
fn short_key_and_date_mode_errors() {
    assert_eq!(
        Mask::new(&[0; 15], DateMode::Year).unwrap_err(),
        MaskError::ShortKey { length: 15 }
    );
    assert!(Mask::new(&[0; 16], DateMode::Year).is_ok());
    assert_eq!(
        DateMode::parse("month").unwrap_err(),
        MaskError::UnknownDateMode {
            given: "month".into()
        }
    );
    assert_eq!(DateMode::parse("shift").unwrap(), DateMode::Shift);
}

#[test]
fn debug_hides_the_key() {
    let mask = Mask::new(KEY, DateMode::Year).unwrap();
    assert_eq!(format!("{mask:?}"), "Mask { key: <16 bytes>, dates: Year }");
}

#[test]
fn display_short_key() {
    let e = MaskError::ShortKey { length: 15 };
    assert_eq!(
        e.to_string(),
        "the mask key must be at least 16 bytes long; got 15"
    );
    assert!(e.source().is_none());
}

#[test]
fn display_unknown_date_mode() {
    let e = MaskError::UnknownDateMode {
        given: "month".into(),
    };
    assert_eq!(
        e.to_string(),
        "the mask date mode must be \"year\", \"shift\" or \"keep\"; got \"month\""
    );
    assert!(e.source().is_none());
}

#[test]
fn date_mode_word_reads_back() {
    for (mode, word) in [
        (DateMode::Year, "year"),
        (DateMode::Shift, "shift"),
        (DateMode::Keep, "keep"),
    ] {
        assert_eq!(mode.word(), word);
        assert_eq!(DateMode::parse(mode.word()), Ok(mode));
    }
}
