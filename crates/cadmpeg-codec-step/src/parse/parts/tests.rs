// SPDX-License-Identifier: Apache-2.0
use super::parse;
use crate::parse::{ParseDiagnosticKind, Value};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn real_record_callback_keeps_later_typed_parameters_and_one_exact_failed_statement() {
    let defective = "#2=ITEM('literal;semicolon', /* ; */ ,3);";
    let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(1);{defective}#3=CARTESIAN_POINT('later',(4.,5.,6.));ENDSEC;END-ISO-10303-21;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::service())
            .expect("real STEP source");
    let (exchange, diagnostics, parts) =
        parse(root, &ctx).expect("independent terminated statement recovery");
    assert_eq!(
        exchange.records().keys().copied().collect::<Vec<_>>(),
        [1, 3]
    );
    let later = exchange.records()[&3].partials.first();
    assert_eq!(later.name, "CARTESIAN_POINT");
    assert_eq!(later.parameters[0], Value::String(b"later".to_vec()));
    let Value::List(coordinates) = &later.parameters[1] else {
        panic!("actual point coordinate aggregate");
    };
    assert_eq!(
        coordinates
            .iter()
            .map(|value| match value {
                Value::Real(value) => value.get(),
                _ => panic!("actual real coordinates"),
            })
            .collect::<Vec<_>>(),
        [4., 5., 6.]
    );
    let start = source.find(defective).expect("literal failed statement");
    assert_eq!(exchange.omitted_spans(), [start..start + defective.len()]);
    assert!(exchange.shared_part_home(start));
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| diagnostic.offset == start
            && diagnostic.kind == ParseDiagnosticKind::RecordOmitted));
    let (model, native, fidelity, losses) = parts.into_parts();
    assert_eq!(model.entity_count(), 0);
    assert!(native.namespace("step").is_none());
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, retained) = fidelity
        .retained_records()
        .first_key_value()
        .expect("one helper source home");
    assert_eq!(retained.data(), Some(defective.as_bytes()));
    assert_eq!(
        retained.offset(),
        u64::try_from(start).expect("source offset")
    );
    assert!(!retained.untyped().is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0]
        .message
        .contains(&format!("offset={start} length={}", defective.len())));
    assert!(losses[0]
        .message
        .starts_with("part step.simple_entity_instance "));
}

#[test]
fn complex_callback_preserves_real_partials_and_recovers_after_duplicate_partial() {
    let valid = "#1=(A('quoted);', /* ) ; */ 1) B(2));";
    let defective = "#2=(A(1) A(2));";
    let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;{valid}{defective}#3=ITEM(3);ENDSEC;END-ISO-10303-21;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::service())
            .expect("source extent");
    let (exchange, _, parts) = parse(root, &ctx).expect("complex statement callback");
    assert_eq!(
        exchange.records().keys().copied().collect::<Vec<_>>(),
        [1, 3]
    );
    let partials = &exchange.records()[&1].partials;
    assert_eq!(
        partials
            .iter()
            .map(|partial| partial.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(
        partials.first().parameters[0],
        Value::String(b"quoted);".to_vec())
    );
    assert_eq!(partials.first().parameters[1], Value::Integer(1));
    assert_eq!(
        exchange.records()[&3].partials.first().parameters[0],
        Value::Integer(3)
    );
    let (_, _, fidelity, losses) = parts.into_parts();
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, failed) = fidelity
        .retained_records()
        .first_key_value()
        .expect("one duplicate-partial failure");
    assert!(!failed.untyped().is_empty());
    assert_eq!(failed.data(), Some(defective.as_bytes()));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0]
        .message
        .starts_with("part step.complex_entity_instance "));
}

#[test]
fn cut_short_entities_use_real_parent_or_eof_without_synthesized_framing() {
    for (defective, closing, level) in [
        (
            "#2=ITEM(3 ",
            "ENDSEC;END-ISO-10303-21;",
            "step.simple_entity_instance",
        ),
        (
            "#2=(A(1) ",
            "ENDSEC;END-ISO-10303-21;",
            "step.complex_entity_instance",
        ),
        ("#2=(A(1))", "", "step.complex_entity_instance"),
        (
            "#2=ITEM('open;#3=ITEM();",
            "",
            "step.simple_entity_instance",
        ),
        (
            "#2=(A(/* open;#3=ITEM();",
            "",
            "step.complex_entity_instance",
        ),
    ] {
        let source = format!("ISO-10303-21;HEADER;FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(1);{defective}{closing}");
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::service())
                .expect("cut-short source extent");
        let (exchange, diagnostics, parts) =
            parse(root, &ctx).expect("actual parent or EOF bounds one damaged entity");
        assert_eq!(
            exchange.records().keys().copied().collect::<Vec<_>>(),
            [1],
            "{defective}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == ParseDiagnosticKind::EnvelopeIncomplete),
            closing.is_empty()
        );
        let (_, native, fidelity, losses) = parts.into_parts();
        assert!(native.namespace("step").is_none());
        assert_eq!(fidelity.retained_records().len(), 1);
        let (_, retained) = fidelity
            .retained_records()
            .first_key_value()
            .expect("one cut-short source home");
        assert!(!retained.untyped().is_empty());
        assert_eq!(retained.data(), Some(defective.as_bytes()));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "record.unreadable");
        assert!(losses[0].message.starts_with(&format!("part {level} ")));
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ParseDiagnosticKind::RecordOmitted));
    }
}
