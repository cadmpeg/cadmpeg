// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use crate::bytes::reference_fields::ReadField;
use crate::design::decode::scopes::point_data::POINT_DATA_TYPE_GUID;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::work_axes::{DesignWorkAxisConstruction, DesignWorkAxisSource};
use crate::records::feature::work_geometry::POINT_DIRECTION_TYPE_GUID;
use crate::test_support::indexed_header;
use cadmpeg_ir::design::construction::InputReference;
use cadmpeg_ir::design::names::{Base64Bytes, NativeNameValue};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::features::FeatureOperation;

const CARRIER: u32 = 40;
const FIRST_INPUT: u32 = 41;
const SECOND_INPUT: u32 = 42;
const RECIPE_GUID: &str = "5662F619-2281-4AD9-93DC-1FCFDA1062E7";

enum Input {
    Plain(u32),
    Typed(u32),
    CrossSegment(u32),
}

struct Carrier {
    class_tag: [u8; 3],
    leading: u8,
    version: u32,
    value_count: u32,
    values: [f64; 8],
    reference_type: u32,
    inputs: Vec<Input>,
}

impl Carrier {
    fn new(values: [f64; 8], reference_type: u32) -> Self {
        Self {
            class_tag: *b"421",
            leading: 0,
            version: 1,
            value_count: 8,
            values,
            reference_type,
            inputs: vec![Input::Plain(FIRST_INPUT), Input::Plain(SECOND_INPUT)],
        }
    }

    fn write(&self, bytes: &mut Vec<u8>) -> usize {
        let start = bytes.len();
        indexed_header(bytes, self.class_tag, CARRIER);
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[self.leading, 0]);
        bytes.extend_from_slice(&self.value_count.to_le_bytes());
        for value in self.values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.reference_type.to_le_bytes());
        if self.version == 4 {
            bytes.push(0);
            for value in [-1.0_f64; 3] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(
            &u32::try_from(self.inputs.len())
                .expect("fixture input count fits u32")
                .to_le_bytes(),
        );
        for input in &self.inputs {
            bytes.push(1);
            match input {
                Input::Plain(target) => {
                    bytes.extend_from_slice(&u64::from(*target).to_le_bytes());
                }
                Input::Typed(target) => {
                    bytes.extend_from_slice(&u64::from(*target).to_le_bytes());
                    bytes.extend_from_slice(&36u32.to_le_bytes());
                    bytes.extend_from_slice(RECIPE_GUID.as_bytes());
                }
                Input::CrossSegment(target) => {
                    bytes.extend_from_slice(&u64::from(*target).to_le_bytes());
                    bytes.extend_from_slice(&[0, 1]);
                    bytes.extend_from_slice(&7u32.to_le_bytes());
                    continue;
                }
            }
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes.extend_from_slice(&[0, 0, 1, 0, 0, 0]);
        indexed_header(bytes, *b"260", CARRIER);
        start
    }
}

fn write_point_data(bytes: &mut Vec<u8>, record_index: u32, point: [f64; 3]) -> usize {
    indexed_header(bytes, *b"433", record_index);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&[0; 16]);
    bytes.push(0);
    let point_at = bytes.len();
    for value in point {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&10u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&77u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    indexed_header(bytes, *b"260", record_index);
    point_at
}

fn work_axis_scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:test/BulkStream.dat:work-axis#1",
        DesignFeatureKind::WorkAxis,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                CARRIER,
                FIRST_INPUT,
                SECOND_INPUT,
            ]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn carrier_types(version: u32) -> HashMap<u64, (&'static str, u32)> {
    HashMap::from([(u64::from(CARRIER), (POINT_DIRECTION_TYPE_GUID, version))])
}

struct ReadAxis {
    evidence: DesignWorkAxisConstruction,
    ir: cadmpeg_ir::CadIr,
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    fidelity: cadmpeg_ir::SourceFidelity,
}

impl ReadAxis {
    fn axis(&self) -> Option<&cadmpeg_ir::features::datums::DatumAxis> {
        match self.ir.model.features[0]
            .evaluation
            .definition()
            .operation()
        {
            FeatureOperation::DatumAxis { axis } => Some(axis),
            _ => None,
        }
    }
    fn has_loss(&self, code: F3dLossCode) -> bool {
        self.losses.iter().any(|loss| loss.code == code.kind())
    }
}

