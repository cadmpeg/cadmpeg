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
        false,
        OffsetExtension::Legacy {
            flags: LegacyExtensionFlags::Absent {},
        },
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
fn an_offset_extension_layout_carries_only_the_keys_its_own_arm_owns() {
    use crate::geometry::{
        RevisionCacheForm, RevisionSurfaceForm, RevisionSurfaceParameterization,
    };

    let form = RevisionSurfaceForm {
        revision: 1,
        support_bounds: [None; 4],
        reference_endpoints: [None; 2],
        second_endpoints: [None; 2],
        flags: [true, false, true, false],
        cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization::default()),
        discontinuities: Default::default(),
        tail_flag: false,
        trailing_flags: Vec::new(),
    };
    let offset = |extension| {
        OffsetSurfaceConstruction::try_new(support(), 1.0, None, None, false, extension).unwrap()
    };
    let legacy = offset(OffsetExtension::Legacy {
        flags: LegacyExtensionFlags::Enabled {
            secondary: true,
            tertiary: None,
        },
    });
    let revision = offset(OffsetExtension::Revision { form: form.clone() });

    let legacy_wire = serde_json::to_value(&legacy).unwrap();
    assert_eq!(legacy_wire["layout"], serde_json::json!("legacy"));
    assert_eq!(
        legacy_wire["flags"],
        serde_json::json!({"state": "enabled", "secondary": true})
    );
    assert!(legacy_wire.get("extension_flags").is_none());
    let revision_wire = serde_json::to_value(&revision).unwrap();
    assert_eq!(revision_wire["layout"], serde_json::json!("revision"));
    assert!(revision_wire.get("extension_flags").is_none());
    assert!(revision_wire.get("revision_form").is_none());
    assert_eq!(
        serde_json::from_value::<OffsetSurfaceConstruction>(legacy_wire.clone()).unwrap(),
        legacy
    );
    assert_eq!(
        serde_json::from_value::<OffsetSurfaceConstruction>(revision_wire.clone()).unwrap(),
        revision
    );

    let mut legacy_with_form = legacy_wire;
    legacy_with_form["flags"] = serde_json::json!({"state": "absent"});
    legacy_with_form["form"] = serde_json::to_value(&form).unwrap();
    let error = serde_json::from_value::<OffsetSurfaceConstruction>(legacy_with_form)
        .unwrap_err()
        .to_string();
    assert!(error.contains("form"), "{error}");

    let error = serde_json::from_value::<LegacyExtensionFlags>(
        serde_json::json!({"state": "disabled", "secondary": true}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown field `secondary`"), "{error}");

    for wire in [
        serde_json::json!({"state": "absent"}),
        serde_json::json!({"state": "disabled"}),
        serde_json::json!({"state": "enabled", "secondary": false}),
        serde_json::json!({"state": "enabled", "secondary": false, "tertiary": true}),
    ] {
        let flags = serde_json::from_value::<LegacyExtensionFlags>(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(flags).unwrap(), wire);
    }

    let mut short_run = revision_wire;
    short_run["form"]["flags"] = serde_json::json!([true, true, true]);
    let error = serde_json::from_value::<OffsetSurfaceConstruction>(short_run)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("invalid length 3, expected an array of length 4"),
        "{error}"
    );
}

#[test]
fn an_exact_spline_layout_carries_only_the_keys_its_own_arm_owns() {
    use super::ExactSurfacePayload;
    use crate::geometry::{
        ExactSpline, RevisionCacheForm, RevisionSurfaceForm, RevisionSurfaceParameterization,
    };

    let form = RevisionSurfaceForm {
        revision: 1,
        support_bounds: [None; 4],
        reference_endpoints: [None; 2],
        second_endpoints: [None; 2],
        flags: Vec::new(),
        cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization::default()),
        discontinuities: Default::default(),
        tail_flag: false,
        trailing_flags: Vec::new(),
    };
    let legacy = ExactSurfacePayload::try_new(ExactSpline::Legacy {
        ranges: [[0.0, 1.0]; 2],
        extension: 0,
    })
    .unwrap();
    let revision = ExactSurfacePayload::try_new(ExactSpline::Revision {
        intervals: [[Some(0.0), Some(1.0)]; 2],
        extension: 0,
        form: form.clone(),
    })
    .unwrap();

    let legacy_wire = serde_json::to_value(&legacy).unwrap();
    assert_eq!(legacy_wire["layout"], serde_json::json!("legacy"));
    assert!(legacy_wire.get("parameters").is_none());
    assert!(legacy_wire.get("revision_form").is_none());
    let revision_wire = serde_json::to_value(&revision).unwrap();
    assert_eq!(revision_wire["layout"], serde_json::json!("revision"));
    assert!(revision_wire.get("form").is_some());
    assert_eq!(
        serde_json::from_value::<ExactSurfacePayload>(legacy_wire.clone()).unwrap(),
        legacy
    );
    assert_eq!(
        serde_json::from_value::<ExactSurfacePayload>(revision_wire.clone()).unwrap(),
        revision
    );

    let mut legacy_with_form = legacy_wire;
    legacy_with_form["form"] = serde_json::to_value(&form).unwrap();
    let error = serde_json::from_value::<ExactSurfacePayload>(legacy_with_form)
        .unwrap_err()
        .to_string();
    assert!(error.contains("form"), "{error}");

    let mut revision_without_form = revision_wire;
    revision_without_form
        .as_object_mut()
        .unwrap()
        .remove("form");
    let error = serde_json::from_value::<ExactSurfacePayload>(revision_without_form)
        .unwrap_err()
        .to_string();
    assert!(error.contains("form"), "{error}");
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
    invalid["ranges"][0] = serde_json::json!([2.0, 1.0]);
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
                            path: None,
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
