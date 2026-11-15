use super::{write_repeating, SpellingSweep, Step};

#[test]
fn the_repeated_key_is_the_first_key_of_the_named_object_only() {
    let value = serde_json::json!({"a": [{"b": 1, "c": 2}, {"b": 3}], "d": {"e": 4}});
    let mut text = String::new();
    write_repeating(
        &value,
        &[Step::Key("a".to_owned()), Step::Index(1)],
        &mut text,
    );
    assert_eq!(text, r#"{"a":[{"b":1,"c":2},{"b":3,"b":3}],"d":{"e":4}}"#);
    let mut text = String::new();
    write_repeating(&value, &[], &mut text);
    assert_eq!(
        text,
        r#"{"a":[{"b":1,"c":2},{"b":3}],"a":[{"b":1,"c":2},{"b":3}],"d":{"e":4}}"#
    );
}

#[test]
fn every_example_document_has_one_spelling() {
    let documents = [
        cadmpeg_ir::CadIr::empty(),
        cadmpeg_ir::examples::unit_cube().expect("the unit cube builds"),
        cadmpeg_ir::examples::unit_cube_with_history_body().expect("the history cube builds"),
        cadmpeg_ir::examples::directed_subd_sum().expect("the subd sum builds"),
    ];
    let values = documents
        .iter()
        .map(|ir| serde_json::to_value(ir).expect("a CadIr serializes"))
        .collect::<Vec<_>>();
    let mut sweep = SpellingSweep::default();
    for value in &values {
        sweep.learn(value);
    }
    let mut accepting = std::collections::BTreeSet::new();
    for (ir, value) in documents.iter().zip(&values) {
        accepting.extend(sweep.sweep(value).expect("an example reads back"));
        let text = ir.to_canonical_json().expect("an example writes");
        let read = cadmpeg_ir::CadIr::from_json(&text).expect("an example reads its own text");
        assert_eq!(read.to_canonical_json().expect("rewrites"), text);
    }
    assert!(sweep.probes > 0);
    assert!(
        accepting.is_empty(),
        "{} second spellings are admitted:\n{}",
        accepting.len(),
        accepting.into_iter().collect::<Vec<_>>().join("\n")
    );
}
