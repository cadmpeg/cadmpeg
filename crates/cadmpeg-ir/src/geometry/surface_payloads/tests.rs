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

#[test]
fn exact_and_compound_payloads_reject_nonfinite_nested_parameters() {
    use super::{CompoundSurfacePayload, ExactSurfacePayload};
    use crate::geometry::{CompoundComponent, ExactSpline};

    let exact = |range| {
        ExactSurfacePayload::try_new(ExactSpline::Legacy {
            ranges: [range, [0.0, 1.0]],
            extension: 0,
        })
    };
    let valid = ProceduralSurfaceDefinition::Exact(exact([1.0, 1.0]).unwrap());
    let wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        valid
    );
    for range in [[2.0, 1.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(exact(range).is_err());
    }
    let mut invalid = wire;
    invalid["parameters"]["ranges"][0] = serde_json::json!([2.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(invalid).is_err());
    assert!(CompoundSurfacePayload::try_new(Vec::new()).is_ok());
    for parameter in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(CompoundSurfacePayload::try_new(vec![CompoundComponent {
            parameter,
            component: support(),
        }])
        .is_err());
    }
}

#[test]
fn loft_payload_admits_only_finite_entries_and_bridge_doubles() {
    use super::LoftSurfacePayload;
    use crate::geometry::{
        LoftBridgeToken, LoftPath, LoftSection, LoftSectionEntry, SplineSurfaceParameters,
    };

    let loft = |parameter, bridge| {
        LoftSurfacePayload::try_new(
            [
                LoftSection {
                    entries: vec![LoftSectionEntry {
                        parameter,
                        profile: Vec::new(),
                        path: LoftPath {
                            curve: None,
                            auxiliaries: Vec::new(),
                            flag: 0,
                        },
                    }],
                },
                LoftSection {
                    entries: Vec::new(),
                },
            ],
            SplineSurfaceParameters::OrderedRanges {
                ranges: [[0.0, 0.0], [0.0, 1.0]],
            },
            [0; 2],
            [0; 2],
            0,
            vec![LoftBridgeToken::Double(bridge)],
            None,
        )
    };
    let valid = ProceduralSurfaceDefinition::Loft(loft(-1.0, -2.0).unwrap());
    let wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        valid
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(loft(value, 0.0).is_err());
        assert!(loft(0.0, value).is_err());
    }
    let mut invalid = wire;
    invalid["parameters"]["ranges"][0] = serde_json::json!([1.0, 0.0]);
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(invalid).is_err());
}
