// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::check::Finding;
use cadmpeg_ir::source_fidelity::SourceFidelity;
use serde_json::json;

use super::super::Ctx;
use super::Layouts;
use crate::native::F3dNative;
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::patterns::{
    DesignRectangularPatternConstruction, DesignRectangularPatternConstructionWire,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::identity::Sourced;
use crate::records::parameters::{
    DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
    DesignParameterSource,
};
use crate::F3dCodec;

const STREAM: &str = "Design/BulkStream.dat";
const OWNER_MESSAGE: &str = "Fusion Design parameter owner has an invalid frame or indexed link";
const COMPANION_MESSAGE: &str =
    "Fusion Design parameter companion has an invalid prefix or owner link";
const SCOPE_MESSAGE: &str = "Fusion Design parameter scope has an invalid paired frame";

fn decoded_constraints() -> (CadIr, SourceFidelity) {
    let source = include_bytes!("../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source[..]), &DecodeOptions::default())
        .expect("design fixture decodes");
    let (ir, _, fidelity) = decoded.into_parts();
    (ir, fidelity)
}

fn findings(ir: &CadIr, fidelity: &SourceFidelity) -> Vec<Finding> {
    crate::test_support::with_decode_context(|ctx| {
        super::validate_source_layout(ctx, ir, fidelity).expect("validate the source layout")
    })
}

/// The ids of the records of one native arena.
fn ids(ir: &CadIr, arena: &str) -> Vec<String> {
    ir.native
        .namespace("f3d")
        .expect("f3d native")
        .arenas()
        .get(arena)
        .into_iter()
        .flatten()
        .map(|record| record.id().to_owned())
        .collect()
}

/// The same sidecar with pointer `name` of record `id` replaced by `value`,
/// or removed when `value` is `None`.
fn with_pointer(
    fidelity: &SourceFidelity,
    id: &str,
    name: &str,
    value: Option<u64>,
) -> SourceFidelity {
    let mut wire = serde_json::to_value(fidelity).expect("serialize the sidecar");
    let pointers = wire["record_layout"][id]["pointers"]
        .as_object_mut()
        .unwrap_or_else(|| panic!("{id} has a record layout"));
    assert!(pointers.contains_key(name), "{id} holds pointer {name}");
    match value {
        Some(value) => pointers.insert(name.to_owned(), json!(value)),
        None => pointers.remove(name),
    };
    serde_json::from_value(wire).expect("the edited sidecar is admitted")
}

fn pointer(fidelity: &SourceFidelity, id: &str, name: &str) -> u64 {
    fidelity.record_layout()[id].pointers()[name]
}

fn only_finding(findings: &[Finding], message: &str, entity: &str) {
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].message, message);
    assert_eq!(findings[0].entity.as_deref(), Some(entity));
}

#[test]
fn the_decoded_sidecar_passes_every_source_layout_check() {
    let (ir, fidelity) = decoded_constraints();
    assert!(!ids(&ir, "design_parameter_owners").is_empty());
    assert!(!ids(&ir, "design_parameter_companions").is_empty());
    assert!(findings(&ir, &fidelity).is_empty());
}

#[test]
fn every_golden_fixture_sidecar_passes_every_source_layout_check() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");
    let mut paths: Vec<_> = std::fs::read_dir(&fixtures)
        .expect("fixture directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "f3d"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    let mut scopes = 0;
    for path in paths {
        let source = std::fs::read(&path).expect("read fixture");
        let Ok(decoded) = F3dCodec.decode(&mut Cursor::new(&source[..]), &DecodeOptions::default())
        else {
            continue;
        };
        let (ir, _, fidelity) = decoded.into_parts();
        scopes += ids(&ir, "design_parameter_scopes").len();
        let found = findings(&ir, &fidelity);
        assert!(found.is_empty(), "{}: {found:#?}", path.display());
    }
    assert!(scopes > 0);
}

