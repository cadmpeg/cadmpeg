// SPDX-License-Identifier: Apache-2.0
use super::*;

use std::collections::BTreeMap;

use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::extents::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureContent, FeatureId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::sketches::SketchId;

use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalFaceSense, AsmHistoricalPlane,
};

const EPS_START_OFFSET_ANGULAR: f64 = 1.0e-9;
const EPS_START_OFFSET_LENGTH: f64 = 1.0e-12;

fn length(value: f64) -> Length {
    Length::new(value).expect("finite length")
}

fn extrude(offset: Option<f64>) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:model:feature#start-offset").expect("identity"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(
                    SketchId::mint("f3d:model:sketch#start-offset").expect("identity"),
                )),
                direction: ExtrudeDirection::ProfileNormal {},
                start: ExtrudeStart::FromFace {
                    face: FaceSelection::Native("f3d:test:group#start".into()),
                    offset: offset.map(length),
                },
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::ThroughAll {},
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: None,
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: None,
    }
}

fn plane_face(forward: bool) -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![7],
        surfaces: vec![70],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 7,
            carrier: 70,
        }],
        surface_planes: vec![AsmHistoricalPlane {
            surface: 70,
            origin: Point3::new(0.0, 0.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }],
        face_senses: vec![AsmHistoricalFaceSense { face: 7, forward }],
        ..AsmHistoricalTopology::default()
    }
}

#[test]
fn a_start_offset_keeps_its_sign_when_the_outward_normal_runs_with_the_travel() {
    let travel = Vector3::new(0.0, 0.0, 1.0);
    let along = along_travel(length(-5.0), travel, travel, EPS_START_OFFSET_ANGULAR);
    assert_eq!(along, Some(length(-5.0)));
}

#[test]
fn a_start_offset_changes_sign_when_the_outward_normal_runs_against_the_travel() {
    let along = along_travel(
        length(-5.0),
        Vector3::new(0.0, 0.0, -1.0),
        Vector3::new(0.0, 0.0, 2.0),
        EPS_START_OFFSET_ANGULAR,
    );
    assert_eq!(along, Some(length(5.0)));
}

#[test]
fn an_inclined_start_face_offset_divides_by_the_normal_travel_cosine() {
    let along = along_travel(
        length(0.5),
        Vector3::new(-1.0, 0.0, 1.0),
        Vector3::new(0.0, 0.0, 1.0),
        EPS_START_OFFSET_ANGULAR,
    )
    .expect("an inclined face has a travel reading");
    assert!((along.get() - 0.5 * std::f64::consts::SQRT_2).abs() < EPS_START_OFFSET_LENGTH);
}

#[test]
fn a_start_offset_has_no_travel_reading_when_the_outward_normal_is_perpendicular() {
    let along = along_travel(
        length(-5.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        EPS_START_OFFSET_ANGULAR,
    );
    assert_eq!(along, None);
}

#[test]
fn a_historical_face_points_outward_by_its_sense() {
    assert_eq!(
        historical_outward(&plane_face(true), 7),
        Some(Vector3::new(0.0, 0.0, 1.0))
    );
    assert_eq!(
        historical_outward(&plane_face(false), 7),
        Some(Vector3::new(0.0, 0.0, -1.0))
    );
    let mut unsensed = plane_face(true);
    unsensed.face_senses.clear();
    assert_eq!(historical_outward(&unsensed, 7), None);
}

#[test]
fn an_unoriented_nonzero_start_offset_charges_the_direction_loss() {
    let code = F3dLossCode::FeatureExtrudeStartOffsetDirectionAssumed;
    let losses = unoriented_start_losses(&[extrude(Some(-5.0))], &BTreeSet::new());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, code.kind());

    let oriented = BTreeSet::from(["f3d:model:feature#start-offset".to_owned()]);
    assert!(unoriented_start_losses(&[extrude(Some(-5.0))], &oriented).is_empty());
    assert!(unoriented_start_losses(&[extrude(Some(0.0))], &BTreeSet::new()).is_empty());
    assert!(unoriented_start_losses(&[extrude(None)], &BTreeSet::new()).is_empty());
}
