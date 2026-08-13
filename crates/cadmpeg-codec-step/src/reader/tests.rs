// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn byte_accounting_reports_an_unrecognized_suffix() {
    let input = include_bytes!("../../tests/fixtures/ap242_minimal.p21");
    let (mut exchange, _) = crate::parse::parse(input).expect("parse accounting fixture");
    let mut extended = input.to_vec();
    extended.push(0xc3);

    let accounting = byte_accounting(&extended, &exchange, &HashSet::new());

    assert_eq!(accounting.unclassified, 1);
    assert_eq!(
        accounting.structural + accounting.typed + accounting.opaque + accounting.unclassified,
        extended.len()
    );

    let result = decode_exchange_mode(
        &extended,
        cadmpeg_ir::codec::DecodeOptions::default(),
        &mut exchange,
        &[],
        true,
        None,
    )
    .expect("synthesized unknown record conversion")
    .0;
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == LossKind::shared(LossTaxonomy::DecodeDiagnostic)
            && loss.severity == Severity::Error
            && loss.message.contains("1 byte(s) unclassified")
    }));
}

#[test]
fn byte_accounting_claims_controls_inside_print_directives() {
    let input = b"1\\\x01N\x02\\2";
    let mut classes = vec![ByteClass::Unclassified; input.len()];

    claim_trivia(input, 1..input.len(), &mut classes);

    assert!(classes[1..6]
        .iter()
        .all(|class| *class == ByteClass::Structural));
}

#[test]
fn semantic_work_counts_nested_source_graph_nodes() {
    let simple = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;";
    let nested = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(((1,2),TYPE((3,4))));ENDSEC;END-ISO-10303-21;";
    let (simple_exchange, _) = crate::parse::parse(simple).expect("simple exchange");
    let (nested_exchange, _) = crate::parse::parse(nested).expect("nested exchange");

    assert!(semantic_input_work(&nested_exchange) > semantic_input_work(&simple_exchange));
}

#[test]
fn implicit_face_plane_work_scales_with_point_count() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('test','2026-07-14T00:00:00',('cadmpeg'),('cadmpeg'),'cadmpeg-step','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POLY_LOOP('',(#2,#3,#4,#5));#2=ITEM();#3=ITEM();#4=ITEM();#5=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("polygon exchange");

    assert_eq!(implicit_face_plane_work(&exchange), 4);
}