fn read(bytes: &[u8], types: &HashMap<u64, (&str, u32)>) -> ReadAxis {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, source) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut scope = work_axis_scope();
    let mut parts = Vec::new();
    let carrier_version = types.get(&u64::from(CARRIER)).map_or(1, |(_, v)| *v);
    let point_version = types.get(&u64::from(FIRST_INPUT)).map_or(1, |(_, v)| *v);
    let classes = HashMap::from([
        (421, (POINT_DIRECTION_TYPE_GUID, carrier_version)),
        (443, (POINT_DIRECTION_TYPE_GUID, carrier_version)),
        (433, (POINT_DATA_TYPE_GUID, point_version)),
    ]);
    let evidence = super::super::work_axes::decode_work_axis(
        &ctx,
        source,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        &scope,
        "test/BulkStream.dat",
        super::super::work_axes::WorkAxisRegistrations {
            entities: types,
            classes: &classes,
        },
        &mut parts,
    )
    .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkAxis(slot) =
        scope.payload_mut()
    {
        *slot = Some(evidence.clone());
    }
    let scopes = std::slice::from_ref(&scope);
    let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
    let projected = crate::design::feature_project::project_parameter_design_with_edge_identities(
        &ctx,
        &crate::design::feature_project::ProjectInputs {
            scopes,
            timelines: &timelines,
            ..Default::default()
        },
    )
    .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features = projected.features;
    let mut native = crate::native::F3dNative::default();
    native.design_parameter_scopes.push(scope);
    native.store(&ctx, ir.native.namespace_mut("f3d")).unwrap();
    let mut fidelity = cadmpeg_ir::SourceFidelity::for_root(source);
    let mut losses = projected.projection_losses;
    for part in parts {
        losses.push(part.commit(&ctx, &mut ir, &mut fidelity).unwrap());
    }
    ctx.finish_session().unwrap();
    ReadAxis {
        evidence,
        ir,
        losses,
        fidelity,
    }
}

fn unassigned_rule_values() -> [f64; 8] {
    [1.5, -2.0, 3.25, 0.6, 0.0, 0.8, 0.0, 0.0]
}

#[test]
fn point_direction_carrier_supplies_the_solved_axis_of_an_unassigned_rule() {
    for class_tag in [*b"421", *b"443"] {
        let mut bytes = Vec::new();
        Carrier {
            class_tag,
            ..Carrier::new(unassigned_rule_values(), 18)
        }
        .write(&mut bytes);
        let read = read(&bytes, &carrier_types(1));
        let axis = read.axis().unwrap();
        let origin = axis.origin().unwrap().get();
        assert!(
            (origin.x - 15.0).abs() < 1.0e-12
                && (origin.y + 20.0).abs() < 1.0e-12
                && (origin.z - 32.5).abs() < 1.0e-12
        );
        assert!(read.has_loss(F3dLossCode::FeatureWorkAxisRuleUnassigned));
        assert_eq!(axis.inputs().len(), 2);
        assert!(axis.inputs().iter().all(|input| input.role.is_none()));
        let source = &read.evidence.candidates[0].source;
        assert!(
            matches!(source, Some(DesignWorkAxisSource::PointDirection {reference_type: ReadField::Value(18), inputs, ..}) if inputs.references.len() == 2 && inputs.references[0].local().unwrap().0 == u64::from(FIRST_INPUT))
        );
    }
}

#[test]
fn work_axis_nonzero_leading_block_keeps_the_old_line_with_a_layout_assumption() {
    for leading in [1, 255] {
        let mut bytes = Vec::new();
        Carrier {
            leading,
            ..Carrier::new([0.0, 0.0, 2.0, 0.0, 0.0, 1.0, 0.0, 0.0], 19)
        }
        .write(&mut bytes);
        let result = read(&bytes, &carrier_types(1));
        let axis = result.axis().unwrap();
        assert!((axis.origin().unwrap().get().z - 20.0).abs() < 1.0e-12);
        assert!((axis.direction().unwrap().get().z - 1.0).abs() < 1.0e-12);
        assert_eq!(
            result.evidence.candidates[0].prefix.leading_block,
            ReadField::Value(leading)
        );
        assert!(result
            .losses
            .iter()
            .any(|loss| loss.code.to_string()
                == "f3d/feature.work-axis-leading-block-layout-assumed"));
        assert_eq!(result.ir.native_unknowns("f3d").unwrap().len(), 1);
    }
}

