// SPDX-License-Identifier: Apache-2.0
//! Scope offsets that name another record: the record headers of a
//! `CopyPasteBodies` operation and of pattern instance frames, and the
//! long-form `CoilPrimitive` operation.

use super::*;

fn decoded_fixture(name: &str) -> (CadIr, SourceFidelity) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/fixtures")
        .join(name);
    let source = std::fs::read(path).expect("read fixture");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source[..]), &DecodeOptions::default())
        .expect("design fixture decodes");
    let (ir, _, fidelity) = decoded.into_parts();
    (ir, fidelity)
}

/// The sidecar with every pointer of `id` whose name starts with one of
/// `prefixes` moved by `delta`.
fn with_moved_pointers(
    fidelity: &SourceFidelity,
    id: &str,
    prefixes: &[&str],
    delta: u64,
) -> SourceFidelity {
    let names = fidelity.record_layout()[id]
        .pointers()
        .keys()
        .filter(|name| prefixes.iter().any(|prefix| name.starts_with(prefix)))
        .cloned()
        .collect::<Vec<_>>();
    assert!(!names.is_empty());
    names.iter().fold(fidelity.clone(), |fidelity, name| {
        let value = pointer(&fidelity, id, name) + delta;
        with_pointer(&fidelity, id, name, Some(value))
    })
}

#[test]
fn copy_paste_bodies_offsets_name_their_record_headers() {
    let (ir, fidelity) = decoded_fixture("design_copy_paste_bodies.f3d");
    assert!(findings(&ir, &fidelity).is_empty());
    let scope = ids(&ir, "design_parameter_scopes")
        .into_iter()
        .find(|id| {
            fidelity.record_layout()[id]
                .pointers()
                .contains_key("copy_paste_bodies_operation.body_group_byte_offset")
        })
        .expect("a CopyPasteBodies scope");
    // Each run moves with its header, so only the header link breaks.
    for prefixes in [
        &[
            "copy_paste_bodies_operation.body_group_byte_offset",
            "copy_paste_bodies_operation.body_operand_record_offsets",
        ][..],
        &[
            "copy_paste_bodies_operation.relation_byte_offset",
            "copy_paste_bodies_operation.source_body_entity_suffix_offsets",
            "copy_paste_bodies_operation.copied_body_entity_suffix_offsets",
        ][..],
    ] {
        let moved = with_moved_pointers(&fidelity, &scope, prefixes, 1);
        only_finding(&findings(&ir, &moved), SCOPE_MESSAGE, &scope);
    }
}

const PATTERN_HEADERS: [(u32, u64); 2] = [(60, 5_000), (61, 6_000)];

/// The rectangular-pattern lane namespace with two body instances whose
/// frames follow their record headers by `transform_delta` bytes.
fn pattern_instance_namespace(transform_delta: [i64; 2]) -> F3dNative {
    let mut native = rectangular_pattern_namespace();
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut construction: DesignRectangularPatternConstruction = serde_json::from_value(json!({
        "u_count": 2, "v_count": 1, "u_extent": 1.0, "v_extent": 0.0,
        "owner_record_indices": [10, 20, 30, 40],
        "instances": {"record_indices": [60, 61], "transforms": [identity, identity]},
    }))
    .expect("pattern construction");
    construction.value_offsets = Some([1_040, 2_040, 3_040, 4_040]);
    let scope = &mut native.design_parameter_scopes[0];
    if let DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut() {
        *slot = Some(construction);
    }
    let mut pointers = crate::records::source_offsets::collect(&mut scope.clone());
    for (index, ((_, header), delta)) in PATTERN_HEADERS.iter().zip(transform_delta).enumerate() {
        pointers.insert(
            format!("rectangular_pattern_construction.instances.transform_offsets.{index}"),
            header.checked_add_signed(delta).expect("fixture offset"),
        );
    }
    assert!(crate::records::source_offsets::restore(scope, &pointers).is_empty());
    for (record_index, byte_offset) in PATTERN_HEADERS {
        native
            .design_record_headers
            .push(crate::records::decal::DesignRecordHeader {
                id: format!("f3d:{STREAM}:design-record-header#{byte_offset}"),
                record_index,
                class_tag: "300".to_owned().try_into().unwrap(),
                byte_offset: None,
            });
    }
    native
}

