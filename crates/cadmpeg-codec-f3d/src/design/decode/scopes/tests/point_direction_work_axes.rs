// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use crate::design::decode::scopes::point_data::POINT_DATA_TYPE_GUID;
use crate::design::decode::scopes::point_direction::POINT_DIRECTION_TYPE_GUID;
use crate::design::decode::scopes::work_geometry::exact_work_axis_construction;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::feature::work_geometry::{DesignWorkAxisConstruction, DesignWorkAxisSource};
use crate::test_support::indexed_header;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};

const CARRIER: u32 = 40;
const FIRST_INPUT: u32 = 41;
const SECOND_INPUT: u32 = 42;
const RECIPE_GUID: &str = "5662F619-2281-4AD9-93DC-1FCFDA1062E7";

enum Input {
    Plain(u32),
    Typed(u32),
}

struct Carrier {
    class_tag: [u8; 3],
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
        bytes.extend_from_slice(&[0, 0]);
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

fn read(bytes: &[u8], types: &HashMap<u64, (&str, u32)>) -> Option<DesignWorkAxisConstruction> {
    exact_work_axis_construction(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        &work_axis_scope(),
        types,
    )
}

fn unassigned_rule_values() -> [f64; 8] {
    [1.5, -2.0, 3.25, 0.6, 0.0, 0.8, 0.0, 0.0]
}

#[test]
fn point_direction_carrier_supplies_the_solved_axis_of_an_unassigned_rule() {
    for class_tag in [*b"421", *b"443"] {
        let mut bytes = Vec::new();
        let start = Carrier {
            class_tag,
            ..Carrier::new(unassigned_rule_values(), 18)
        }
        .write(&mut bytes);
        let construction = read(&bytes, &carrier_types(1)).expect("point-and-direction carrier");
        assert_eq!(
            construction.origin.map(cadmpeg_ir::scalar::FiniteReal::get),
            [1.5, -2.0, 3.25]
        );
        assert_eq!(
            construction
                .displacement
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            [0.6, 0.0, 0.8]
        );
        assert_eq!(
            construction.origin_offset,
            u64::try_from(start + 25).unwrap()
        );
        assert_eq!(
            construction.displacement_offset,
            u64::try_from(start + 49).unwrap()
        );
        assert_eq!(
            construction.source,
            Some(DesignWorkAxisSource::PointDirection {
                carrier_record_index: CARRIER,
                reference_type: 18,
                input_record_indices: vec![FIRST_INPUT, SECOND_INPUT],
            })
        );
    }
}

#[test]
fn point_direction_carrier_projects_a_datum_axis() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 16).write(&mut bytes);
    let mut scope = work_axis_scope();
    let construction = exact_work_axis_construction(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &carrier_types(1),
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::WorkAxis(slot) =
        scope.payload_mut()
    {
        *slot = construction;
    }
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            crate::design::feature_project::project_parameter_design_with_edge_identities(
                ctx,
                &crate::design::feature_project::ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    ..Default::default()
                },
            )
            .expect("test projection has a synthetic exact timeline")
        });
    let [Feature { evaluation, .. }] = features.as_slice() else {
        panic!("one projected feature");
    };
    let FeatureDefinition::Operation(FeatureOperation::DatumAxis { origin, direction }) =
        evaluation.definition()
    else {
        panic!("datum axis");
    };
    let origin = origin.get();
    assert_eq!([origin.x, origin.y, origin.z], [15.0, -20.0, 32.5]);
    let direction = direction.get();
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
    let construction = read(&bytes, &carrier_types(4)).expect("version-4 carrier");
    assert_eq!(
        construction.source,
        Some(DesignWorkAxisSource::PointDirection {
            carrier_record_index: CARRIER,
            reference_type: 6,
            input_record_indices: vec![FIRST_INPUT],
        })
    );
    assert!(read(&bytes, &carrier_types(1)).is_none());
}

#[test]
fn two_point_rule_reads_the_point_data_endpoints() {
    let mut bytes = Vec::new();
    Carrier::new([2.0, 1.0, -4.0, 0.0, -0.4, 0.0, 0.0, 0.0], 12).write(&mut bytes);
    let first_at = write_point_data(&mut bytes, FIRST_INPUT, [2.0, 1.0, -4.0]);
    let second_at = write_point_data(&mut bytes, SECOND_INPUT, [2.0, 0.6, -4.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    types.insert(u64::from(SECOND_INPUT), (POINT_DATA_TYPE_GUID, 1));
    let construction = read(&bytes, &types).expect("two-point carrier");
    assert_eq!(
        construction.source,
        Some(DesignWorkAxisSource::TwoPoint {
            point_record_indices: [FIRST_INPUT, SECOND_INPUT],
            point_offsets: [
                u64::try_from(first_at).unwrap(),
                u64::try_from(second_at).unwrap(),
            ],
        })
    );
}

#[test]
fn two_point_rule_with_a_displaced_endpoint_stays_native() {
    let mut bytes = Vec::new();
    Carrier::new([2.0, 1.0, -4.0, 0.0, -0.4, 0.0, 0.0, 0.0], 12).write(&mut bytes);
    write_point_data(&mut bytes, FIRST_INPUT, [2.0, 1.0, -4.0]);
    write_point_data(&mut bytes, SECOND_INPUT, [2.0, 0.5, -4.0]);
    let mut types = carrier_types(1);
    types.insert(u64::from(FIRST_INPUT), (POINT_DATA_TYPE_GUID, 1));
    types.insert(u64::from(SECOND_INPUT), (POINT_DATA_TYPE_GUID, 1));
    assert!(read(&bytes, &types).is_none());
}

#[test]
fn point_direction_carrier_outside_the_axis_value_block_stays_native() {
    let cases = [
        Carrier {
            value_count: 7,
            ..Carrier::new(unassigned_rule_values(), 18)
        },
        Carrier::new([1.5, -2.0, 3.25, 0.6, 0.0, 0.8, 0.25, 0.0], 18),
        Carrier::new([1.5, -2.0, 3.25, 0.0, 0.0, 0.0, 0.0, 0.0], 18),
    ];
    for carrier in cases {
        let mut bytes = Vec::new();
        carrier.write(&mut bytes);
        assert!(read(&bytes, &carrier_types(1)).is_none());
    }
}

#[test]
fn point_direction_work_axis_requires_the_registered_carrier_type() {
    let mut bytes = Vec::new();
    Carrier::new(unassigned_rule_values(), 18).write(&mut bytes);
    assert!(read(&bytes, &HashMap::new()).is_none());
    assert!(read(
        &bytes,
        &HashMap::from([(u64::from(CARRIER), (POINT_DATA_TYPE_GUID, 1))])
    )
    .is_none());
}
