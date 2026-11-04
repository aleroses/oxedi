use super::mask::{date_datum, masked_date};
use super::*;
use crate::{DateMode, Document, Mask, Phi, Rule, SnipLevel};

const SAMPLE: &[u8] = include_bytes!("../../tests/fixtures/balanced_5010_sample.txt");
const KEY: &[u8] = b"oxedi-test-key-0";

fn mask() -> Mask {
    Mask::new(KEY, DateMode::Year).unwrap()
}

/// The index of the first segment `id` whose first element is `first`, and
/// that element.
fn find(document: &Document<'_>, id: &[u8], first: Option<&[u8]>) -> (usize, Vec<u8>) {
    document
        .segments()
        .find_map(|segment| {
            let value = segment.element(1)?.simple()?.to_vec();
            (segment.id == id && first.is_none_or(|first| first == value.as_slice()))
                .then_some((segment.index, value))
        })
        .unwrap()
}

fn external(message: String, segment: Option<usize>, element: usize, datum: &[u8]) -> Diagnostic {
    Diagnostic::new(
        Rule::External {
            origin: "pyx12".into(),
            code: Some("8".into()),
            message,
            level: SnipLevel::L2,
        },
        segment,
        Some(element),
        None,
        vec![],
        datum.to_vec(),
    )
}

fn message(diagnostic: &Diagnostic) -> &str {
    match &diagnostic.rule {
        Rule::External { message, .. } => message,
        _ => panic!("not an external finding"),
    }
}

#[test]
fn mask_found_masks_the_datum_and_message_of_a_finding_on_a_flagged_place() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, id) = find(&document, b"CLP", None);
    let text = String::from_utf8(id.clone()).unwrap();
    let mut found = vec![external(
        format!("Invalid value '{text}' in CLP01"),
        Some(index),
        1,
        &id,
    )];

    projector.mask_found(&mut found, &document);

    let token = mask.text(Phi::Id, &id);
    assert_eq!(found[0].datum, token.to_vec());
    let token = String::from_utf8_lossy(&token).into_owned();
    assert!(!message(&found[0]).contains(&text));
    assert_eq!(
        message(&found[0]),
        format!("Invalid value '{token}' in CLP01")
    );
}

#[test]
fn mask_found_leaves_a_finding_on_an_unflagged_place_as_written() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, _) = find(&document, b"N1", Some(b"PR"));
    let name = b"ACME HEALTH PLAN";
    let original = external(
        "Invalid value 'ACME HEALTH PLAN' in N102".into(),
        Some(index),
        2,
        name,
    );
    let mut found = vec![original.clone()];

    projector.mask_found(&mut found, &document);

    assert_eq!(found, vec![original]);
}

#[test]
fn mask_found_leaves_a_finding_without_a_segment_or_past_the_end_as_written() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (_, id) = find(&document, b"CLP", None);
    let text = String::from_utf8(id.clone()).unwrap();
    let originals = vec![
        external(format!("Invalid value '{text}'"), None, 1, &id),
        external(format!("Invalid value '{text}'"), Some(usize::MAX), 1, &id),
    ];
    let mut found = originals.clone();

    projector.mask_found(&mut found, &document);

    assert_eq!(found, originals);
}

#[test]
fn mask_found_does_nothing_without_a_mask() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let projector = Projector::new(&spec, document.delimiters());
    let (index, id) = find(&document, b"CLP", None);
    let text = String::from_utf8(id.clone()).unwrap();
    let original = external(format!("Invalid value '{text}'"), Some(index), 1, &id);
    let mut found = vec![original.clone()];

    projector.mask_found(&mut found, &document);

    assert_eq!(found, vec![original]);
}

#[test]
fn mask_found_replaces_every_occurrence_of_a_one_character_datum() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, _) = find(&document, b"CLP", None);
    let mut found = vec![external("1 of 1".into(), Some(index), 1, b"1")];

    projector.mask_found(&mut found, &document);

    let token = mask.text(Phi::Id, b"1");
    assert_eq!(found[0].datum, token.to_vec());
    let token = String::from_utf8_lossy(&token).into_owned();
    assert_eq!(message(&found[0]), format!("{token} of {token}"));
}

#[test]
fn mask_found_replaces_a_datum_quoted_as_latin_1_text() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, _) = find(&document, b"NM1", Some(b"QC"));
    let datum = b"D\xe9OE";
    let mut found = vec![external(
        "contains an invalid character(D\u{e9}OE)".into(),
        Some(index),
        3,
        datum,
    )];

    projector.mask_found(&mut found, &document);

    let token = mask.text(Phi::Name, datum);
    assert_eq!(found[0].datum, token.to_vec());
    let token = String::from_utf8_lossy(&token).into_owned();
    assert_eq!(
        message(&found[0]),
        format!("contains an invalid character({token})")
    );
}

