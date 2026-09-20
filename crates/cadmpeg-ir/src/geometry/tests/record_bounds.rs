// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{ProceduralSurface, RecordBounds};
use crate::ids::ProceduralSurfaceId;

fn id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("test:geometry:procedural-surface#bounds").expect("valid identity")
}

#[test]
fn partial_record_bounds_round_trip_through_checked_storage() {
    let bounds = RecordBounds::try_new([Some(0.1), None, Some(0.2), None])
        .expect("finite partial record bounds");
    assert_eq!(bounds.get(), [Some(0.1), None, Some(0.2), None]);

    let wire = serde_json::to_value(bounds).expect("serialize record bounds");
    assert_eq!(wire, serde_json::json!([0.1, null, 0.2, null]));
    assert_eq!(
        serde_json::from_value::<RecordBounds>(wire).expect("deserialize record bounds"),
        bounds
    );
}

#[test]
fn non_finite_record_bounds_are_rejected_without_mutating_the_surface() {
    let mut surface = ProceduralSurface::new(
        id(),
        crate::geometry::ProceduralSurfaceDefinition::Unknown {
            record: None,
            cache: None,
        },
        Some(
            RecordBounds::try_new([Some(0.1), Some(0.9), None, None])
                .expect("finite initial bounds"),
        ),
    );

    assert!({
        let replacement = Some([Some(f64::NAN), None, Some(0.2), None]);
        cadmpeg_test_support::edit::replace(&mut surface, |previous| {
            crate::geometry::RecordBounds::try_option(replacement).map(|bounds| {
                crate::geometry::ProceduralSurface::new(
                    previous.id.clone(),
                    previous.definition().clone(),
                    bounds,
                )
            })
        })
    }
    .is_err());
    assert_eq!(
        surface.record_bounds(),
        Some([Some(0.1), Some(0.9), None, None])
    );
    assert!(RecordBounds::try_option(Some([Some(f64::INFINITY), None, None, None,])).is_err());
}
