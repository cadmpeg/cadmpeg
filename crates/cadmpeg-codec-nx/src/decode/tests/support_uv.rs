// SPDX-License-Identifier: Apache-2.0
//! Support-UV admission and invalidation tests.

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{PcurveGeometry, ProceduralCurveDefinition};
use cadmpeg_ir::ids::ProceduralCurveId;
use cadmpeg_ir::math::Point3;

use crate::test_support::*;
use crate::NxCodec;

use super::*;

#[test]
fn invalidation_preserves_lanes_with_a_prior_validation_proof() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let validated_id = result.ir().model.procedural_curves[0].id.clone();
    let unvalidated_id = ProceduralCurveId("synthetic:unvalidated-support-uv".into());
    let mut unvalidated = result.ir().model.procedural_curves[0].clone();
    unvalidated.id = unvalidated_id.clone();
    {
        let mut ir = result.ir_mut();
        ir.model.procedural_curves.push(unvalidated);
        for procedural_id in [&validated_id, &unvalidated_id] {
            let procedural = ir
                .model
                .procedural_curves
                .iter_mut()
                .find(|procedural| procedural.id == *procedural_id)
                .unwrap();
            let ProceduralCurveDefinition::Intersection { context, .. } =
                &mut procedural.definition
            else {
                panic!("typed intersection");
            };
            let Some(PcurveGeometry::Nurbs { control_points, .. }) =
                context.sides[0].pcurve.as_mut()
            else {
                panic!("NURBS support lane");
            };
            for point in control_points {
                point.u += 100.0;
            }
        }
    }
    let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)];
    let parameters = vec![0.0, 0.01];
    let pending = vec![
        (
            validated_id.clone(),
            points.clone(),
            parameters.clone(),
            0.01,
            [None, None],
        ),
        (
            unvalidated_id.clone(),
            points,
            parameters,
            0.01,
            [None, None],
        ),
    ];
    let validated_lanes = BTreeSet::from([(validated_id.clone(), 0)]);
    let support_budget = WorkBudget::new(10);
    let geometry_budget = crate::decode::geometry_work::GeometryWorkBudget::new(
        crate::decode::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK,
    );

    crate::decode::support_uv::invalidate_inconsistent_support_uv_with_validated_lanes(
        &mut result.ir_mut(),
        &pending,
        &validated_lanes,
        &support_budget,
        &geometry_budget,
    );

    let pcurve_present = |procedural_id: &ProceduralCurveId| {
        let procedural = result
            .ir()
            .model
            .procedural_curves
            .iter()
            .find(|procedural| procedural.id == *procedural_id)
            .unwrap();
        let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
            panic!("typed intersection");
        };
        context.sides[0].pcurve.is_some()
    };
    assert!(pcurve_present(&validated_id));
    assert!(!pcurve_present(&unvalidated_id));
}

#[test]
fn validated_support_uv_exposes_ordered_endpoint_witnesses() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let partition =
        two_support_charted_intersection_curve_stream_with_second_plane_axis([0.0, 0.0, 1.0]);
    let mut cur = Cursor::new(prt_with_ext11_intersection(&partition, &stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural = &result.ir().model.procedural_curves[0];
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        panic!("typed intersection");
    };
    let side = context
        .sides
        .iter()
        .enumerate()
        .find(|(_, side)| side.surface.is_some() && side.pcurve.is_some())
        .map(|(side, side_data)| (side, side_data.surface.clone().unwrap()))
        .expect("charted support lane");
    let points = vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)];
    let parameters = context.parameter_range.to_vec();
    let pending = vec![(
        procedural.id.clone(),
        points.clone(),
        parameters,
        0.01,
        [None, None],
    )];
    let validated_lanes = BTreeSet::from([(procedural.id.clone(), side.0)]);

    let witnesses = crate::decode::support_uv::validated_support_uv_endpoint_witnesses(
        result.ir(),
        &pending,
        &validated_lanes,
    );

    assert_eq!(
        witnesses.get(&(procedural.curve.clone(), side.1)),
        Some(&[points[0], points[1]])
    );
}