#[test]
fn mask_found_masks_flagged_values_a_segment_level_message_quotes() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, id) = find(&document, b"CLP", None);
    let text = String::from_utf8(id.clone()).unwrap();
    let mut found = vec![Diagnostic::new(
        Rule::External {
            origin: "pyx12".into(),
            code: Some("1".into()),
            message: format!("Segment CLP*{text} not found."),
            level: SnipLevel::L2,
        },
        Some(index),
        None,
        None,
        vec![],
        b"CLP".to_vec(),
    )];

    projector.mask_found(&mut found, &document);

    let token = String::from_utf8_lossy(&mask.text(Phi::Id, &id)).into_owned();
    assert_eq!(found[0].datum, b"CLP".to_vec());
    assert_eq!(
        message(&found[0]),
        format!("Segment CLP*{token} not found.")
    );
}

#[test]
fn mask_found_rewrites_a_message_once_when_the_datum_and_the_segment_quote_values() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, id) = find(&document, b"CLP", None);
    let text = String::from_utf8(id.clone()).unwrap();
    let mut found = vec![external(
        format!("1 is not CLP*{text}"),
        Some(index),
        1,
        b"1",
    )];

    projector.mask_found(&mut found, &document);

    let one = String::from_utf8_lossy(&mask.text(Phi::Id, b"1")).into_owned();
    let token = String::from_utf8_lossy(&mask.text(Phi::Id, &id)).into_owned();
    assert_eq!(message(&found[0]), format!("{one} is not CLP*{token}"));
}

#[test]
fn mask_found_leaves_values_of_another_segment_in_the_message_as_written() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (_, id) = find(&document, b"CLP", None);
    let (index, _) = find(&document, b"TRN", None);
    let text = String::from_utf8(id).unwrap();
    let original = Diagnostic::new(
        Rule::External {
            origin: "pyx12".into(),
            code: Some("1".into()),
            message: format!("Segment CLP*{text} not found."),
            level: SnipLevel::L2,
        },
        Some(index),
        None,
        None,
        vec![],
        b"TRN".to_vec(),
    );
    let mut found = vec![original.clone()];

    projector.mask_found(&mut found, &document);

    assert_eq!(found, vec![original]);
}

/// An original and the masked form `unquoted` writes for it.
type Pair<'a> = (&'a [u8], &'a str);

