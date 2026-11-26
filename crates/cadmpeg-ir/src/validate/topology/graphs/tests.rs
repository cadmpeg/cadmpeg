// SPDX-License-Identifier: Apache-2.0

use crate::report::{check::Check, Severity};

#[test]
fn radial_ring_crossing_keeps_error_order_and_owner() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let owner = ir.geometry.coedges[0].id.clone();
    let other = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.edge != ir.geometry.coedges[0].edge)
        .unwrap()
        .id
        .clone();
    ir.geometry.coedges[0].radial_next = other;
    let mut findings = Vec::new();
    super::check_coedge_pairing(&ir, &mut findings).unwrap();
    assert!(findings.len() >= 2);
    for finding in &findings[..2] {
        assert_eq!(finding.check, Check::CoedgePairing);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.entity.as_deref(), Some(owner.as_str()));
    }
    assert_eq!(findings[0].message, "radial ring crosses edges");
    assert_eq!(findings[1].message, "radial ring does not close");
}

/// Faces with one vertex loop on each named vertex, each in the shell its
/// index names.
fn vertex_loop_model(faces: &[(&str, usize, &[&str])], shell_count: usize) -> crate::CadIr {
    use crate::ids::{FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId};
    use crate::topology::{Face, FaceLoops, Loop, LoopBoundary, Sense, Shell};
    let mut ir = crate::CadIr::empty();
    let shell_id = |index: usize| ShellId::mint(format!("test:model:shell#{index}")).unwrap();
    for (face, shell, vertices) in faces {
        let face_id = FaceId::mint(format!("test:model:face#{face}")).unwrap();
        let loop_ids: Vec<LoopId> = vertices
            .iter()
            .map(|vertex| LoopId::mint(format!("test:model:loop#{face}-{vertex}")).unwrap())
            .collect();
        ir.geometry.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id(*shell),
            surface: SurfaceId::mint("test:model:surface#plane").unwrap(),
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(loop_ids.clone()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
        for (loop_id, vertex) in loop_ids.into_iter().zip(*vertices) {
            ir.geometry.loops.push(Loop {
                id: loop_id,
                face: face_id.clone(),
                boundary: LoopBoundary::Vertex {
                    vertex: VertexId::mint(format!("test:model:vertex#{vertex}")).unwrap(),
                    pcurves: Vec::new(),
                },
                native_ref: None,
            });
        }
    }
    for index in 0..shell_count {
        ir.geometry.shells.push(Shell::new(
            shell_id(index),
            RegionId::mint("test:model:region#0").unwrap(),
        ));
    }
    ir
}

fn disconnected_shells(ir: &crate::CadIr) -> Vec<String> {
    let mut findings = Vec::new();
    super::check_shell_connectivity(&crate::index::ModelIndex::new_model_only(ir), &mut findings);
    findings
        .into_iter()
        .map(|finding| {
            assert_eq!(finding.check, Check::ShellTopology);
            finding.entity.unwrap()
        })
        .collect()
}

#[test]
fn shell_connectivity_joins_only_the_shells_own_faces() {
    // Faces a and b share no vertex; c, in another shell, shares one with
    // each. Faces d and e are joined by f in their own shell.
    let faces: [(&str, usize, &[&str]); 6] = [
        ("a", 0, &["1"]),
        ("b", 0, &["2"]),
        ("c", 1, &["1", "2"]),
        ("d", 2, &["3"]),
        ("e", 2, &["4"]),
        ("f", 2, &["3", "4"]),
    ];
    let ir = vertex_loop_model(&faces, 3);
    assert_eq!(disconnected_shells(&ir), ["test:model:shell#0"]);
}

#[test]
fn shell_connectivity_through_one_vertex_shared_by_many_faces_is_linear() {
    // Every ordered pair of faces on the shared vertex would be 2^32 steps.
    const FACES: usize = 1 << 16;
    let names: Vec<String> = (0..FACES).map(|index| index.to_string()).collect();
    let mut faces: Vec<(&str, usize, &[&str])> = names
        .iter()
        .map(|name| (name.as_str(), 0, &["shared"][..]))
        .collect();
    let ir = vertex_loop_model(&faces, 1);
    assert!(disconnected_shells(&ir).is_empty());
    faces.push(("isolated", 0, &["other"]));
    let ir = vertex_loop_model(&faces, 1);
    assert_eq!(disconnected_shells(&ir), ["test:model:shell#0"]);
}

#[test]
fn shell_connectivity_is_not_checked_on_a_marked_shell_or_one_with_a_partial_face() {
    let faces: [(&str, usize, &[&str]); 4] = [
        ("a", 0, &["1"]),
        ("b", 0, &["2"]),
        ("c", 1, &["3"]),
        ("d", 1, &["4"]),
    ];
    let mut ir = vertex_loop_model(&faces, 2);
    assert_eq!(
        disconnected_shells(&ir),
        ["test:model:shell#0", "test:model:shell#1"]
    );
    ir.geometry.shells[0].unstated = Some(crate::topology::Unstated::Members);
    let stated = ir.geometry.faces[2]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>();
    ir.geometry.faces[2].loops = crate::topology::FaceLoops::PartialUnspecified { loops: stated };
    assert!(disconnected_shells(&ir).is_empty());
}

#[test]
fn each_partial_record_is_reported_at_warning() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let stated = ir.geometry.faces[0]
        .loops
        .stated()
        .cloned()
        .collect::<Vec<_>>();
    ir.geometry.faces[0].loops = crate::topology::FaceLoops::PartialUnspecified { loops: stated };
    ir.geometry.shells[0].unstated = Some(crate::topology::Unstated::Members);
    ir.geometry.regions[0].unstated = Some(crate::topology::RegionUnstated::Voids);
    ir.geometry.bodies[0].unstated = Some(crate::topology::Unstated::Members);
    let mut findings = Vec::new();
    super::check_partial_topology(&ir, &mut findings);
    let mut reported = findings
        .iter()
        .map(|finding| {
            assert_eq!(finding.check, Check::TopologyPartial);
            assert_eq!(finding.severity, Severity::Warning);
            finding.entity.clone().unwrap()
        })
        .collect::<Vec<_>>();
    reported.sort();
    let mut expected = vec![
        ir.geometry.faces[0].id.as_str().to_owned(),
        ir.geometry.shells[0].id.as_str().to_owned(),
        ir.geometry.regions[0].id.as_str().to_owned(),
        ir.geometry.bodies[0].id.as_str().to_owned(),
    ];
    expected.sort();
    assert_eq!(reported, expected);
}

