// SPDX-License-Identifier: Apache-2.0
//! Plane checks for an attached-plate face profile.

use crate::{
    features::{FeatureId, PlanarProfileRef},
    geometry::{analytic::PlaneSurface, SolvedSurfaceGeometry},
    index::{ModelIndex, StandardIndex},
    report::check::Finding,
};

const EPS_PROFILE_NORMAL: f64 = 1.0e-9;
const EPS_PROFILE_DISTANCE_MM: f64 = 1.0e-9;

fn coplanar(first: &PlaneSurface, second: &PlaneSurface) -> bool {
    let normal = first.frame().axis().as_raw();
    let separation = second.origin().get().vector_from(first.origin().get());
    normal.cross(*second.frame().axis().as_raw()).norm() <= EPS_PROFILE_NORMAL
        && separation.dot(*normal).abs() <= EPS_PROFILE_DISTANCE_MM
}

pub(super) fn check_profile(
    owner: &FeatureId,
    profile: &PlanarProfileRef,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let PlanarProfileRef::Faces(faces) = profile else {
        return;
    };
    let mut support = None;
    let mut planar = !faces.is_empty();
    for face in faces {
        let Some(face) = ids.faces(face.as_str(), StandardIndex) else {
            continue;
        };
        let Some(surface) = ids.surfaces(face.surface.as_str(), StandardIndex) else {
            continue;
        };
        let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
            planar = false;
            break;
        };
        if support.is_some_and(|support| !coplanar(support, plane)) {
            planar = false;
            break;
        }
        support = Some(plane);
    }
    if !planar {
        super::geometry_error(
            findings,
            owner.as_str(),
            "attached plate profile is not planar",
        );
    }
}

pub(super) fn check_pair(
    feature: &crate::features::Feature,
    features: &[crate::features::Feature],
    findings: &mut Vec<Finding>,
) {
    use crate::features::{FeatureDefinition, FeatureOperation};
    let count = features
        .iter()
        .filter(|bend| {
            matches!(bend.evaluation.definition(), FeatureDefinition::Operation(
            FeatureOperation::SheetMetalBend { plate, .. }
        ) if *plate == feature.id)
        })
        .count();
    if count != 1 {
        super::geometry_error(
            findings,
            feature.id.as_str(),
            "attached plate requires exactly one bend",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Point3, Vector3};

    #[test]
    fn plane_profiles_require_one_support_plane() {
        let plane =
            |origin, normal, u_axis| PlaneSurface::try_new(origin, normal, u_axis).expect("plane");
        let first = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let same = plane(
            Point3::new(4.0, 5.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let offset = plane(
            Point3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        );
        let perpendicular = plane(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        );
        assert!(coplanar(&first, &same));
        assert!(!coplanar(&first, &offset));
        assert!(!coplanar(&first, &perpendicular));
    }

    #[test]
    fn neutral_face_profiles_report_nonplanarity() {
        let ir = crate::examples::unit_cube().expect("cube");
        let ids = ModelIndex::build(&ir);
        let owner = FeatureId::mint("test:model:feature#plate").expect("identity");
        let profile =
            PlanarProfileRef::Faces(ir.model.faces.iter().map(|face| face.id.clone()).collect());
        let mut findings = Vec::new();
        check_profile(&owner, &profile, &ids, &mut findings);
        assert!(findings
            .iter()
            .any(|finding| finding.message == "attached plate profile is not planar"));
        findings.clear();
        let profile = PlanarProfileRef::Faces(vec![ir.model.faces[0].id.clone()]);
        check_profile(&owner, &profile, &ids, &mut findings);
        assert!(findings.is_empty());
    }
    #[test]
    fn plate_and_bend_selections_require_their_own_input_topology() {
        use crate::{
            features::*,
            ids::{FeatureInputTopologyId, HistoricalEdgeId},
            scalar::{NonNegativeLength, PositiveLength},
        };
        let mut ir = crate::examples::unit_cube().expect("cube");
        let plate_id = FeatureId::mint("test:model:feature#plate").expect("plate");
        let bend_id = FeatureId::mint("test:model:feature#bend").expect("bend");
        let edge = HistoricalEdgeId::mint("test:input-edge#selected").expect("edge");
        let plate_state = FeatureInputTopologyId::mint("test:model:input#plate").expect("state");
        let bend_state = FeatureInputTopologyId::mint("test:model:input#bend").expect("state");
        let selection = EdgeSelection::historical(
            plate_state.clone(),
            vec![edge.clone()],
            "test:attachment".into(),
        )
        .expect("selection");
        let bend_selection = EdgeSelection::historical(
            bend_state.clone(),
            vec![edge.clone()],
            "test:attachment".into(),
        )
        .expect("bend selection");
        let relief = SheetMetalRelief {
            form: SheetMetalReliefForm::StyleDefault,
            width: PositiveLength::new(1.0).expect("width"),
            depth: PositiveLength::new(2.0).expect("depth"),
            minimum_remnant: NonNegativeLength::new(0.0).expect("remnant"),
            seam_gap: NonNegativeLength::new(0.0).expect("gap"),
        };
        let transition = SheetMetalTransition {
            form: SheetMetalTransitionForm::StyleDefault,
            radius: NonNegativeLength::new(0.0).expect("radius"),
        };
        let feature = |id, ordinal, operation| Feature {
            product_definition: Default::default(),
            id,
            ordinal,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
            native_ref: None,
        };
        ir.model.features = vec![
            feature(
                plate_id.clone(),
                0,
                FeatureOperation::SheetMetalAttachedPlate {
                    profile: PlanarProfileRef::Faces(vec![ir.model.faces[0].id.clone()]),
                    thickness: PositiveLength::new(1.0).expect("thickness"),
                    side: SheetMetalThicknessSide::Forward,
                    attachment: selection.clone(),
                },
            ),
            feature(
                bend_id.clone(),
                1,
                FeatureOperation::SheetMetalBend {
                    plate: plate_id.clone(),
                    attachment: bend_selection.clone(),
                    inside_radius: PositiveLength::new(1.0).expect("radius"),
                    relief,
                    transition,
                },
            ),
        ];
        ir.model.features[1].dependencies.insert(plate_id.clone());
        let geometry = HistoricalEdgeGeometry {
            edge: edge.clone(),
            start: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)),
            end: FinitePoint3::new(Point3::new(2.0, 0.0, 0.0)),
            interior: FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("point"),
            tangent: Some(crate::units::UnitVector3::X_AXIS),
            faces: DistinctMembers::default(),
        };
        let state = |id, input_of| FeatureInputTopology {
            id,
            input_of,
            bodies: DistinctMembers::default(),
            faces: DistinctMembers::default(),
            edges: vec![edge.clone()].try_into().expect("edges"),
            vertices: DistinctMembers::default(),
            edge_geometry: vec![geometry.clone()],
            native_ref: Some("test:attachment".into()),
        };
        ir.model.feature_input_topologies = vec![
            state(plate_state, plate_id),
            state(bend_state, bend_id.clone()),
        ];
        let findings = crate::validate::validate_neutral(&ir, Vec::new())
            .expect("validate")
            .findings;
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("another feature's input topology")));
        ir.model.features[0].evaluation.edit(|definition, _| {
            if let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
                attachment,
                ..
            }) = definition
            {
                *attachment = bend_selection;
            }
        });
        let findings = crate::validate::validate_neutral(&ir, Vec::new())
            .expect("validate")
            .findings;
        assert!(findings
            .iter()
            .any(|finding| finding.message.contains("another feature's input topology")));
    }
}