#[test]
fn a_document_without_its_f3d_namespace_has_no_source_layout_finding() {
    let (_, fidelity) = decoded_constraints();
    assert!(findings(&CadIr::empty(), &fidelity).is_empty());
}

#[test]
fn a_companion_timestamp_away_from_prefix_offset_42_fails() {
    let (ir, fidelity) = decoded_constraints();
    for id in ids(&ir, "design_parameter_companions") {
        let moved = pointer(&fidelity, &id, "timestamp_micros_offset") + 1;
        let corrupted = with_pointer(&fidelity, &id, "timestamp_micros_offset", Some(moved));
        only_finding(&findings(&ir, &corrupted), COMPANION_MESSAGE, &id);
        let absent = with_pointer(&fidelity, &id, "timestamp_micros_offset", None);
        only_finding(&findings(&ir, &absent), COMPANION_MESSAGE, &id);
    }
}

#[test]
fn a_bound_companion_payload_away_from_the_prefix_end_fails() {
    let (ir, fidelity) = decoded_constraints();
    let companions = ids(&ir, "design_parameter_companions");
    let bound = companions
        .iter()
        .filter(|id| {
            fidelity.record_layout()[id.as_str()]
                .pointers()
                .contains_key("payload_byte_offset")
        })
        .collect::<Vec<_>>();
    assert!(!bound.is_empty());
    for id in bound {
        let moved = pointer(&fidelity, id, "payload_byte_offset") - 1;
        let corrupted = with_pointer(&fidelity, id, "payload_byte_offset", Some(moved));
        only_finding(&findings(&ir, &corrupted), COMPANION_MESSAGE, id);
        let absent = with_pointer(&fidelity, id, "payload_byte_offset", None);
        only_finding(&findings(&ir, &absent), COMPANION_MESSAGE, id);
    }
}

#[test]
fn a_companion_start_away_from_its_timestamp_fails() {
    let (ir, fidelity) = decoded_constraints();
    for id in ids(&ir, "design_parameter_companions") {
        let moved = pointer(&fidelity, &id, "byte_offset") + 1;
        let corrupted = with_pointer(&fidelity, &id, "byte_offset", Some(moved));
        only_finding(&findings(&ir, &corrupted), COMPANION_MESSAGE, &id);
    }
}

#[test]
fn a_typed_reference_companion_prefix_passes_and_ends_its_own_prefix() {
    use crate::layout::typed_reference_companion_prefix as typed;
    let (ir, fidelity) = decoded_constraints();
    for id in ids(&ir, "design_parameter_companions") {
        let start = pointer(&fidelity, &id, "byte_offset");
        let timestamp = start + u64::try_from(typed::TIMESTAMP_MICROS).unwrap();
        let typed_prefix = with_pointer(&fidelity, &id, "timestamp_micros_offset", Some(timestamp));
        if !fidelity.record_layout()[id.as_str()]
            .pointers()
            .contains_key("payload_byte_offset")
        {
            assert!(findings(&ir, &typed_prefix).is_empty());
            continue;
        }
        only_finding(&findings(&ir, &typed_prefix), COMPANION_MESSAGE, &id);
        let payload = start + u64::try_from(typed::LEN).unwrap();
        let typed_payload = with_pointer(&typed_prefix, &id, "payload_byte_offset", Some(payload));
        assert!(findings(&ir, &typed_payload).is_empty());
    }
}

/// Findings of the source-layout checks over an in-memory namespace.
fn loaded_findings(native: &F3dNative, fidelity: &SourceFidelity) -> Vec<Finding> {
    crate::test_support::with_decode_context(|decode| {
        let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
        ir.scope.design = true;
        let ctx = Ctx::new(&ir, native, decode);
        super::validate_loaded(&ctx, &Layouts { fidelity })
    })
}