#[test]
fn a_region_mark_names_each_shell_once_and_none_its_shells_name() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let shell = ir.geometry.shells[0].id.clone();
    let region = ir.geometry.regions[0].id.as_str().to_owned();
    ir.geometry.regions[0].set_shells(Vec::new());
    ir.geometry.regions[0].unstated = Some(crate::topology::RegionUnstated::Unclassified {
        shells: vec![shell.clone()],
    });
    let repeated = |ir: &crate::CadIr| {
        let mut findings = Vec::new();
        super::check_child_list_reciprocity(ir, &mut findings);
        findings
            .into_iter()
            .filter(|finding| {
                finding.check == Check::ReferentialIntegrity
                    && finding.entity.as_deref() == Some(region.as_str())
            })
            .count()
    };
    assert_eq!(repeated(&ir), 0);
    ir.geometry.regions[0].set_shells(vec![shell]);
    assert_eq!(repeated(&ir), 1);
}

/// One body whose shells hold faces with no loops: each face is named, put in
/// the shell its index names, and lies on a surface of its own of `kind`.
fn loopless_model(faces: &[(&str, usize, &str)], shell_count: usize) -> crate::CadIr {
    use crate::geometry::analytic::{ConeSurface, CylinderSurface, PlaneSurface, SphereSurface};
    use crate::geometry::{PlacedSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use crate::ids::{FaceId, RegionId, ShellId, SurfaceId};
    use crate::math::{Point3, Vector3};
    use crate::topology::{Face, FaceLoops, Sense, Shell};
    let mut ir = crate::CadIr::empty();
    let shell_id = |index: usize| ShellId::mint(format!("test:model:shell#{index}")).unwrap();
    for (face, shell, kind) in faces {
        let surface = SurfaceId::mint(format!("test:model:surface#{face}")).unwrap();
        let (z, x) = (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
        let sphere = || {
            SolvedSurfaceGeometry::Sphere(
                SphereSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x, 1.0).unwrap(),
            )
        };
        let plane = || {
            SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x).unwrap(),
            )
        };
        let placed = |basis: SolvedSurfaceGeometry| {
            SolvedSurfaceGeometry::Transformed(
                PlacedSurface::try_new(Box::new(basis), crate::transform::Transform::identity())
                    .unwrap(),
            )
        };
        let geometry = match *kind {
            "sphere" => sphere(),
            "plane" => plane(),
            "cylinder" => SolvedSurfaceGeometry::Cylinder(
                CylinderSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x, 1.0).unwrap(),
            ),
            "cone" => SolvedSurfaceGeometry::Cone(
                ConeSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x, 3.0, 1.0, 0.5).unwrap(),
            ),
            "placed sphere" => placed(sphere()),
            "placed plane" => placed(placed(plane())),
            other => unreachable!("{other}"),
        };
        ir.geometry.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        ir.geometry.faces.push(Face {
            id: FaceId::mint(format!("test:model:face#{face}")).unwrap(),
            shell: shell_id(*shell),
            surface,
            sense: Sense::Forward,
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
            native_ref: None,
        });
    }
    for index in 0..shell_count {
        ir.geometry.shells.push(Shell::new(
            shell_id(index),
            RegionId::mint("test:model:region#0").unwrap(),
        ));
    }
    ir
}