#[test]
fn work_axis_missing_competing_carrier_does_not_establish_unique_geometry() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 18).write(&mut bytes);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DIRECTION_TYPE_GUID, 1));
    let result = read(&bytes, &types);
    assert!(result.axis().is_none());
    assert_eq!(result.evidence.candidates.len(), 2);
    let missing = &result.evidence.candidates[1];
    assert_eq!(missing.record_index, Some(FIRST_INPUT));
    assert_eq!(missing.version, ReadField::Value(1));
    assert!(missing.source.is_none());
    assert_eq!(missing.origin, [ReadField::Unread; 3]);
    assert_eq!(missing.displacement, [ReadField::Unread; 3]);
}

#[test]
fn work_axis_unregistered_or_unsupported_carrier_keeps_metadata_without_geometry() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 18).write(&mut bytes);
    let result = read(&bytes, &HashMap::new());
    assert!(result.axis().is_none());
    assert_eq!(result.evidence.candidates[0].version, ReadField::Value(1));
    assert!(result.evidence.candidates[0].source.is_none());
    let result = read(&bytes, &carrier_types(99));
    assert!(result.axis().is_none());
    assert_eq!(result.evidence.candidates.len(), 1);
    assert_eq!(result.evidence.candidates[0].version, ReadField::Value(99));
    assert!(result.evidence.candidates[0].source.is_none());
}

#[test]
fn point_direction_carrier_projects_a_datum_axis() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 16).write(&mut bytes);
    let read = read(&bytes, &carrier_types(1));
    let direction = read.axis().unwrap().direction().unwrap().get();
    for (actual, expected) in [direction.x, direction.y, direction.z]
        .into_iter()
        .zip([0.6, 0.0, 0.8])
    {
        assert!((actual - expected).abs() < 1.0e-12);
    }
}

#[test]
fn point_direction_carrier_reads_version_four_and_typed_inputs() {
    let mut bytes = Vec::new();
    Carrier {
        version: 4,
        inputs: vec![Input::Typed(FIRST_INPUT)],
        ..Carrier::new(unassigned_rule_values(), 6)
    }
    .write(&mut bytes);
    let read = read(&bytes, &carrier_types(4));
    assert!(read.axis().is_some());
    assert!(
        matches!(&read.evidence.candidates[0].source, Some(DesignWorkAxisSource::PointDirection {tangent_prefix: ReadField::Value(0), inputs, ..}) if inputs.references[0].inline_type_guid.value().map(String::as_str) == Some(RECIPE_GUID))
    );
}

#[test]
fn nonzero_parameter_version_four_retains_every_source_value_and_projects_the_stored_line() {
    let mut bytes = Vec::new();
    Carrier {
        version: 4,
        inputs: vec![Input::Plain(FIRST_INPUT)],
        ..Carrier::new([0.0, 0.0, 2.0, 0.0, 0.0, 1.0, -1.5, -1.0], 19)
    }
    .write(&mut bytes);
    let read = read(&bytes, &carrier_types(4));
    let axis = read.axis().unwrap();
    assert!((axis.origin().unwrap().get().z - 20.0).abs() < 1.0e-12);
    assert!((axis.direction().unwrap().get().z - 1.0).abs() < 1.0e-12);
    assert!(read.has_loss(F3dLossCode::FeatureWorkAxisNonzeroParametersSolvedAssumed));
    assert!(read.has_loss(F3dLossCode::FeatureWorkAxisRuleUnassigned));
    assert!(
        matches!(&read.evidence.candidates[0].source, Some(DesignWorkAxisSource::PointDirection {point_parameters, tangent_prefix: ReadField::Value(0), tangent, inputs, ..}) if point_parameters[0].value().is_some_and(|v| (v.get() + 1.5).abs() < 1.0e-12) && tangent.iter().all(|v| v.value().is_some_and(|v| (v.get() + 1.0).abs() < 1.0e-12)) && inputs.references.len() == 1)
    );
}

