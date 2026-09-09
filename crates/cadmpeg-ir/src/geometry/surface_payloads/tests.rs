// SPDX-License-Identifier: Apache-2.0
use super::{OffsetSurfaceConstruction, SubSurfaceConstruction, SubsetSurfaceConstruction};
use crate::geometry::{LegacyExtensionFlags, OffsetExtension, ProceduralSurfaceDefinition};
use crate::ids::SurfaceId;

fn support() -> SurfaceId {
    SurfaceId::mint("synthetic:test:surface#support").unwrap()
}

#[test]
fn surface_restrictions_keep_their_distinct_range_domains_and_wire_fields() {
    let ranges = [[2.0, 0.0], [1.0, 1.0]];
    let definition = ProceduralSurfaceDefinition::SubSurface(
        SubSurfaceConstruction::try_new(support(), ranges).unwrap(),
    );
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "kind": "sub_surface", "support": support(), "parameter_ranges": ranges,
        })
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    assert!(SubsetSurfaceConstruction::try_new(support(), ranges, None, None).is_err());
    assert!(
        SubsetSurfaceConstruction::try_new(support(), [[2.0, 0.0], [3.0, 1.0]], None, None).is_ok()
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(SubSurfaceConstruction::try_new(support(), [[value, 1.0], [0.0, 1.0]]).is_err());
    }
}

#[test]
fn offset_distance_mutation_preserves_the_previous_value_on_rejection() {
    let mut payload = OffsetSurfaceConstruction::try_new(
        support(),
        -2.0,
        None,
        None,
        None,
        OffsetExtension::Legacy(LegacyExtensionFlags::Absent),
    )
    .unwrap();
    let before = payload.clone();
    for distance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(payload.try_set_distance(distance).is_err());
        assert_eq!(payload, before);
    }
    payload.try_set_distance(0.0).unwrap();
    assert_eq!(*payload.distance(), 0.0);
}
