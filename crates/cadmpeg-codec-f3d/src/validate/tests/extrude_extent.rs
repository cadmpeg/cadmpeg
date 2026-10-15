// SPDX-License-Identifier: Apache-2.0
//! Extrude extents carried by fixed scalar lanes.

use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::fixed_parameters::{
    DesignFixedExtrudeDistance, DesignFixedExtrudeParameters, DesignFixedExtrudeScalar,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

const CONFLICT: &str =
    "Fusion Design Extrude start or extent conflicts with its parameters and face operands";

fn distance(
    value: f64,
    record_index: u32,
) -> DesignFixedExtrudeScalar<cadmpeg_ir::scalar::NonZeroReal> {
    DesignFixedExtrudeScalar {
        value: cadmpeg_ir::scalar::NonZeroReal::new(value).unwrap(),
        record_index,
        value_offset: u64::from(record_index) * 100,
    }
}

/// A NewBody distance Extrude without indexed parameters whose fixed lanes
/// are `fixed`.
fn fixed_extrude(
    extent: DesignExtrudeExtent,
    fixed: DesignFixedExtrudeParameters,
) -> crate::native::F3dNative {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::Extrude,
        10,
    );
    if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
        scope.payload_mut()
    {
        let extrude = slot.get_or_insert_with(Default::default);
        extrude.extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 28,
            direction_face_extend_values: [2, 0],
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
        extrude.fixed_extrude_parameters = Some(fixed);
    }
    crate::native::F3dNative {
        design_parameter_scopes: vec![scope],
        ..Default::default()
    }
}

fn conflicts(native: &crate::native::F3dNative) -> bool {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_extrude_parameter_operands(&ctx, &mut findings);
        findings.iter().any(|finding| finding.message == CONFLICT)
    })
}

/// Fixed first-side and second-side distance lanes carry a two-sided distance
/// Extrude. Without the second-side lane the extent has no second distance.
#[test]
fn two_sided_distance_extrude_accepts_fixed_distance_lanes() {
    let lanes = |against_distance| DesignFixedExtrudeParameters {
        along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(distance(4.3, 11))),
        taper_angle: None,
        against_distance,
        side_two_taper_angle: None,
    };
    assert!(!conflicts(&fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        lanes(Some(distance(0.2, 17))),
    )));
    assert!(conflicts(&fixed_extrude(
        DesignExtrudeExtent::TwoSidedDistance,
        lanes(None),
    )));
}