/// The originals and masked forms `unquoted` takes, from text pairs.
fn pairs(values: &[Pair<'_>]) -> Vec<(Vec<u8>, Vec<u8>)> {
    values
        .iter()
        .map(|(original, masked)| (original.to_vec(), masked.as_bytes().to_vec()))
        .collect()
}

#[test]
fn mask_found_masks_a_one_character_datum_quoted_after_a_word_that_starts_with_it() {
    let spec = Spec::builtin_835();
    let document = Document::parse(SAMPLE).unwrap();
    let mask = mask();
    let projector = Projector::new(&spec, document.delimiters()).with_mask(&mask);
    let (index, _) = find(&document, b"NM1", Some(b"QC"));
    let mut found = vec![external(
        "Data element too long: \"D\"".into(),
        Some(index),
        3,
        b"D",
    )];

    projector.mask_found(&mut found, &document);

    let token = String::from_utf8_lossy(&mask.text(Phi::Name, b"D")).into_owned();
    assert_eq!(
        message(&found[0]),
        format!("Data element too long: \"{token}\"")
    );
}

#[test]
fn unquoted_replaces_a_short_value_that_stands_alone() {
    let quoted = pairs(&[(b"7", "<7>")]);
    for (message, expected) in [
        ("Segment CLP*7*1 not found", "Segment CLP*<7>*1 not found"),
        ("value (7)", "value (<7>)"),
        ("value '7'", "value '<7>'"),
        ("value \"7\"", "value \"<7>\""),
        ("7", "<7>"),
        ("7 is first", "<7> is first"),
        ("last is 7", "last is <7>"),
        ("7*7", "<7>*<7>"),
    ] {
        assert_eq!(
            super::quote::unquoted(message, &quoted),
            expected,
            "{message}"
        );
    }
}

#[test]
fn unquoted_leaves_a_short_value_inside_a_word_or_number() {
    let quoted = pairs(&[(b"7", "<7>"), (b"D", "<D>")]);
    for message in ["CLP07", "17 of 27", "7A", "Data element", "ID", "x7y"] {
        assert_eq!(super::quote::unquoted(message, &quoted), message);
    }
}

#[test]
fn unquoted_replaces_a_value_of_four_characters_or_more_anywhere() {
    let quoted = pairs(&[(b"ABCD", "<abcd>")]);
    assert_eq!(
        super::quote::unquoted("xABCDx and ABCD", &quoted),
        "x<abcd>x and <abcd>"
    );
}

#[test]
fn unquoted_replaces_overlapping_values_by_their_tokens_in_start_order() {
    for quoted in [
        pairs(&[(b"XAB", "<x>"), (b"ABYZ", "<y>")]),
        pairs(&[(b"ABYZ", "<y>"), (b"XAB", "<x>")]),
    ] {
        assert_eq!(super::quote::unquoted("(XABYZ)", &quoted), "(<x><y>)");
        assert_eq!(super::quote::unquoted("XABYZ", &quoted), "<x><y>");
    }
    let quoted = pairs(&[(b"WXAB", "<w>"), (b"ABYZ", "<y>")]);
    assert_eq!(super::quote::unquoted("is WXABYZ.", &quoted), "is <w><y>.");
}

#[test]
fn unquoted_replaces_a_value_held_in_a_longer_one_by_the_longer_token() {
    let quoted = pairs(&[(b"AB", "<ab>"), (b"ABCD", "<abcd>")]);
    assert_eq!(super::quote::unquoted("ABCD AB", &quoted), "<abcd> <ab>");
}

#[test]
fn unquoted_reads_a_short_value_as_latin_1_or_utf_8_on_boundaries() {
    let latin1 = pairs(&[(b"D\xe9", "<d>")]);
    assert_eq!(
        super::quote::unquoted("character(D\u{e9}) not XD\u{e9}", &latin1),
        "character(<d>) not XD\u{e9}"
    );
    let utf8 = pairs(&[("\u{e9}".as_bytes(), "<e>")]);
    assert_eq!(
        super::quote::unquoted("character(\u{e9}) not a\u{e9}", &utf8),
        "character(<e>) not a\u{e9}"
    );
}

#[test]
fn unquoted_replaces_short_values_glued_to_each_other_as_one_group() {
    let cases: [(&[Pair<'_>], &str, &str); 4] = [
        (&[(b"AB", "<ab>"), (b"CD", "<cd>")], "ABCD", "<ab><cd>"),
        (&[(b"D", "<d>"), (b"J", "<j>")], "*DJ*", "*<d><j>*"),
        (&[(b"AB", "<ab>"), (b"BC", "<bc>")], "(ABC)", "(<ab><bc>)"),
        (
            &[(b"XXXX", "<l>"), (b"AB", "<ab>"), (b"BC", "<bc>")],
            "XXXXABC",
            "<l><ab><bc>",
        ),
    ];
    for (values, message, expected) in cases {
        assert_eq!(
            super::quote::unquoted(message, &pairs(values)),
            expected,
            "{message}"
        );
    }
}

#[test]
fn unquoted_leaves_a_group_of_short_values_glued_to_a_word() {
    let quoted = pairs(&[(b"AB", "<ab>"), (b"CD", "<cd>")]);
    assert_eq!(super::quote::unquoted("xABCD", &quoted), "xABCD");
}

#[test]
fn unquoted_leaves_a_short_value_glued_to_a_letter_no_value_holds() {
    let quoted = pairs(&[(b"XAB", "<x>"), (b"ABYZ", "<y>")]);
    assert_eq!(super::quote::unquoted("QXABYZ", &quoted), "QX<y>");
}

#[test]
fn unquoted_replaces_a_word_two_short_values_spell_together() {
    let quoted = pairs(&[(b"A", "<a>"), (b"N", "<n>")]);
    assert_eq!(
        super::quote::unquoted("Data element is type AN", &quoted),
        "Data element is type <a><n>"
    );
}

#[test]
fn unquoted_counts_a_value_by_the_characters_of_its_utf_8_reading() {
    let quoted = pairs(&[("AB\u{e9}".as_bytes(), "<v>")]);
    assert_eq!(
        super::quote::unquoted("xAB\u{c3}\u{a9}x and xAB\u{e9}x", &quoted),
        "xAB\u{c3}\u{a9}x and xAB\u{e9}x"
    );
    assert_eq!(
        super::quote::unquoted("(AB\u{c3}\u{a9}) and (AB\u{e9})", &quoted),
        "(<v>) and (<v>)"
    );
}

#[test]
fn unquoted_takes_an_accented_letter_or_a_hyphen_as_a_boundary() {
    let quoted = pairs(&[(b"D", "<d>")]);
    assert_eq!(
        super::quote::unquoted("D\u{e9} and D-1", &quoted),
        "<d>\u{e9} and <d>-1"
    );
}

#[test]
fn a_date_with_no_year_start_is_masked_as_keyed_text() {
    let mask = mask();
    let token = mask.text(Phi::Date, b"20240318").to_vec();
    assert_eq!(date_datum(&mask, Some(i32::MIN), b"20240318"), token);
    assert_eq!(date_datum(&mask, None, b"20240318"), token);
    assert_eq!(date_datum(&mask, Some(19_800), b"20240318"), b"20240101");
    assert_eq!(masked_date(DateMode::Year, i32::MIN, None), None);
    assert_eq!(masked_date(DateMode::Shift, i32::MIN, Some(-1)), None);
    assert_eq!(masked_date(DateMode::Year, 19_800, None), Some(19_723));
}