fn loopless_findings(ir: &crate::CadIr) -> Vec<(String, String)> {
    let mut findings = Vec::new();
    super::check_loopless_faces(&crate::index::ModelIndex::new_model_only(ir), &mut findings);
    findings
        .into_iter()
        .map(|finding| {
            assert_eq!(finding.check, Check::ShellTopology);
            assert_eq!(finding.severity, Severity::Error);
            (finding.entity.unwrap(), finding.message)
        })
        .collect()
}

#[test]
fn a_face_with_no_loops_is_the_only_face_of_its_shell_on_an_unbounded_surface() {
    let ir = loopless_model(&[("a", 0, "sphere"), ("b", 0, "sphere")], 1);
    let found = loopless_findings(&ir);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .all(|(_, message)| message.ends_with("it is the only face of its shell")));
    let ir = loopless_model(&[("a", 0, "plane"), ("b", 1, "placed plane")], 2);
    assert_eq!(
        loopless_findings(&ir),
        [
            (
                "test:model:face#a".to_owned(),
                "a face with no loops lies on a plane, which a face bounds with a loop".to_owned()
            ),
            (
                "test:model:face#b".to_owned(),
                "a face with no loops lies on a plane, which a face bounds with a loop".to_owned()
            ),
        ]
    );
}

#[test]
fn a_lone_face_with_no_loops_on_a_closed_surface_breaks_no_rule() {
    let ir = loopless_model(&[("a", 0, "sphere"), ("b", 1, "placed sphere")], 2);
    assert!(loopless_findings(&ir).is_empty());
}

#[test]
fn a_marked_shell_is_checked_for_the_surface_alone() {
    // A marked shell does not state how many faces it holds, so two faces in
    // it may be the only ones stated of two shells; the plane face is still
    // a face with no loops on a plane.
    let mut ir = loopless_model(&[("a", 0, "sphere"), ("b", 0, "plane")], 1);
    ir.geometry.shells[0].unstated = Some(crate::topology::Unstated::Members);
    assert_eq!(
        loopless_findings(&ir),
        [(
            "test:model:face#b".to_owned(),
            "a face with no loops lies on a plane, which a face bounds with a loop".to_owned()
        )]
    );
}