#[test]
fn pattern_instance_frames_follow_their_record_headers() {
    let native = pattern_instance_namespace([100, 100]);
    assert!(loaded_findings(&native, &lane_sidecar(&native, None)).is_empty());
    let scope = native.design_parameter_scopes[0].id.clone();
    for delta in [[100, -100], [0, 100]] {
        let native = pattern_instance_namespace(delta);
        only_finding(
            &loaded_findings(&native, &lane_sidecar(&native, None)),
            SCOPE_MESSAGE,
            &scope,
        );
    }
}

#[test]
fn a_pattern_instance_frame_without_its_record_header_fails() {
    let mut native = pattern_instance_namespace([100, 100]);
    native.design_record_headers.pop();
    let scope = native.design_parameter_scopes[0].id.clone();
    only_finding(
        &loaded_findings(&native, &lane_sidecar(&native, None)),
        SCOPE_MESSAGE,
        &scope,
    );
}

/// A ten-reference `CoilPrimitive` scope whose operation is at `operation`
/// bytes from the frame and whose section is stored or fixed by the form.
fn coil_namespace(operation: u64, section_stored: bool) -> F3dNative {
    use crate::records::feature::coil::{DesignCoilScope, DesignCoilSection};
    use crate::records::identity::{MaybeRecordedValue, RecordedValue};
    let mut scope = lane_scope(
        &format!("f3d:{STREAM}:coil#200"),
        DesignFeatureKind::CoilPrimitive,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::from_columns(
                (1..=10).map(|index| index * 10).collect(),
                vec![0; 10],
                "reference_members",
            )
            .unwrap();
            draft.frame_length = 300;
            draft.paired_byte_offset = draft.byte_offset + 300;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .expect("ten-reference coil frame");
    if let DesignScopePayloadMut::CoilPrimitive(slot) = scope.payload_mut() {
        *slot = Some(DesignCoilScope {
            operation: Some(RecordedValue {
                value: crate::records::feature::extrude::DesignExtrudeOperation::Join,
                offset: Some(LANE_SCOPE_START + operation),
            }),
            section: Some(if section_stored {
                MaybeRecordedValue::Located(RecordedValue {
                    value: DesignCoilSection::Circular,
                    offset: Some(LANE_SCOPE_START + 92),
                })
            } else {
                MaybeRecordedValue::Unlocated(DesignCoilSection::Circular)
            }),
            ..DesignCoilScope::default()
        });
    }
    let mut native = F3dNative::default();
    native.design_parameter_scopes.push(scope);
    native
}

/// The sidecar of `coil_namespace`: the capture of the 300-byte coil frame.
fn coil_sidecar(native: &F3dNative) -> SourceFidelity {
    let scope = &native.design_parameter_scopes[0];
    let pointers = crate::records::source_offsets::collect(&mut scope.clone());
    let pointers = pointers
        .iter()
        .map(|(name, offset)| (name.as_str(), *offset))
        .collect::<Vec<_>>();
    crate::test_support::source_layout::sidecar(
        STREAM,
        &[(scope.id.as_str(), LANE_SCOPE_START, 300, &pointers[..])],
    )
}

#[test]
fn the_long_coil_form_has_its_operation_at_prologue_offset_22() {
    let long = coil_namespace(22, false);
    assert!(loaded_findings(&long, &coil_sidecar(&long)).is_empty());
    let compact = coil_namespace(20, true);
    assert!(loaded_findings(&compact, &coil_sidecar(&compact)).is_empty());
    let scope = long.design_parameter_scopes[0].id.clone();
    for (operation, section_stored) in [(20, false), (22, true)] {
        let native = coil_namespace(operation, section_stored);
        only_finding(
            &loaded_findings(&native, &coil_sidecar(&native)),
            SCOPE_MESSAGE,
            &scope,
        );
    }
}
