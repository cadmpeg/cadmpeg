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
}
