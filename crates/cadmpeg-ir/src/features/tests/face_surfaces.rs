use crate::features::evaluation::{HistoricalFaceGeometry, HistoricalFaceSurface};
use crate::geometry::analytic::CylinderSurface;
use crate::geometry::SolvedSurfaceGeometry;
use crate::ids::HistoricalFaceId;
use crate::math::{Point3, Vector3};
use cadmpeg_core::serde_erase::json;

fn cylinder() -> CylinderSurface {
    CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        5.0,
    )
    .unwrap()
}

fn geometry() -> HistoricalFaceGeometry {
    HistoricalFaceGeometry {
        face: HistoricalFaceId::mint("test:model:historical-face#one").unwrap(),
        surface: HistoricalFaceSurface::Cylinder(cylinder()),
        reversed: true,
    }
}

#[test]
fn a_historical_face_surface_round_trips_through_json() {
    let face = geometry();
    let json = json::to_value(&face).unwrap();
    assert_eq!(json["surface"]["kind"], "cylinder");
    assert_eq!(json["reversed"], true);
    assert_eq!(
        json::from_value::<HistoricalFaceGeometry>(json).unwrap(),
        face
    );
}

#[test]
fn a_face_surface_that_refers_to_a_record_is_refused_on_wire() {
    let mut json = json::to_value(geometry()).unwrap();
    json["surface"] = cadmpeg_core::json!({"kind": "unknown"});
    assert!(json::from_value::<HistoricalFaceGeometry>(json.clone()).is_err());
    json["surface"] = cadmpeg_core::json!({"kind": "polygonal"});
    assert!(json::from_value::<HistoricalFaceGeometry>(json).is_err());
}

#[test]
fn only_surfaces_without_arena_references_become_face_surfaces() {
    let solved = SolvedSurfaceGeometry::Cylinder(cylinder());
    assert_eq!(
        HistoricalFaceSurface::from_solved(&solved).map(|surface| surface.to_solved()),
        Some(solved)
    );
    assert!(
        HistoricalFaceSurface::from_solved(&SolvedSurfaceGeometry::Unknown { record: None })
            .is_none()
    );
}