#[test]
fn two_point_rule_reads_the_point_data_endpoints() {
    let mut bytes = Vec::new();
    Carrier::new([2.0, 1.0, -4.0, 0.0, -0.4, 0.0, 0.0, 0.0], 12).write(&mut bytes);
    write_point_data(&mut bytes, FIRST_INPUT, [2.0, 1.0, -4.0]);
    write_point_data(&mut bytes, SECOND_INPUT, [2.0, 0.6, -4.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    types.insert(u64::from(SECOND_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let read = read(&bytes, &types);
    let axis = read.axis().unwrap();
    assert!(axis.inputs().iter().all(|input| matches!(
        &input.role,
        Some(cadmpeg_ir::design::construction::InputRole {
            subject: cadmpeg_ir::design::construction::Subject::Whole,
            relation: cadmpeg_ir::design::construction::Relation::Coincident
        })
    )));
    assert!(read.evidence.candidates[0]
        .endpoints
        .iter()
        .all(|endpoint| endpoint.readings.len() == 1));
    assert!(matches!(
        read.evidence.candidates[0].source,
        Some(DesignWorkAxisSource::PointDirection {
            reference_type: ReadField::Value(12),
            ..
        })
    ));
}

#[test]
fn qualified_axis_input_keeps_the_solved_axis_and_is_named_by_its_stored_reference() {
    for reference_type in [12, 18] {
        let mut bytes = Vec::new();
        Carrier {
            inputs: vec![Input::Plain(FIRST_INPUT), Input::CrossSegment(SECOND_INPUT)],
            ..Carrier::new([2.0, 1.0, -4.0, 0.0, -0.4, 0.0, 0.0, 0.0], reference_type)
        }
        .write(&mut bytes);
        write_point_data(&mut bytes, FIRST_INPUT, [2.0, 1.0, -4.0]);
        write_point_data(&mut bytes, SECOND_INPUT, [2.0, 0.6, -4.0]);
        let mut types = carrier_types(1);
        types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
        types.insert(u64::from(SECOND_INPUT), (POINT_DATA_TYPE_GUID, 1));
        let read = read(&bytes, &types);
        let axis = read.axis().expect("solved axis with qualified input");
        let direction = axis.direction().unwrap().get();
        for (actual, expected) in [direction.x, direction.y, direction.z]
            .into_iter()
            .zip([0.0, -1.0, 0.0])
        {
            assert!((actual - expected).abs() < 1.0e-12);
        }
        let [local, qualified_input] = axis.inputs() else {
            panic!("two stated inputs");
        };
        assert!(local.reference.is_some() && local.role.is_none());
        assert!(qualified_input.role.is_none());
        let Some(DesignWorkAxisSource::PointDirection { inputs, .. }) =
            &read.evidence.candidates[0].source
        else {
            panic!("point and direction carrier");
        };
        let qualified = &inputs.references[1];
        assert_eq!(qualified.target, ReadField::Value(u64::from(SECOND_INPUT)));
        assert_eq!(qualified.segment, ReadField::Value(7));
        let Some(InputReference::Picks(selection)) = &qualified_input.reference else {
            panic!("a selection");
        };
        let Picks::Named(picks) = &selection.picks else {
            panic!("named picks");
        };
        let [pick] = picks.as_slice() else {
            panic!("one pick");
        };
        assert!(matches!(pick.target, PickTarget::Native { within: None }));
        assert!(pick.witness.is_none());
        let mut stored = vec![1];
        stored.extend_from_slice(&u64::from(SECOND_INPUT).to_le_bytes());
        stored.extend_from_slice(&[0, 1]);
        stored.extend_from_slice(&7u32.to_le_bytes());
        assert_eq!(
            pick.native.as_ref().map(|name| &name.value),
            Some(&NativeNameValue::Bytes(Base64Bytes(stored)))
        );
    }
}

#[test]
fn two_point_rule_within_the_endpoint_tolerance_reports_the_assumption() {
    let mut bytes = Vec::new();
    Carrier::new([2.0, 1.0, -4.0, 0.0, -0.5, 0.0, 0.0, 0.0], 12).write(&mut bytes);
    write_point_data(&mut bytes, FIRST_INPUT, [2.0 + 5.0e-10, 1.0, -4.0]);
    write_point_data(&mut bytes, SECOND_INPUT, [2.0, 0.5, -4.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    types.insert(u64::from(SECOND_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let read = read(&bytes, &types);
    assert!(read.has_loss(F3dLossCode::FeatureWorkAxisEndpointToleranceAssumed));
    assert!(read
        .axis()
        .unwrap()
        .inputs()
        .iter()
        .all(|input| input.role.is_some()));
}

#[test]
fn two_point_rule_with_a_displaced_endpoint_retains_points_without_asserting_the_rule() {
    let mut bytes = Vec::new();
    Carrier::new([2.0, 1.0, -4.0, 0.0, -0.4, 0.0, 0.0, 0.0], 12).write(&mut bytes);
    write_point_data(&mut bytes, FIRST_INPUT, [2.0, 1.0, -4.0]);
    write_point_data(&mut bytes, SECOND_INPUT, [2.0, 0.5, -4.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    types.insert(u64::from(SECOND_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let read = read(&bytes, &types);
    assert!(read.has_loss(F3dLossCode::FeatureWorkAxisTwoPointConflictSolvedAssumed));
    assert!(read
        .axis()
        .unwrap()
        .inputs()
        .iter()
        .all(|input| input.role.is_none() && input.reference.is_some()));
}

#[test]
fn point_direction_work_axis_requires_the_registered_carrier_type() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 18).write(&mut bytes);
    for types in [
        HashMap::new(),
        HashMap::from([(u64::from(CARRIER), (POINT_DATA_TYPE_GUID, 1))]),
        carrier_types(2),
    ] {
        let read = read(&bytes, &types);
        assert!(read.axis().is_none());
        assert!(matches!(
            read.ir.model.features[0]
                .evaluation
                .definition()
                .operation(),
            FeatureOperation::Unresolved {
                family: cadmpeg_ir::features::UnresolvedFamily::DatumAxis
            }
        ));
    }
}

#[test]
fn an_axis_states_its_stored_line_whole_or_not_at_all() {
    for (values, whole) in [
        ([1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0], false),
        ([f64::MAX, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0], false),
        ([1.0, 2.0, 3.0, f64::MAX, f64::MAX, 0.0, 0.0, 0.0], true),
    ] {
        let mut bytes = Vec::new();
        Carrier::new(values, 19).write(&mut bytes);
        let read = read(&bytes, &carrier_types(1));
        let axis = read.axis().unwrap();
        assert_eq!(axis.origin().is_some(), whole);
        assert_eq!(axis.direction().is_some(), whole);
        assert_eq!(
            read.has_loss(F3dLossCode::FeatureWorkAxisGeometryUnrepresentable),
            !whole
        );
        assert_eq!(
            read.evidence.candidates[0].origin[0]
                .value()
                .unwrap()
                .get()
                .to_bits(),
            values[0].to_bits()
        );
    }
}

#[test]
fn malformed_scalar_retains_later_fields_and_commits_one_real_unknown_part() {
    let mut bytes = Vec::new();
    Carrier::new([1.0, 2.0, 3.0, f64::NAN, 0.0, 1.0, -1.5, -1.0], 19).write(&mut bytes);
    let read = read(&bytes, &carrier_types(1));
    assert!(read.axis().unwrap().origin().is_none());
    assert!(read.axis().unwrap().direction().is_none());
    assert!(read.has_loss(F3dLossCode::FeatureWorkAxisPartialLineNative));
    assert_eq!(
        read.evidence.candidates[0]
            .origin
            .map(|field| field.value().map(|value| value.get())),
        [Some(1.0), Some(2.0), Some(3.0)],
        "the origin read stays in the scope's native record"
    );
    assert!(
        matches!(&read.evidence.candidates[0].source, Some(DesignWorkAxisSource::PointDirection {point_parameters, reference_type: ReadField::Value(19), ..}) if point_parameters[0].value().is_some_and(|value| (value.get() + 1.5).abs() < 1.0e-12))
    );
    assert_eq!(read.ir.native_unknowns("f3d").unwrap().len(), 1);
    assert_eq!(read.fidelity.retained_records().len(), 1);
    let stubs = read.ir.native_unknowns("f3d").unwrap();
    assert_eq!(
        stubs[0]
            .links
            .iter()
            .map(cadmpeg_ir::ids::Identity::as_str)
            .collect::<Vec<_>>(),
        vec![work_axis_scope().id.as_str()]
    );
    let retained = read.fidelity.retained_record(stubs[0].id.as_str()).unwrap();
    let nan = f64::NAN.to_le_bytes();
    let at = bytes.windows(8).position(|part| part == nan).unwrap();
    assert_eq!(retained.offset(), u64::try_from(at).unwrap());
    assert_eq!(retained.byte_len(), 8);
    assert_eq!(retained.stream(), "test/BulkStream.dat");
    assert_eq!(retained.data(), Some(nan.as_slice()));
    assert!(retained
        .untyped()
        .contains(&cadmpeg_core::decode::ArenaClass::Design));

    assert_eq!(
        read.losses
            .iter()
            .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
            .count(),
        1
    );
}

#[test]
fn native_properties_keep_blank_duplicate_keys_and_unsigned_real_lanes() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 18).write(&mut bytes);
    let mut properties = vec![0, 1];
    properties.extend_from_slice(&3u32.to_le_bytes());
    for (key, kind, value) in [
        ("", "IntrinsicMetaTypeuint64", u64::MAX.to_le_bytes()),
        ("same", "IntrinsicMetaTypedouble", (-2.5_f64).to_le_bytes()),
        ("same", "IntrinsicMetaTypeuint64", 7u64.to_le_bytes()),
    ] {
        crate::test_support::lp_ascii(&mut properties, key);
        crate::test_support::lp_ascii(&mut properties, kind);
        properties.extend_from_slice(&value);
    }
    bytes.splice(19..21, properties);
    let read = read(&bytes, &carrier_types(1));
    let properties = &read.evidence.candidates[0].prefix.properties;
    assert_eq!(properties.len(), 3);
    assert_eq!(properties[0].key.value().unwrap(), "");
    assert!(matches!(
        properties[0].value,
        ReadField::Value(
            crate::records::feature::work_axes::AxisCarrierPropertyValue::Unsigned(u64::MAX)
        )
    ));
    assert_eq!(properties[1].key, properties[2].key);
    assert!(
        matches!(properties[1].value,ReadField::Value(crate::records::feature::work_axes::AxisCarrierPropertyValue::Real(v)) if v.get()==-2.5)
    );
    assert!(read.axis().unwrap().geometry().is_some());
}

#[test]
fn repeated_operands_remain_separate_and_tiny_directions_keep_the_parent_boundary() {
    let mut bytes = Vec::new();
    Carrier {
        inputs: vec![Input::Plain(FIRST_INPUT), Input::Plain(FIRST_INPUT)],
        ..Carrier::new([0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0], 12)
    }
    .write(&mut bytes);
    write_point_data(&mut bytes, FIRST_INPUT, [0.0, 0.0, 0.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let result = read(&bytes, &types);
    assert_eq!(result.axis().unwrap().inputs().len(), 2);
    assert!(result
        .axis()
        .unwrap()
        .inputs()
        .iter()
        .all(|v| v.role.is_none() && v.reference.is_some()));
    let mut bytes = Vec::new();
    Carrier::new([1.0, 2.0, 3.0, 1e-300, 0.0, 0.0, 0.0, 0.0], 19).write(&mut bytes);
    let read = read(&bytes, &carrier_types(1));
    assert!(read.axis().unwrap().origin().is_none());
    assert!(read.axis().unwrap().direction().is_none());
    assert!(read.has_loss(F3dLossCode::FeatureWorkAxisGeometryUnrepresentable));
    assert_eq!(
        read.evidence.candidates[0].displacement[0]
            .value()
            .unwrap()
            .get(),
        1e-300
    );
}

#[test]
fn repeated_damaged_operands_keep_occurrences_without_duplicate_source_parts() {
    let mut bytes = Vec::new();
    Carrier {
        inputs: vec![Input::Plain(FIRST_INPUT), Input::Plain(FIRST_INPUT)],
        ..Carrier::new([0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0], 12)
    }
    .write(&mut bytes);
    let point_at = write_point_data(&mut bytes, FIRST_INPUT, [f64::NAN, 2.0, 3.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let result = read(&bytes, &types);
    assert_eq!(result.axis().unwrap().inputs().len(), 2);
    assert_eq!(result.evidence.candidates[0].endpoints.len(), 1);
    assert_eq!(
        result.evidence.candidates[0].endpoints[0].readings[0].position[1]
            .value()
            .unwrap()
            .get(),
        2.0
    );
    let stubs = result.ir.native_unknowns("f3d").unwrap();
    assert_eq!(stubs.len(), 1);
    let retained = result
        .fidelity
        .retained_record(stubs[0].id.as_str())
        .unwrap();
    assert_eq!(retained.data(), Some(f64::NAN.to_le_bytes().as_slice()));
    assert_eq!(retained.offset(), u64::try_from(point_at).unwrap());
}

#[test]
fn shared_damaged_endpoint_keeps_both_native_scope_owners() {
    let mut bytes = Vec::new();
    Carrier {
        inputs: vec![Input::Plain(FIRST_INPUT)],
        ..Carrier::new(unassigned_rule_values(), 19)
    }
    .write(&mut bytes);
    write_point_data(&mut bytes, FIRST_INPUT, [f64::NAN, 2.0, 3.0]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, source) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let classes = HashMap::from([
        (421, (POINT_DIRECTION_TYPE_GUID, 1)),
        (433, (POINT_DATA_TYPE_GUID, 1)),
    ]);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let mut parts = Vec::new();
    let first = work_axis_scope();
    let mut second = first.clone();
    second.id = "f3d:test/BulkStream.dat:work-axis#2".into();
    second.record_index = 2;
    for scope in [&first, &second] {
        super::super::work_axes::decode_work_axis(
            &ctx,
            source,
            &records,
            scope,
            "test/BulkStream.dat",
            super::super::work_axes::WorkAxisRegistrations {
                entities: &types,
                classes: &classes,
            },
            &mut parts,
        )
        .unwrap();
    }
    assert_eq!(parts.len(), 2);
    let parts = cadmpeg_ir::unknown::part::GivenUpPart::coalesce(parts).unwrap();
    assert_eq!(parts.len(), 1);
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut fidelity = cadmpeg_ir::SourceFidelity::for_root(source);
    parts
        .into_iter()
        .next()
        .unwrap()
        .commit(&ctx, &mut ir, &mut fidelity)
        .unwrap();
    let stubs = ir.native_unknowns("f3d").unwrap();
    assert_eq!(
        stubs[0]
            .links
            .iter()
            .map(cadmpeg_ir::ids::Identity::as_str)
            .collect::<Vec<_>>(),
        vec![first.id.as_str(), second.id.as_str()]
    );
    ctx.finish_session().unwrap();
}

#[test]
fn class_registered_endpoint_versions_never_use_an_older_layout_by_analogy() {
    for (version, x) in [(1, 1.0), (99, 1.0), (1, f64::NAN)] {
        let mut bytes = Vec::new();
        Carrier {
            inputs: vec![Input::Plain(FIRST_INPUT)],
            ..Carrier::new(unassigned_rule_values(), 19)
        }
        .write(&mut bytes);
        write_point_data(&mut bytes, FIRST_INPUT, [x, 2.0, 3.0]);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, source) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let types = carrier_types(1);
        let classes = HashMap::from([
            (421, (POINT_DIRECTION_TYPE_GUID, 1)),
            (433, (POINT_DATA_TYPE_GUID, version)),
        ]);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let mut parts = Vec::new();
        let evidence = super::super::work_axes::decode_work_axis(
            &ctx,
            source,
            &records,
            &work_axis_scope(),
            "test/BulkStream.dat",
            super::super::work_axes::WorkAxisRegistrations {
                entities: &types,
                classes: &classes,
            },
            &mut parts,
        )
        .unwrap();
        let readings = &evidence.candidates[0].endpoints[0].readings;
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].version, ReadField::Value(version));
        if version == 99 {
            assert!(readings[0]
                .position
                .iter()
                .all(|value| value == &ReadField::Unread));
            assert!(parts.is_empty());
        } else {
            assert_eq!(readings[0].position[1].value().unwrap().get(), 2.0);
            assert_eq!(parts.len(), usize::from(x.is_nan()));
            assert_eq!(readings[0].complete, !x.is_nan());
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn truncated_reference_keeps_path_prefix_and_an_empty_missing_count_is_reported() {
    let mut bytes = Vec::new();
    Carrier {
        inputs: vec![Input::Plain(FIRST_INPUT)],
        ..Carrier::new(unassigned_rule_values(), 19)
    }
    .write(&mut bytes);
    let count_at = 11 + 4 + 4 + 2 + 4 + 8 * 8 + 4;
    let paired = bytes.split_off(bytes.len() - 11);
    bytes.truncate(count_at + 4);
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(FIRST_INPUT).to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&paired);
    let result = read(&bytes, &carrier_types(1));
    assert!(result.axis().unwrap().geometry().is_some());
    let Some(DesignWorkAxisSource::PointDirection { inputs, .. }) =
        &result.evidence.candidates[0].source
    else {
        panic!("point-direction evidence")
    };
    assert_eq!(inputs.references.len(), 1);
    assert!(!inputs.complete());
    assert_eq!(
        inputs.references[0].target,
        ReadField::Value(u64::from(FIRST_INPUT))
    );
    assert_eq!(inputs.references[0].cross_document, ReadField::Value(1));
    assert_eq!(inputs.references[0].segment, ReadField::Unread);
    assert_eq!(result.ir.native_unknowns("f3d").unwrap().len(), 1);
    bytes.truncate(count_at);
    bytes.extend_from_slice(&paired);
    let result = read(&bytes, &carrier_types(1));
    assert!(result.axis().unwrap().geometry().is_some());
    let records = result.fidelity.retained_records();
    assert_eq!(records.len(), 1);
    let record = records.values().next().unwrap();
    assert_eq!(record.offset(), u64::try_from(count_at).unwrap());
    assert_eq!(record.byte_len(), 0);
    assert_eq!(
        result
            .losses
            .iter()
            .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
            .count(),
        1
    );
}

#[test]
fn work_axis_damaged_counts_keep_neutral_input_occurrences_and_native_fields() {
    for (declared, reference) in [
        (2u32, vec![0]),
        (u32::MAX, {
            let mut reference = vec![1];
            reference.extend_from_slice(&u64::from(FIRST_INPUT).to_le_bytes());
            reference.extend_from_slice(&[0, 0]);
            reference
        }),
    ] {
        let mut bytes = Vec::new();
        Carrier::new(unassigned_rule_values(), 19).write(&mut bytes);
        let paired = bytes.split_off(bytes.len() - 11);
        let count_at = 11 + 4 + 4 + 2 + 4 + 8 * 8 + 4;
        bytes.truncate(count_at);
        bytes.extend_from_slice(&declared.to_le_bytes());
        bytes.extend_from_slice(&reference);
        bytes.extend_from_slice(&paired);
        let result = read(&bytes, &carrier_types(1));
        assert!(result.axis().unwrap().geometry().is_some());
        let neutral = result.axis().unwrap().inputs();
        assert_eq!(neutral.len(), usize::from(reference[0]) + 1);
        assert!(matches!(
            neutral.last().and_then(|input| input.reference.as_ref()),
            Some(InputReference::Picks(selection)) if matches!(selection.picks, Picks::Unread)
        ));
        let Some(DesignWorkAxisSource::PointDirection { inputs, .. }) =
            &result.evidence.candidates[0].source
        else {
            panic!("point direction")
        };
        assert_eq!(inputs.declared, ReadField::Value(declared));
        assert_eq!(inputs.references.len(), 1);
        assert_eq!(inputs.references[0].present, ReadField::Value(reference[0]));
        assert!(!inputs.complete());
        assert!(result.has_loss(F3dLossCode::FeatureWorkAxisPartialCarrierSolvedAssumed));
        assert_eq!(result.ir.native_unknowns("f3d").unwrap().len(), 1);
    }
}