#[test]
fn a_face_with_no_loops_on_a_cylinder_or_a_cone_breaks_the_rule() {
    let ir = loopless_model(&[("a", 0, "cylinder"), ("b", 1, "cone")], 2);
    assert_eq!(
        loopless_findings(&ir),
        [
            (
                "test:model:face#a".to_owned(),
                "a face with no loops lies on a cylinder, which a face bounds with a loop"
                    .to_owned()
            ),
            (
                "test:model:face#b".to_owned(),
                "a face with no loops lies on a cone, which a face bounds with a loop".to_owned()
            ),
        ]
    );
}

#[test]
fn a_partial_face_is_exempt_and_exempts_no_other_face() {
    use crate::topology::FaceLoops;
    let partial = |ir: &mut crate::CadIr, face: &str| {
        let face = ir
            .geometry
            .faces
            .iter_mut()
            .find(|candidate| candidate.id.as_str() == face)
            .unwrap();
        face.loops = FaceLoops::PartialUnspecified { loops: Vec::new() };
    };
    let mut ir = loopless_model(&[("a", 0, "plane")], 1);
    partial(&mut ir, "test:model:face#a");
    assert!(loopless_findings(&ir).is_empty());
    // A complete face beside the partial face in its shell.
    let mut ir = loopless_model(&[("a", 0, "plane"), ("b", 0, "cone")], 1);
    partial(&mut ir, "test:model:face#a");
    assert_eq!(
        loopless_findings(&ir),
        [
            (
                "test:model:face#b".to_owned(),
                "a face with no loops shares its shell with 1 other face(s); it is the only face of its shell".to_owned()
            ),
            (
                "test:model:face#b".to_owned(),
                "a face with no loops lies on a cone, which a face bounds with a loop".to_owned()
            ),
        ]
    );
    // A complete face in another shell of the region that holds the partial
    // face.
    let mut ir = loopless_model(&[("a", 0, "plane"), ("b", 1, "cylinder")], 2);
    partial(&mut ir, "test:model:face#a");
    assert_eq!(
        loopless_findings(&ir),
        [(
            "test:model:face#b".to_owned(),
            "a face with no loops lies on a cylinder, which a face bounds with a loop".to_owned()
        )]
    );
}

#[test]
fn a_face_a_source_states_no_loop_for_on_an_unbounded_surface_is_partial() {
    use crate::geometry::analytic::{PlaneSurface, SphereSurface};
    use crate::geometry::{PlacedSurface, SolvedSurfaceGeometry, SurfaceGeometry};
    use crate::ids::LoopId;
    use crate::math::{Point3, Vector3};
    use crate::topology::FaceLoops;
    let (z, x) = (Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0));
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x).unwrap(),
            )),
            crate::transform::Transform::identity(),
        )
        .unwrap(),
    ));
    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(Point3::new(0.0, 0.0, 0.0), z, x, 1.0).unwrap(),
    ));
    assert_eq!(
        FaceLoops::unspecified_on(Vec::new(), Some(&plane)),
        (
            FaceLoops::PartialUnspecified { loops: Vec::new() },
            Some("plane")
        )
    );
    assert_eq!(
        FaceLoops::unspecified_on(Vec::new(), Some(&sphere)),
        (FaceLoops::unspecified(Vec::new()), None)
    );
    assert_eq!(
        FaceLoops::unspecified_on(Vec::new(), None),
        (FaceLoops::unspecified(Vec::new()), None)
    );
    let outer = LoopId::mint("test:model:loop#0").unwrap();
    assert_eq!(
        FaceLoops::unspecified_on(vec![outer.clone()], Some(&plane)),
        (FaceLoops::unspecified(vec![outer]), None)
    );
}
