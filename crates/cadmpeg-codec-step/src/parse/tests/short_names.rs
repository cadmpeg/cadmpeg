// SPDX-License-Identifier: Apache-2.0
//! Part 21 short entity name tests.

#[test]
fn parser_resolves_short_entity_names_in_simple_and_complex_instances() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));ENDSEC;DATA;#1=CRTPNT('',(0.,0.,0.));#2=(LNGUNT()NMDUNT(*)SI_UNIT(.MILLI.,.METRE.));#3=DIRECTION('',(1.,0.,0.));ENDSEC;END-ISO-10303-21;";
    let (exchange, diagnostics) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("parse short names");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let names = |id: u64| -> Vec<String> {
        exchange.records()[&id]
            .partials
            .iter()
            .map(|partial| partial.name.clone())
            .collect()
    };
    assert_eq!(names(1), ["CARTESIAN_POINT"]);
    assert_eq!(names(2), ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"]);
    assert_eq!(names(3), ["DIRECTION"]);
}