fn owner(
    record_index: u32,
    byte_offset: u64,
    frame_length: u64,
    scope: u32,
) -> DesignParameterOwner {
    DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: crate::ids::native_design_parameter_owner_id(STREAM, byte_offset),
        byte_offset,
        frame_length,
        class_tag: "268".to_owned().try_into().unwrap(),
        record_index,
        scope_record_index: scope,
        local_ordinal: 0,
        evaluated_value: 6.0,
        evaluated_value_offset: byte_offset + 40,
        parameter_record_index: record_index + 1,
        owned_ordinal: 0,
        variant: None,
        companion_record_index: record_index + 2,
    })
    .unwrap()
}

fn parameter(record_index: u32, byte_offset: u64) -> DesignParameter {
    DesignParameter::try_from(DesignParameterDraft {
        id: crate::ids::native_design_parameter_id(STREAM, byte_offset),
        byte_offset,
        class_tag: "305".to_owned().try_into().unwrap(),
        record_index,
        source_ordinal: 0,
        source: DesignParameterSource::new("Feature Input".into(), Some(record_index - 1), None)
            .unwrap(),
        expression: "6 cm".into(),
        expression_offset: byte_offset + 12,
        source_kind_offset: byte_offset + 32,
        unit: None,
        name: "Length".into(),
        name_offset: byte_offset + 62,
        evaluated_value: 6.0,
        evaluated_value_offset: byte_offset + 72,
    })
    .unwrap()
}

/// A 68-byte legacy owner at 1000 and its parameter at 1068, whose
/// evaluated value is at 1140.
fn legacy_owner_namespace() -> (F3dNative, String, String) {
    let mut native = F3dNative::default();
    let owner = owner(100, 1_000, 68, 0);
    let parameter = parameter(101, 1_068);
    let ids = (owner.id().to_owned(), parameter.id.clone());
    native.design_parameter_owners.push(owner);
    native.design_parameters.push(parameter);
    (native, ids.0, ids.1)
}

#[test]
fn a_legacy_owner_takes_its_evaluated_value_from_its_parameter_layout() {
    let (native, owner, parameter) = legacy_owner_namespace();
    let fidelity = crate::test_support::source_layout::sidecar(
        STREAM,
        &[
            (&owner, 1_000, 68, &[("byte_offset", 1_000)]),
            (
                &parameter,
                1_068,
                149,
                &[("byte_offset", 1_068), ("evaluated_value_offset", 1_140)],
            ),
        ],
    );
    assert!(loaded_findings(&native, &fidelity).is_empty());
}

#[test]
fn a_legacy_owner_pointer_away_from_its_parameter_value_fails() {
    let (native, owner, parameter) = legacy_owner_namespace();
    let fidelity = crate::test_support::source_layout::sidecar(
        STREAM,
        &[
            (
                &owner,
                1_000,
                68,
                &[("byte_offset", 1_000), ("evaluated_value_offset", 1_141)],
            ),
            (
                &parameter,
                1_068,
                149,
                &[("byte_offset", 1_068), ("evaluated_value_offset", 1_140)],
            ),
        ],
    );
    only_finding(&loaded_findings(&native, &fidelity), OWNER_MESSAGE, &owner);
}

#[test]
fn a_legacy_owner_whose_parameter_has_no_value_pointer_fails() {
    let (native, owner, parameter) = legacy_owner_namespace();
    let fidelity = crate::test_support::source_layout::sidecar(
        STREAM,
        &[
            (&owner, 1_000, 68, &[("byte_offset", 1_000)]),
            (&parameter, 1_068, 149, &[("byte_offset", 1_068)]),
        ],
    );
    only_finding(&loaded_findings(&native, &fidelity), OWNER_MESSAGE, &owner);
}

#[test]
fn an_owner_without_a_record_layout_fails() {
    let (native, owner, parameter) = legacy_owner_namespace();
    let fidelity = crate::test_support::source_layout::sidecar(
        STREAM,
        &[(
            &parameter,
            1_068,
            149,
            &[("byte_offset", 1_068), ("evaluated_value_offset", 1_140)],
        )],
    );
    only_finding(&loaded_findings(&native, &fidelity), OWNER_MESSAGE, &owner);
}

