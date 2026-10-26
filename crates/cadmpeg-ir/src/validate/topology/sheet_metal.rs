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

pub(super) fn shared_attachment_owner<'a>(
    ir: &'a crate::CadIr,
    feature: &crate::features::Feature,
    selection: &crate::features::EdgeSelection,
) -> Option<&'a FeatureId> {
    use crate::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalAttachedPlate {
        attachment, ..
    }) = feature.evaluation.definition()
    else {
        return None;
    };
    if attachment != selection {
        return None;
    }
    let EdgeSelection::Historical { state, edges, .. } = selection else {
        return None;
    };
    let selected = ir
        .model
        .feature_input_topologies
        .iter()
        .find(|input| input.id == *state)?;
    let bend = ir
        .model
        .features
        .iter()
        .find(|bend| bend.id == selected.input_of)?;
    let FeatureDefinition::Operation(FeatureOperation::SheetMetalBend {
        plate, attachment, ..
    }) = bend.evaluation.definition()
    else {
        return None;
    };
    if *plate != feature.id
        || attachment != selection
        || bend.ordinal <= feature.ordinal
        || !bend.dependencies.contains(&feature.id)
    {
        return None;
    }
    let mut own_states = ir
        .model
        .feature_input_topologies
        .iter()
        .filter(|input| input.input_of == feature.id);
    let own = own_states.next()?;
    if own_states.next().is_some()
        || own.bodies != selected.bodies
        || own.faces != selected.faces
        || own.edges != selected.edges
        || own.vertices != selected.vertices
        || own.edge_geometry != selected.edge_geometry
        || own.native_ref != selected.native_ref
        || edges.iter().any(|edge| {
            !selected.edge_geometry.iter().any(|geometry| {
                geometry.edge == *edge
                    && geometry.start.is_some()
                    && geometry.end.is_some()
                    && geometry.tangent.is_some()
            })
        })
    {
        return None;
    }
    Some(&bend.id)
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
    fn shared_plate_bend_input_requires_equal_selection_and_complete_matching_states() {
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
            bend_state.clone(),
            vec![edge.clone()],
            "test:attachment".into(),
        )
        .expect("selection");
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
                    op: BooleanOp::Join,
                    attachment: selection.clone(),
                    inside_radius: PositiveLength::new(1.0).expect("radius"),
                    relief: relief.clone(),
                    transition: transition.clone(),
                },
            ),
            feature(
                bend_id.clone(),
                1,
                FeatureOperation::SheetMetalBend {
                    plate: plate_id.clone(),
                    attachment: selection.clone(),
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
        assert_eq!(
            shared_attachment_owner(&ir, &ir.model.features[0], &selection),
            Some(&bend_id)
        );
        let findings = crate::validate::validate_neutral(&ir, Vec::new())
            .expect("validate")
            .findings;
        assert!(!findings
            .iter()
            .any(|finding| finding.message.contains("another feature's input topology")));
        for change in 0..5 {
            let mut changed = ir.clone();
            match change {
                0 => {
                    changed.model.feature_input_topologies.remove(0);
                }
                1 => {
                    changed.model.feature_input_topologies[0].edge_geometry[0].start =
                        FinitePoint3::new(Point3::new(3.0, 0.0, 0.0));
                }
                2 => changed.model.feature_input_topologies[0].edge_geometry[0].tangent = None,
                3 => changed.model.features[1].dependencies = DistinctMembers::default(),
                _ => changed.model.features[1].evaluation.edit(|definition, _| {
                    if let FeatureDefinition::Operation(FeatureOperation::SheetMetalBend {
                        attachment,
                        ..
                    }) = definition
                    {
                        *attachment = EdgeSelection::All;
                    }
                }),
            }
            assert!(
                shared_attachment_owner(&changed, &changed.model.features[0], &selection).is_none()
            );
        }
    }
}
