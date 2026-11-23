// SPDX-License-Identifier: Apache-2.0

use super::{
    classify_face_plane, classify_inserted_face, FaceSurfaceIndex, InsertedFaceClassification,
};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalPlane,
    AsmHistoricalSurfaceAxis, AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
    AsmHistoricalTransition,
};
use cadmpeg_ir::math::{Point3, Vector3};

/// The plane of `face` by a scan of every surface row.
fn scanned_face_plane(topology: &AsmHistoricalTopology, face: i64) -> Option<(Point3, Vector3)> {
    let mut bindings = topology.face_surfaces.iter().filter(|f| f.entity == face);
    let carrier = bindings.next()?.carrier;
    if bindings.next().is_some() {
        return None;
    }
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|s| s.surface == carrier);
    let curved = topology
        .surface_cylinders
        .iter()
        .any(|s| s.surface == carrier && s.axis.unit().is_some() && s.radius > 0.0)
        || topology
            .surface_axes
            .iter()
            .any(|s| s.surface == carrier && s.direction.unit().is_some())
        || topology
            .surface_radii
            .iter()
            .any(|s| s.surface == carrier && s.radius > 0.0 && s.radius.is_finite());
    let plane = planes.next()?;
    if planes.next().is_some() || curved || plane.normal.unit().is_none() {
        return None;
    }
    Some((plane.origin, plane.normal))
}

fn binding(entity: i64, carrier: i64) -> AsmHistoricalCarrierBinding {
    AsmHistoricalCarrierBinding { entity, carrier }
}

fn plane(surface: i64, normal: Vector3) -> AsmHistoricalPlane {
    AsmHistoricalPlane {
        surface,
        origin: Point3::new(0.0, 0.0, f64::from(u32::try_from(surface).unwrap())),
        normal,
    }
}

/// Faces 1..=9 cover a single plane, a doubly bound face, a carrier with
/// two planes, planes shared with a cylinder, an axis or a radius, a zero
/// normal, a carrier with no plane, and an unbound face.
fn topology() -> AsmHistoricalTopology {
    let up = Vector3::new(0.0, 0.0, 1.0);
    let zero = Vector3::new(0.0, 0.0, 0.0);
    AsmHistoricalTopology {
        faces: (1..=10).collect(),
        face_surfaces: vec![
            binding(1, 101),
            binding(2, 102),
            binding(2, 101),
            binding(3, 103),
            binding(4, 104),
            binding(5, 105),
            binding(6, 106),
            binding(7, 107),
            binding(8, 108),
            binding(9, 109),
        ],
        surface_planes: vec![
            plane(101, up),
            plane(102, up),
            plane(103, up),
            plane(103, up),
            plane(104, up),
            plane(105, up),
            plane(106, up),
            plane(107, zero),
            plane(109, up),
        ],
        surface_cylinders: vec![
            AsmHistoricalCylinder {
                surface: 104,
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: up,
                radius: 2.0,
            },
            AsmHistoricalCylinder {
                surface: 108,
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: up,
                radius: 2.0,
            },
            AsmHistoricalCylinder {
                surface: 109,
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: zero,
                radius: 2.0,
            },
        ],
        surface_axes: vec![AsmHistoricalSurfaceAxis {
            surface: 105,
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: up,
        }],
        surface_radii: vec![
            AsmHistoricalSurfaceRadius {
                surface: 106,
                radius: 3.0,
            },
            AsmHistoricalSurfaceRadius {
                surface: 109,
                radius: f64::INFINITY,
            },
        ],
        ..AsmHistoricalTopology::default()
    }
}

#[test]
fn face_plane_matches_a_scan_of_the_surface_rows() {
    let topology = topology();
    let surfaces = FaceSurfaceIndex::new(&topology);
    for face in 1..=10 {
        assert_eq!(
            classify_face_plane(&surfaces, face),
            scanned_face_plane(&topology, face),
            "face {face}"
        );
    }
    assert!(classify_face_plane(&surfaces, 1).is_some());
    assert!(classify_face_plane(&surfaces, 9).is_some());
}

#[test]
fn inserted_face_classification_reads_the_index() {
    let topology = topology();
    let surfaces = FaceSurfaceIndex::new(&topology);
    let transition = AsmHistoricalTransition {
        geometry_unchanged: false,
        previous_state_id: None,
        records: Default::default(),
        topology: Default::default(),
        record_revisions: Vec::new(),
        rows: Default::default(),
    };
    let classify = |face| classify_inserted_face(&surfaces, &transition, face);
    assert!(matches!(classify(1), InsertedFaceClassification::Plane(..)));
    assert!(matches!(classify(8), InsertedFaceClassification::NonPlanar));
    assert!(matches!(
        classify(4),
        InsertedFaceClassification::Unclassifiable
    ));
    assert!(matches!(
        classify(2),
        InsertedFaceClassification::Unclassifiable
    ));
    assert!(matches!(
        classify(10),
        InsertedFaceClassification::Unclassifiable
    ));
}