/// Frame starts of the four 103-byte owners of scope 200, whose evaluated
/// values are 40 bytes into their frames.
const LANE_OWNER_STARTS: [u64; 4] = [1_000, 2_000, 3_000, 4_000];

fn lane_owners() -> Vec<DesignParameterOwner> {
    [10, 20, 30, 40]
        .into_iter()
        .zip(LANE_OWNER_STARTS)
        .map(|(record_index, byte_offset)| owner(record_index, byte_offset, 103, 200))
        .collect()
}

/// Frame start and length of the lane scope 200.
const LANE_SCOPE_START: u64 = 500;
const LANE_SCOPE_LENGTH: u64 = 200;

/// Scope 200 of `kind`, located at its decoded frame.
fn lane_scope(id: &str, kind: DesignFeatureKind) -> DesignParameterScope {
    DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: id.to_owned(),
            byte_offset: LANE_SCOPE_START,
            class_tag: "256".to_owned().try_into().unwrap(),
            record_index: 200,
            frame_length: LANE_SCOPE_LENGTH,
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: LANE_SCOPE_START + 20,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![10, 20, 30, 40],
                vec![0; 4],
                "reference_members",
            )
            .unwrap(),
            payload: kind.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: "256".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        }
        .with_fixture_layout(),
    )
    .expect("lane scope frame")
}

