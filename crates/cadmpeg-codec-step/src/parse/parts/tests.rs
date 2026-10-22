// SPDX-License-Identifier: Apache-2.0
use super::parse;
use crate::parse::{ParseDiagnosticKind, Value};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::source_fidelity::parts::PartState;

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
    let (model, native, fidelity, losses) = parts.into_parts().into_parts();
    assert_eq!(model.entity_count(), 0);
    let unknowns = native
        .namespace("step")
        .expect("shared failure namespace")
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .expect("typed helper stubs");
    assert_eq!(unknowns.len(), 1);
    let stub = &unknowns[0];
    assert!(stub.links.is_empty());
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(
        fidelity
            .retained_record(stub.id.as_str())
            .expect("one helper source home")
            .data(),
        Some(defective.as_bytes())
    );
    let node = &fidelity.parts()[&stub.id];
    assert_eq!(node.level().as_str(), "step.simple_entity_instance");
    assert_eq!(node.state(), &PartState::GivenUp);
    assert_eq!(node.offset(), u64::try_from(start).expect("source offset"));
    assert_eq!(
        node.byte_len(),
        u64::try_from(defective.len()).expect("source extent")
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|node| node.state() == &PartState::Decoded)
            .count(),
        2
    );
    let layout = &fidelity.record_layout()[stub.id.as_str()];
    assert_eq!(layout.offset(), node.offset());
    assert_eq!(layout.byte_len(), node.byte_len());
    assert!(layout.reads().is_empty());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0]
        .message
        .contains(&format!("offset={start} length={}", defective.len())));
    assert!(losses[0].message.ends_with("dropped=[]"));
}
