// SPDX-License-Identifier: Apache-2.0
//! Fixed scalar lanes of symmetric and two-sided distance Extrudes.

use crate::design::decode::scopes::fixed_parameters::exact_fixed_extrude_parameters;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::u64_from_index;

const SCOPE: u32 = 10;

/// 104-byte fixed scalar frames owned by [`SCOPE`], one per `(record, ordinal,
/// value)` lane, each followed by its paired header.
fn scalar_lanes(lanes: &[(u32, u8, f64)]) -> (Vec<u8>, Vec<u64>) {
    let mut bytes = Vec::new();
    let mut value_offsets = Vec::new();
    for &(record_index, ordinal, value) in lanes {
        let start = bytes.len();
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"273");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&SCOPE.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"272");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
        value_offsets.push(u64_from_index(start + 40));
    }
    (bytes, value_offsets)
}

fn distance_extrude(extent: DesignExtrudeExtent, references: Vec<u32>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Extrude,
        SCOPE,
    );
    let direction = match extent {
        DesignExtrudeExtent::TwoSidedDistance => 2,
        DesignExtrudeExtent::SymmetricDistance => 3,
        _ => 1,
    };
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 28,
                direction_face_extend_values: [direction, 0],
                side_extent_discriminators: [1, 1],
                side_extent_discriminator_offsets: [77, 90],
                first_side_target_ordinal: None,
                extent,
                direction_face_extend_offsets: [32, 36],
                direction_reversed: false,
                direction_reversed_offset: 40,
                solid_operation: true,
                solid_operation_offset: 41,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 42,
            });
    }
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(references);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn decoded(bytes: &[u8], scope: &DesignParameterScope) -> Option<DesignFixedExtrudeParameters> {
    exact_fixed_extrude_parameters(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &[],
        &[],
    )
}

fn lane(record_index: u32, value: f64, value_offset: u64) -> DesignFixedExtrudeScalar {
    DesignFixedExtrudeScalar {
        value: crate::test_support::real(value),
        record_index,
        value_offset,
    }
}

fn distance_lane(
    record_index: u32,
    value: f64,
    value_offset: u64,
) -> DesignFixedExtrudeScalar<cadmpeg_ir::scalar::NonZeroReal> {
    DesignFixedExtrudeScalar {
        value: cadmpeg_ir::scalar::NonZeroReal::new(value).unwrap(),
        record_index,
        value_offset,
    }
}

/// A two-sided distance Extrude without indexed parameters stores its first
/// distance, first taper, second distance, and second taper in lanes zero
/// through three.
#[test]
fn two_sided_distance_extrude_decodes_four_fixed_lanes() {
    let (bytes, offsets) = scalar_lanes(&[(11, 0, 4.3), (14, 1, 0.0), (17, 2, 0.2), (20, 3, 0.0)]);
    let scope = distance_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        vec![11, 14, 17, 20, 30],
    );
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                11, 4.3, offsets[0]
            ))),
            taper_angle: Some(lane(14, 0.0, offsets[1])),
            against_distance: Some(distance_lane(17, 0.2, offsets[2])),
            side_two_taper_angle: Some(lane(20, 0.0, offsets[3])),
        })
    );

    // A two-sided Extrude needs both distance lanes.
    let scope = distance_extrude(DesignExtrudeExtent::TwoSidedDistance, vec![11, 14, 30]);
    assert_eq!(decoded(&bytes, &scope), None);
    // A one-sided Extrude has no second-side lanes.
    let scope = distance_extrude(
        DesignExtrudeExtent::OneSidedDistance,
        vec![11, 14, 17, 20, 30],
    );
    assert_eq!(decoded(&bytes, &scope), None);
}

/// A symmetric distance Extrude without indexed parameters stores its distance
/// and taper in lanes zero and one.
#[test]
fn symmetric_distance_extrude_decodes_two_fixed_lanes() {
    let (bytes, offsets) = scalar_lanes(&[(11, 0, 1.2), (14, 1, 0.0)]);
    let scope = distance_extrude(DesignExtrudeExtent::SymmetricDistance, vec![11, 14, 30]);
    assert_eq!(
        decoded(&bytes, &scope),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance_lane(
                11, 1.2, offsets[0]
            ))),
            taper_angle: Some(lane(14, 0.0, offsets[1])),
            against_distance: None,
            side_two_taper_angle: None,
        })
    );
}