/// The lane owners' layouts and the capture of the scope that names them,
/// with owner `moved` holding its value pointer one byte away from its lane.
fn lane_sidecar(native: &F3dNative, moved: Option<usize>) -> SourceFidelity {
    let owners = &native.design_parameter_owners;
    let mut pointers = owners
        .iter()
        .zip(LANE_OWNER_STARTS)
        .enumerate()
        .map(|(ordinal, (owner, start))| {
            let value = start + 40 + u64::from(moved == Some(ordinal));
            (
                owner.id().to_owned(),
                start,
                103,
                vec![
                    ("byte_offset".to_owned(), start),
                    ("evaluated_value_offset".to_owned(), value),
                ],
            )
        })
        .collect::<Vec<_>>();
    let scope = &native.design_parameter_scopes[0];
    pointers.push((
        scope.id.clone(),
        LANE_SCOPE_START,
        LANE_SCOPE_LENGTH,
        crate::records::source_offsets::collect(&mut scope.clone())
            .into_iter()
            .collect(),
    ));
    let named = pointers
        .iter()
        .map(|(id, start, length, pointers)| {
            (
                id.as_str(),
                *start,
                *length,
                pointers
                    .iter()
                    .map(|(name, value)| (name.as_str(), *value))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    let layouts = named
        .iter()
        .map(|(id, start, length, pointers)| (*id, *start, *length, &pointers[..]))
        .collect::<Vec<_>>();
    crate::test_support::source_layout::sidecar(STREAM, &layouts)
}

fn rectangular_pattern_namespace() -> F3dNative {
    let mut construction =
        DesignRectangularPatternConstruction::try_from(DesignRectangularPatternConstructionWire {
            u_count: 2,
            v_count: 1,
            u_extent: 1.0,
            v_extent: 0.0,
            owner_record_indices: [10, 20, 30, 40],
            instances: None,
            instance_search_cap: None,
            directions: Vec::new(),
        })
        .unwrap();
    construction.value_offsets = Some([1_040, 2_040, 3_040, 4_040]);
    let mut scope = lane_scope(
        &format!("f3d:{STREAM}:pattern#200"),
        DesignFeatureKind::RectangularPattern,
    );
    if let DesignScopePayloadMut::RectangularPattern(slot) = scope.payload_mut() {
        *slot = Some(construction);
    }
    let mut native = F3dNative::default();
    native.design_parameter_owners = lane_owners();
    native.design_parameter_scopes.push(scope);
    native
}

fn alignment_namespace() -> F3dNative {
    let alignment = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        [10, 20, 30, 40]
            .into_iter()
            .zip([1_040, 2_040, 3_040, 4_040])
            .map(|(value, offset)| Sourced::at(value, offset))
            .collect(),
        None,
    )
    .unwrap();
    let mut scope = lane_scope(
        &format!("f3d:{STREAM}:assemble#200"),
        DesignFeatureKind::Assemble,
    );
    if let DesignScopePayloadMut::Assemble(slot) = scope.payload_mut() {
        *slot = Some(alignment);
    }
    let mut native = F3dNative::default();
    native.design_parameter_owners = lane_owners();
    native.design_parameter_scopes.push(scope);
    native
}

#[test]
fn rectangular_pattern_lanes_name_their_owner_values() {
    let native = rectangular_pattern_namespace();
    assert!(loaded_findings(&native, &lane_sidecar(&native, None)).is_empty());
    let scope = native.design_parameter_scopes[0].id.clone();
    for moved in 0..4 {
        only_finding(
            &loaded_findings(&native, &lane_sidecar(&native, Some(moved))),
            SCOPE_MESSAGE,
            &scope,
        );
    }
}

#[test]
fn assembly_alignment_lanes_name_their_owner_values() {
    let native = alignment_namespace();
    assert!(loaded_findings(&native, &lane_sidecar(&native, None)).is_empty());
    let scope = native.design_parameter_scopes[0].id.clone();
    for moved in 0..4 {
        only_finding(
            &loaded_findings(&native, &lane_sidecar(&native, Some(moved))),
            SCOPE_MESSAGE,
            &scope,
        );
    }
}

#[test]
fn a_lane_owner_without_its_value_pointer_fails() {
    let native = rectangular_pattern_namespace();
    let owners = &native.design_parameter_owners;
    let fidelity = with_pointer(
        &lane_sidecar(&native, None),
        owners[2].id(),
        "evaluated_value_offset",
        None,
    );
    only_finding(
        &loaded_findings(&native, &fidelity),
        SCOPE_MESSAGE,
        &native.design_parameter_scopes[0].id,
    );
}

/// `findings` names only `entity`, each with `message`, at least once.
fn findings_name_only(findings: &[Finding], message: &str, entity: &str) {
    assert!(!findings.is_empty());
    for finding in findings {
        assert_eq!(finding.message, message, "{findings:#?}");
        assert_eq!(finding.entity.as_deref(), Some(entity), "{findings:#?}");
    }
}

#[test]
fn a_scope_capture_without_a_lane_pointer_fails() {
    for (native, lane) in [
        (
            rectangular_pattern_namespace(),
            "rectangular_pattern_construction.value_offsets.1",
        ),
        (alignment_namespace(), "assembly_alignment.value_offsets.1"),
    ] {
        let scope = native.design_parameter_scopes[0].id.clone();
        let fidelity = with_pointer(&lane_sidecar(&native, None), &scope, lane, None);
        findings_name_only(&loaded_findings(&native, &fidelity), SCOPE_MESSAGE, &scope);
    }
}

#[test]
fn a_scope_capture_without_a_frame_pointer_fails() {
    for name in [
        "kind_offset",
        "feature_ordinal_offset",
        "paired_byte_offset",
    ] {
        let native = alignment_namespace();
        let scope = native.design_parameter_scopes[0].id.clone();
        let fidelity = with_pointer(&lane_sidecar(&native, None), &scope, name, None);
        findings_name_only(&loaded_findings(&native, &fidelity), SCOPE_MESSAGE, &scope);
    }
}

#[test]
fn a_scope_capture_with_a_pointer_the_scope_does_not_hold_fails() {
    let native = alignment_namespace();
    let scope = native.design_parameter_scopes[0].id.clone();
    let mut wire =
        serde_json::to_value(lane_sidecar(&native, None)).expect("serialize the sidecar");
    wire["record_layout"][&scope]["pointers"]["assembly_alignment.value_offsets.4"] = json!(5_040);
    let fidelity: SourceFidelity =
        serde_json::from_value(wire).expect("the edited sidecar is admitted");
    findings_name_only(&loaded_findings(&native, &fidelity), SCOPE_MESSAGE, &scope);
}
