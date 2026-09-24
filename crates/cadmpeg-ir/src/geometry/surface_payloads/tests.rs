// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;

use super::{OffsetSurfaceConstruction, SubSurfaceConstruction, SubsetSurfaceConstruction};
use crate::geometry::{LegacyExtensionFlags, OffsetExtension, ProceduralSurfaceDefinition};
use crate::ids::SurfaceId;

fn support() -> SurfaceId {
    SurfaceId::mint("synthetic:test:surface#support").unwrap()
}

/// The positive serializer-revision lane at `pointer` in an admitted
/// definition's wire refuses `0` and `-1` and admits `i64::MAX`.
fn assert_positive_revision_lane(wire: &serde_json::Value, pointer: &str) {
    assert!(
        wire.pointer(pointer)
            .is_some_and(|revision| revision.as_i64().is_some_and(|value| value > 0)),
        "{pointer}"
    );
    for revision in [0_i64, -1] {
        assert!(crate::scalar::PositiveI64::new(revision).is_none());
        let mut invalid = wire.clone();
        *invalid.pointer_mut(pointer).unwrap() = serde_json::json!(revision);
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(invalid).is_err());
    }
    let mut maximum = wire.clone();
    *maximum.pointer_mut(pointer).unwrap() = serde_json::json!(i64::MAX);
    let parsed = serde_json::from_value::<ProceduralSurfaceDefinition>(maximum).unwrap();
    assert_eq!(
        serde_json::to_value(parsed).unwrap().pointer(pointer),
        Some(&serde_json::json!(i64::MAX))
    );
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
    assert!(SubsetSurfaceConstruction::try_new(support(), ranges, None, None, None).is_err());
    assert!(SubsetSurfaceConstruction::try_new(
        support(),
        [[2.0, 0.0], [3.0, 1.0]],
        None,
        None,
        None
    )
    .is_ok());
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
            cache: None,
        },
    )
    .unwrap();
    let before = payload.clone();
    for distance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!({
            let replacement = distance;
            edit::replace(&mut payload, |previous| {
                crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    previous.support().clone(),
                    replacement,
                    *previous.u_sense(),
                    *previous.v_sense(),
                    previous.linear_support_extension(),
                    previous.extension().clone(),
                )
            })
        }
        .is_err());
        assert_eq!(payload, before);
    }
    {
        let replacement = 0.0;
        edit::replace(&mut payload, |previous| {
            crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                previous.support().clone(),
                replacement,
                *previous.u_sense(),
                *previous.v_sense(),
                previous.linear_support_extension(),
                previous.extension().clone(),
            )
        })
    }
    .unwrap();
    assert_eq!(payload.distance().get(), 0.0);
}

#[test]
fn an_offset_extension_layout_carries_only_the_keys_its_own_arm_owns() {
    use crate::geometry::{
        RevisionCacheForm, RevisionSurfaceForm, RevisionSurfaceParameterization,
    };

    let form = RevisionSurfaceForm {
        revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
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
        cache: None,
    });
    let revision = offset(OffsetExtension::Revision { form: form.clone() });

    let legacy_wire = serde_json::to_value(&legacy).unwrap();
    assert_eq!(
        legacy_wire["extension"]["layout"],
        serde_json::json!("legacy")
    );
    assert_eq!(
        legacy_wire["extension"]["flags"],
        serde_json::json!({"state": "enabled", "secondary": true})
    );
    assert!(legacy_wire.get("extension_flags").is_none());
    let revision_wire = serde_json::to_value(&revision).unwrap();
    assert_eq!(
        revision_wire["extension"]["layout"],
        serde_json::json!("revision")
    );
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
    legacy_with_form["extension"]["flags"] = serde_json::json!({"state": "absent"});
    legacy_with_form["extension"]["form"] = serde_json::to_value(&form).unwrap();
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
    short_run["extension"]["form"]["flags"] = serde_json::json!([true, true, true]);
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
        revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
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
        cache: None,
    })
    .unwrap();
    let revision = ExactSurfacePayload::try_new(ExactSpline::Revision {
        intervals: [[Some(0.0), Some(1.0)]; 2],
        extension: 0,
        form: form.clone(),
    })
    .unwrap();

    let legacy_wire = serde_json::to_value(&legacy).unwrap();
    assert_eq!(legacy_wire["spline"]["layout"], serde_json::json!("legacy"));
    assert!(legacy_wire.get("parameters").is_none());
    assert!(legacy_wire.get("revision_form").is_none());
    let revision_wire = serde_json::to_value(&revision).unwrap();
    assert_eq!(
        revision_wire["spline"]["layout"],
        serde_json::json!("revision")
    );
    assert!(revision_wire["spline"].get("form").is_some());
    assert_eq!(
        serde_json::from_value::<ExactSurfacePayload>(legacy_wire.clone()).unwrap(),
        legacy
    );
    assert_eq!(
        serde_json::from_value::<ExactSurfacePayload>(revision_wire.clone()).unwrap(),
        revision
    );

    let mut legacy_with_form = legacy_wire;
    legacy_with_form["spline"]["form"] = serde_json::to_value(&form).unwrap();
    let error = serde_json::from_value::<ExactSurfacePayload>(legacy_with_form)
        .unwrap_err()
        .to_string();
    assert!(error.contains("form"), "{error}");

    let mut revision_without_form = revision_wire;
    revision_without_form["spline"]
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
            cache: None,
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
    invalid["spline"]["ranges"][0] = serde_json::json!([2.0, 1.0]);
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(invalid).is_err());
    assert!(CompoundSurfacePayload::try_new(Vec::new(), None).is_ok());
    for parameter in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(CompoundSurfacePayload::try_new(
            vec![CompoundComponent {
                parameter,
                component: support(),
            }],
            None
        )
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
            crate::geometry::CacheContract::from_form(None),
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

#[test]
fn a_compound_loft_scale_member_direction_is_refused_by_the_payload_admission() {
    use super::CompoundLoftSurfacePayload;
    use crate::geometry::{
        ClassicLoftProfileData, CompoundLoftConstruction, CompoundLoftDirection, CompoundLoftScale,
        CompoundLoftScaleMember, CompoundLoftScales, CompoundLoftTail, LoftSubdata,
    };
    use crate::math::Vector3;

    let construction = |direction| {
        Box::new(CompoundLoftConstruction {
            scales: CompoundLoftScales::try_new(vec![CompoundLoftScale {
                members: vec![CompoundLoftScaleMember {
                    type_code: 0,
                    curve: "test:model:curve#member".try_into().unwrap(),
                    data: ClassicLoftProfileData {
                        surface: support(),
                        pcurve: None,
                        first_flag: false,
                        asm_extension: 0,
                        subdata: LoftSubdata::Type211 {
                            dimensions: [1, 0],
                            row: [0.0, 1.0],
                        },
                        direction,
                    },
                }],
                path: "test:model:curve#path".try_into().unwrap(),
                auxiliaries: Vec::new(),
                tail: [0, 0],
            }])
            .unwrap(),
            flags: [false; 2],
            tail: CompoundLoftTail::Zero {
                flags: [false; 2],
                direction: CompoundLoftDirection::Vector {
                    value: Vector3::new(0.0, 0.0, 1.0),
                },
                trailing_flags: [false; 2],
            },
        })
    };
    let admitted =
        CompoundLoftSurfacePayload::try_new(construction(Some(Vector3::new(0.0, 0.0, 1.0))), None)
            .unwrap();
    let definition = ProceduralSurfaceDefinition::CompoundLoft(admitted);
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
    assert!(CompoundLoftSurfacePayload::try_new(construction(None), None).is_ok());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(CompoundLoftSurfacePayload::try_new(
            construction(Some(Vector3::new(value, 0.0, 1.0))),
            None
        )
        .is_err());
    }
    // JSON states no infinity or NaN, so the wire cannot spell the refused
    // direction; the admission above is the route that reads it.
    assert_eq!(
        wire["construction"]["scales"][0]["members"][0]["data"]["direction"],
        serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0})
    );
}

#[test]
fn a_loft_member_form_direction_is_refused_by_the_loft_and_net_payload_admissions() {
    use super::{
        LoftSurfacePayload, LoftSurfacePayloadWire, NetSurfacePayload, NetSurfacePayloadWire,
    };
    use crate::geometry::{
        CacheContract, LawFormula, LoftMemberForm, LoftPath, LoftPathCurve, LoftProfileMember,
        LoftSection, LoftSectionEntry, LoftSubdata, NetSurfaceConstruction,
        SplineSurfaceParameters,
    };
    use crate::ids::CurveId;
    use crate::math::Vector3;

    let subdata = || LoftSubdata::Type211 {
        dimensions: [1, 0],
        row: [0.0, 1.0],
    };
    let member = |form| LoftProfileMember {
        profile: LoftPathCurve {
            id: CurveId::mint("synthetic:test:curve#profile").unwrap(),
            endpoints: None,
        },
        form,
    };
    // Both forms carry the field, so both are exercised in every section.
    let sections = |support_direction, pair_direction| {
        [
            LoftSection {
                entries: vec![LoftSectionEntry {
                    parameter: 0.0,
                    profile: vec![
                        member(LoftMemberForm::Support {
                            type_code: 1,
                            surface: None,
                            support_bounds: [None; 4],
                            pcurve: None,
                            first_flag: false,
                            asm_extension: None,
                            subdata: subdata(),
                            direction: support_direction,
                        }),
                        member(LoftMemberForm::PcurvePair {
                            pcurve: None,
                            secondary_pcurve: None,
                            asm_extension: None,
                            subdata: subdata(),
                            direction: pair_direction,
                        }),
                    ],
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
        ]
    };
    let parameters = || SplineSurfaceParameters::OrderedRanges {
        ranges: [[0.0, 1.0], [0.0, 1.0]],
    };
    let loft = |support_direction, pair_direction| {
        LoftSurfacePayload::try_new(
            sections(support_direction, pair_direction),
            parameters(),
            [0; 2],
            [0; 2],
            0,
            Vec::new(),
            CacheContract::from_form(None),
        )
    };
    let loft_wire = |support_direction, pair_direction| {
        LoftSurfacePayload::try_from(LoftSurfacePayloadWire {
            sections: sections(support_direction, pair_direction),
            parameters: parameters(),
            closures: [0; 2],
            singularities: [0; 2],
            mode: 0,
            bridge: Vec::new(),
            cache: CacheContract::from_form(None),
        })
    };
    let construction = |support_direction, pair_direction| {
        Box::new(NetSurfaceConstruction {
            sections: Box::new(sections(support_direction, pair_direction)),
            frame_parameters: [0.0; 12],
            flag: 0,
            directions: [Vector3::new(0.0, 0.0, 1.0); 4],
            formulas: Box::new(std::array::from_fn(|_| LawFormula::Null {})),
            discontinuities: Default::default(),
            discontinuity_flag: false,
        })
    };
    let net = |support_direction, pair_direction| {
        NetSurfacePayload::try_new(construction(support_direction, pair_direction), None)
    };
    let net_wire = |support_direction, pair_direction| {
        NetSurfacePayload::try_from(NetSurfacePayloadWire {
            construction: construction(support_direction, pair_direction),
            cache: None,
        })
    };

    let admitted = Vector3::new(0.0, 0.0, 1.0);
    let definition = ProceduralSurfaceDefinition::Loft(loft(Some(admitted), None).unwrap());
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire.clone()).unwrap(),
        definition
    );
    // The admitted direction is the only one the profile writes, and the
    // absent one writes no key at all.
    let profile = &wire["sections"][0]["entries"][0]["profile"];
    assert_eq!(
        profile[0]["form"]["direction"],
        serde_json::json!({"x": 0.0, "y": 0.0, "z": 1.0})
    );
    assert_eq!(profile[1]["form"].get("direction"), None);

    assert!(loft(None, None).is_ok());
    assert!(loft_wire(None, None).is_ok());
    assert!(net(None, None).is_ok());
    assert!(net_wire(None, None).is_ok());
    assert!(loft(Some(admitted), Some(admitted)).is_ok());
    assert!(net(Some(admitted), Some(admitted)).is_ok());

    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused direction; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let degenerate = Vector3::new(value, 0.0, 1.0);
        assert!(loft(Some(degenerate), None).is_err());
        assert!(loft(None, Some(degenerate)).is_err());
        assert!(loft_wire(Some(degenerate), None).is_err());
        assert!(loft_wire(None, Some(degenerate)).is_err());
        assert!(net(Some(degenerate), None).is_err());
        assert!(net(None, Some(degenerate)).is_err());
        assert!(net_wire(Some(degenerate), None).is_err());
        assert!(net_wire(None, Some(degenerate)).is_err());
    }
}

#[test]
fn the_loft_and_net_admissions_refuse_a_non_finite_section_or_cache_scalar() {
    use super::{
        LoftSurfacePayload, LoftSurfacePayloadWire, NetSurfacePayload, NetSurfacePayloadWire,
    };
    use crate::geometry::{
        CacheContract, LawFormula, LoftMemberForm, LoftPath, LoftPathCurve, LoftProfileMember,
        LoftRevisionForm, LoftSection, LoftSectionEntry, LoftSubdata, LoftSubdataRow,
        NetSurfaceConstruction, RevisionCacheForm, RevisionSurfaceParameterization,
        SplineSurfaceParameters,
    };
    use crate::ids::CurveId;
    use crate::math::Vector3;

    // One value per float field family the two admissions reach and the
    // predecessor left unrefused.
    #[derive(Clone, Copy)]
    struct Fields {
        extra: Option<[f64; 2]>,
        support_bounds: [Option<f64>; 4],
        profile_endpoints: Option<[Option<f64>; 2]>,
        path_endpoints: Option<[Option<f64>; 2]>,
        u_interval: [Option<f64>; 2],
        v_interval: [Option<f64>; 2],
        discontinuity: f64,
    }
    let admitted = Fields {
        extra: Some([0.0, 1.0]),
        support_bounds: [Some(0.0), Some(1.0), None, Some(2.0)],
        profile_endpoints: Some([Some(0.0), None]),
        path_endpoints: Some([None, Some(1.0)]),
        u_interval: [Some(0.0), Some(1.0)],
        v_interval: [None, Some(2.0)],
        discontinuity: 3.0,
    };
    let curve = || CurveId::mint("synthetic:test:curve#profile").unwrap();
    // `extra` is stored by the table form only; type 211 has no row to carry it.
    let sections = |fields: Fields| {
        [
            LoftSection {
                entries: vec![LoftSectionEntry {
                    parameter: 0.0,
                    profile: vec![LoftProfileMember {
                        profile: LoftPathCurve {
                            id: curve(),
                            endpoints: fields.profile_endpoints,
                        },
                        form: LoftMemberForm::Support {
                            type_code: 1,
                            surface: None,
                            support_bounds: fields.support_bounds,
                            pcurve: None,
                            first_flag: false,
                            asm_extension: None,
                            subdata: LoftSubdata::table(
                                3,
                                vec![LoftSubdataRow {
                                    parameters: [0.0, 1.0],
                                    columns: Vec::new(),
                                    extra: fields.extra,
                                }],
                            )
                            .unwrap(),
                            direction: None,
                        },
                    }],
                    path: LoftPath {
                        path: Some(LoftPathCurve {
                            id: curve(),
                            endpoints: fields.path_endpoints,
                        }),
                        auxiliaries: Vec::new(),
                        flag: 0,
                    },
                }],
            },
            LoftSection {
                entries: Vec::new(),
            },
        ]
    };
    let cache = |fields: Fields| {
        CacheContract::from_form(Some(LoftRevisionForm {
            revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
            flags: [false; 4],
            ints: [0; 2],
            cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization {
                u_interval: fields.u_interval,
                v_interval: fields.v_interval,
                u_closure: 0,
                v_closure: 0,
                u_singularity: 0,
                v_singularity: 0,
            }),
            discontinuities: [
                vec![fields.discontinuity],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            tail_flag: false,
        }))
    };
    let parameters = || SplineSurfaceParameters::OrderedRanges {
        ranges: [[0.0, 1.0], [0.0, 1.0]],
    };
    let loft = |fields: Fields| {
        LoftSurfacePayload::try_new(
            sections(fields),
            parameters(),
            [0; 2],
            [0; 2],
            0,
            Vec::new(),
            cache(fields),
        )
    };
    let loft_wire = |fields: Fields| {
        LoftSurfacePayload::try_from(LoftSurfacePayloadWire {
            sections: sections(fields),
            parameters: parameters(),
            closures: [0; 2],
            singularities: [0; 2],
            mode: 0,
            bridge: Vec::new(),
            cache: cache(fields),
        })
    };
    // The net construction states no revision cache form, so it carries the
    // section fields only.
    let construction = |fields: Fields| {
        Box::new(NetSurfaceConstruction {
            sections: Box::new(sections(fields)),
            frame_parameters: [0.0; 12],
            flag: 0,
            directions: [Vector3::new(0.0, 0.0, 1.0); 4],
            formulas: Box::new(std::array::from_fn(|_| LawFormula::Null {})),
            discontinuities: Default::default(),
            discontinuity_flag: false,
        })
    };
    let net = |fields: Fields| NetSurfacePayload::try_new(construction(fields), None);
    let net_wire = |fields: Fields| {
        NetSurfacePayload::try_from(NetSurfacePayloadWire {
            construction: construction(fields),
            cache: None,
        })
    };

    let definition = ProceduralSurfaceDefinition::Loft(loft(admitted).unwrap());
    let wire = serde_json::to_value(&definition).unwrap();
    assert_positive_revision_lane(&wire, "/cache/form/revision");
    let entry = &wire["sections"][0]["entries"][0];
    let member = &entry["profile"][0];
    assert_eq!(
        member["form"]["subdata"]["rows"][0]["extra"],
        serde_json::json!([0.0, 1.0])
    );
    assert_eq!(
        member["form"]["support_bounds"],
        serde_json::json!([0.0, 1.0, null, 2.0])
    );
    assert_eq!(
        member["profile"]["endpoints"],
        serde_json::json!([0.0, null])
    );
    assert_eq!(
        entry["path"]["path"]["endpoints"],
        serde_json::json!([null, 1.0])
    );
    assert_eq!(
        wire["cache"]["form"]["cache"]["u_interval"],
        serde_json::json!([0.0, 1.0])
    );
    assert_eq!(
        wire["cache"]["form"]["cache"]["v_interval"],
        serde_json::json!([null, 2.0])
    );
    assert_eq!(
        wire["cache"]["form"]["discontinuities"][0],
        serde_json::json!([3.0])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    assert!(loft_wire(admitted).is_ok());
    assert!(net(admitted).is_ok());
    assert!(net_wire(admitted).is_ok());

    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                extra: Some([value, 1.0]),
                ..admitted
            },
            Fields {
                support_bounds: [Some(0.0), None, Some(value), None],
                ..admitted
            },
            Fields {
                profile_endpoints: Some([Some(value), None]),
                ..admitted
            },
            Fields {
                path_endpoints: Some([None, Some(value)]),
                ..admitted
            },
        ] {
            assert!(loft(fields).is_err());
            assert!(loft_wire(fields).is_err());
            assert!(net(fields).is_err());
            assert!(net_wire(fields).is_err());
        }
        for fields in [
            Fields {
                u_interval: [Some(value), None],
                ..admitted
            },
            Fields {
                v_interval: [None, Some(value)],
                ..admitted
            },
            Fields {
                discontinuity: value,
                ..admitted
            },
        ] {
            assert!(loft(fields).is_err());
            assert!(loft_wire(fields).is_err());
        }
    }
}

#[test]
fn the_blend_admissions_refuse_every_non_finite_rolling_ball_scalar() {
    use super::{
        BlendSurfacePayload, BlendSurfacePayloadWire, VariableBlendSurfacePayload,
        VariableBlendSurfacePayloadWire,
    };
    use crate::geometry::{
        BlendCrossSection, BlendRadiusLaw, CacheContract, RevisionCacheForm,
        RevisionSurfaceParameterization, RollingBallConstruction, RollingBallRadiusSelector,
        RollingBallSide, RollingBallSupportCurve, RollingBallSupportSurface, VariableBlendCache,
        VariableBlendConstruction, VariableBlendConvexity, VariableBlendRadii,
        VariableBlendRenderMode, VariableBlendSupportKind, VariableBlendSurfaceSubtype,
        VariableBlendValue, VariableBlendValuePayload,
    };
    use crate::ids::{CurveId, SurfaceId};
    use crate::math::Point3;

    // One value per float field family the two admissions reach and the
    // predecessor left unrefused.
    #[derive(Clone, Copy)]
    struct Fields {
        side_surface_range: [[Option<f64>; 2]; 2],
        side_curve_range: [Option<f64>; 2],
        slice_range: [Option<f64>; 2],
        u_interval: [Option<f64>; 2],
        discontinuity: f64,
    }
    let admitted = Fields {
        side_surface_range: [[Some(0.0), None], [None, Some(1.0)]],
        side_curve_range: [None, Some(2.0)],
        slice_range: [Some(3.0), None],
        u_interval: [Some(4.0), None],
        discontinuity: 5.0,
    };
    let curve = || CurveId::mint("synthetic:test:curve#blend").unwrap();
    let surface = || SurfaceId::mint("synthetic:test:surface#blend").unwrap();
    let side = |fields: Fields| RollingBallSide {
        support_kind: VariableBlendSupportKind::Surface,
        surface: Some(RollingBallSupportSurface {
            surface: surface(),
            parameter_ranges: fields.side_surface_range,
        }),
        curve: Some(RollingBallSupportCurve {
            curve: curve(),
            parameter_range: fields.side_curve_range,
        }),
        pcurve: None,
        location: Point3::new(0.0, 0.0, 0.0),
        secondary_pcurve: None,
        extension: None,
    };
    let parameterization = |fields: Fields| RevisionSurfaceParameterization {
        u_interval: fields.u_interval,
        v_interval: [None, None],
        u_closure: 0,
        v_closure: 0,
        u_singularity: 0,
        v_singularity: 0,
    };
    let discontinuities = |fields: Fields| {
        [
            vec![fields.discontinuity],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ]
    };

    let variable = |fields: Fields| {
        Box::new(VariableBlendConstruction {
            subtype: VariableBlendSurfaceSubtype::VariableBlend,
            revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
            sides: Box::new([side(fields), side(admitted)]),
            slice: curve(),
            slice_range: fields.slice_range,
            offsets: [0.0, 0.0],
            radii: VariableBlendRadii::Single {
                value: VariableBlendValue {
                    modern_flag: false,
                    calibrated: 0,
                    payload: VariableBlendValuePayload::TwoEnds {
                        discriminator: 0,
                        parameters: [0.0, 1.0],
                        radii: [1.0, 1.0],
                    },
                },
            },
            cross_section: None,
            u_range: [0.0, 1.0],
            v_lower: None,
            shape_parameter: 0.0,
            shape_length: 0.0,
            shape_tail: 0,
            cache: VariableBlendCache::Parameterization {
                shape_prefix: 0,
                parameterization: parameterization(fields),
            },
            discontinuities: discontinuities(fields),
            tail_flag: false,
            tail_extensions: [0; 3],
            secondary_curve: None,
            convexity: VariableBlendConvexity::Convex,
            render_mode: VariableBlendRenderMode::RollingBallEnvelope,
            post_range: [None, None],
            post_curve: None,
            post_pcurve: None,
        })
    };
    let variable_new = |fields: Fields| VariableBlendSurfacePayload::try_new(variable(fields));
    let variable_wire = |fields: Fields| {
        VariableBlendSurfacePayload::try_from(VariableBlendSurfacePayloadWire {
            construction: variable(fields),
        })
    };

    let rolling = |fields: Fields| {
        CacheContract::from_form(Some(Box::new(RollingBallConstruction {
            revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
            sides: Box::new([side(fields), side(admitted)]),
            slice: curve(),
            slice_range: fields.slice_range,
            offsets: [0.0, 0.0],
            radius_selector: RollingBallRadiusSelector::None {},
            u_range: [None, None],
            v_range: [None, None],
            shape_prefix: 0,
            parameters: [0.0, 0.0],
            tail: 0,
            cache: RevisionCacheForm::Parameterization(parameterization(fields)),
            discontinuities: discontinuities(fields),
            tail_flag: false,
            third: None,
            tail_extensions: [0; 3],
        })))
    };
    let blend_new = |fields: Fields| {
        BlendSurfacePayload::try_new(
            [None, None],
            None,
            BlendRadiusLaw::constant(1.0).unwrap(),
            BlendCrossSection::Circular,
            rolling(fields),
        )
    };
    let blend_wire = |fields: Fields| {
        BlendSurfacePayload::try_from(BlendSurfacePayloadWire {
            supports: [None, None],
            spine: None,
            radius: BlendRadiusLaw::constant(1.0).unwrap(),
            cross_section: BlendCrossSection::Circular,
            cache: rolling(fields),
        })
    };

    let definition = ProceduralSurfaceDefinition::VariableBlend(variable_new(admitted).unwrap());
    let wire = serde_json::to_value(&definition).unwrap();
    assert_positive_revision_lane(&wire, "/construction/revision");
    let first_side = &wire["construction"]["sides"][0];
    assert_eq!(
        first_side["surface"]["parameter_ranges"],
        serde_json::json!([[0.0, null], [null, 1.0]])
    );
    assert_eq!(
        first_side["curve"]["parameter_range"],
        serde_json::json!([null, 2.0])
    );
    assert_eq!(
        wire["construction"]["slice_range"],
        serde_json::json!([3.0, null])
    );
    assert_eq!(
        wire["construction"]["cache"]["parameterization"]["u_interval"],
        serde_json::json!([4.0, null])
    );
    assert_eq!(
        wire["construction"]["discontinuities"][0],
        serde_json::json!([5.0])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    assert!(variable_wire(admitted).is_ok());
    assert!(blend_new(admitted).is_ok());
    assert!(blend_wire(admitted).is_ok());
    let blend = ProceduralSurfaceDefinition::Blend(blend_new(admitted).unwrap());
    assert_positive_revision_lane(
        &serde_json::to_value(&blend).unwrap(),
        "/cache/form/revision",
    );

    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                side_surface_range: [[Some(value), None], [None, Some(1.0)]],
                ..admitted
            },
            Fields {
                side_curve_range: [None, Some(value)],
                ..admitted
            },
            Fields {
                slice_range: [Some(value), None],
                ..admitted
            },
            Fields {
                u_interval: [Some(value), None],
                ..admitted
            },
            Fields {
                discontinuity: value,
                ..admitted
            },
        ] {
            assert!(variable_new(fields).is_err());
            assert!(variable_wire(fields).is_err());
            assert!(blend_new(fields).is_err());
            assert!(blend_wire(fields).is_err());
        }
    }
}

#[test]
fn the_revision_gated_surface_admissions_refuse_every_non_finite_form_scalar() {
    use super::{
        admit_revolution_axis, DeformableSurfacePayload, DeformableSurfacePayloadWire,
        ExactSurfacePayload, ExactSurfacePayloadWire, ExtrusionSurfaceConstruction,
        ExtrusionSurfaceConstructionWire, RevolutionSurfaceConstruction,
        RevolutionSurfaceConstructionWire, SumSurfaceConstruction, SumSurfaceConstructionWire,
        TaperSurfaceConstruction, TaperSurfaceConstructionWire,
    };
    use crate::features::FinitePoint3;
    use crate::geometry::{
        CacheContract, DeformableSurfaceConstruction, DeformableSurfaceData, ExactSpline,
        RevisionCacheForm, RevisionSurfaceForm, RevisionSurfaceParameterization, TaperSurfaceKind,
    };
    use crate::ids::CurveId;
    use crate::math::{Point3, Vector3};
    use crate::units::UnitVector3;

    // One value per float field family the shared revision-gated form carries.
    #[derive(Clone, Copy)]
    struct Fields {
        support_bounds: [Option<f64>; 4],
        reference_endpoints: [Option<f64>; 2],
        second_endpoints: [Option<f64>; 2],
        u_interval: [Option<f64>; 2],
        discontinuity: f64,
    }
    let admitted = Fields {
        support_bounds: [Some(0.0), None, Some(1.0), None],
        reference_endpoints: [Some(2.0), None],
        second_endpoints: [None, Some(3.0)],
        u_interval: [Some(4.0), None],
        discontinuity: 5.0,
    };
    let form = |fields: Fields| RevisionSurfaceForm {
        revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
        support_bounds: fields.support_bounds,
        reference_endpoints: fields.reference_endpoints,
        second_endpoints: fields.second_endpoints,
        flags: Vec::new(),
        cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization {
            u_interval: fields.u_interval,
            v_interval: [None, None],
            u_closure: 0,
            v_closure: 0,
            u_singularity: 0,
            v_singularity: 0,
        }),
        discontinuities: [
            vec![fields.discontinuity],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ],
        tail_flag: false,
        trailing_flags: Vec::new(),
    };
    let offset_form = |fields: Fields| RevisionSurfaceForm::<[bool; 4]> {
        revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
        support_bounds: fields.support_bounds,
        reference_endpoints: fields.reference_endpoints,
        second_endpoints: fields.second_endpoints,
        flags: [false; 4],
        cache: form(fields).cache,
        discontinuities: form(fields).discontinuities,
        tail_flag: false,
        trailing_flags: Vec::new(),
    };
    let cache = |fields: Fields| CacheContract::from_form(Some(form(fields)));
    let curve = || CurveId::mint("synthetic:test:curve#revision").unwrap();

    let taper = |fields: Fields| {
        TaperSurfaceConstruction::try_new(
            support(),
            curve(),
            None,
            0.0,
            TaperSurfaceKind::Standard {},
            cache(fields),
        )
    };
    let taper_wire = |fields: Fields| {
        TaperSurfaceConstruction::try_from(TaperSurfaceConstructionWire {
            support: support(),
            reference: curve(),
            pcurve: None,
            parameter: 0.0,
            taper: TaperSurfaceKind::Standard {},
            cache: cache(fields),
        })
    };
    let extrusion = |fields: Fields| {
        ExtrusionSurfaceConstruction::try_new(
            curve(),
            None,
            Vector3::new(0.0, 0.0, 1.0),
            None,
            cache(fields),
        )
    };
    let extrusion_wire = |fields: Fields| {
        ExtrusionSurfaceConstruction::try_from(ExtrusionSurfaceConstructionWire {
            directrix: curve(),
            parameter_interval: None,
            direction: Vector3::new(0.0, 0.0, 1.0),
            native_position: None,
            cache: cache(fields),
        })
    };
    let revolution = |fields: Fields| {
        RevolutionSurfaceConstruction::try_new(
            curve(),
            (FinitePoint3::ZERO, UnitVector3::Z_AXIS),
            [0.0, 1.0],
            None,
            None,
            false,
            cache(fields),
        )
    };
    let revolution_wire = |fields: Fields| {
        RevolutionSurfaceConstruction::try_from(RevolutionSurfaceConstructionWire {
            directrix: curve(),
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            angular_interval: [0.0, 1.0],
            angular_parameter_interval: None,
            parameter_interval: None,
            transposed: false,
            cache: cache(fields),
        })
    };
    let sum = |fields: Fields| {
        SumSurfaceConstruction::try_new(
            curve(),
            curve(),
            Vector3::new(0.0, 0.0, 0.0),
            cache(fields),
        )
    };
    let sum_wire = |fields: Fields| {
        SumSurfaceConstruction::try_from(SumSurfaceConstructionWire {
            first: curve(),
            second: curve(),
            basepoint: Vector3::new(0.0, 0.0, 0.0),
            cache: cache(fields),
        })
    };
    let offset = |fields: Fields| {
        OffsetSurfaceConstruction::try_new(
            support(),
            1.0,
            None,
            None,
            false,
            OffsetExtension::Revision {
                form: offset_form(fields),
            },
        )
    };
    let spline = |fields: Fields| ExactSpline::Revision {
        intervals: [[Some(0.0), None], [None, Some(1.0)]],
        extension: 0,
        form: form(fields),
    };
    let exact = |fields: Fields| ExactSurfacePayload::try_new(spline(fields));
    let exact_wire = |fields: Fields| {
        ExactSurfacePayload::try_from(ExactSurfacePayloadWire {
            spline: spline(fields),
        })
    };
    let deformable_construction = |fields: Fields| {
        Box::new(DeformableSurfaceConstruction {
            support: support(),
            data: DeformableSurfaceData::Minimal {
                vectors: [Vector3::new(0.0, 0.0, 1.0); 4],
                selector: 0,
            },
            cache: cache(fields),
            discontinuities: Default::default(),
            discontinuity_flag: false,
        })
    };
    let deformable =
        |fields: Fields| DeformableSurfacePayload::try_new(deformable_construction(fields));
    let deformable_wire = |fields: Fields| {
        DeformableSurfacePayload::try_from(DeformableSurfacePayloadWire {
            construction: deformable_construction(fields),
        })
    };

    let definition = ProceduralSurfaceDefinition::Exact(exact(admitted).unwrap());
    let wire = serde_json::to_value(&definition).unwrap();
    assert_positive_revision_lane(&wire, "/spline/form/revision");
    let stored = &wire["spline"]["form"];
    assert_eq!(
        stored["support_bounds"],
        serde_json::json!([0.0, null, 1.0, null])
    );
    assert_eq!(
        stored["reference_endpoints"],
        serde_json::json!([2.0, null])
    );
    assert_eq!(stored["second_endpoints"], serde_json::json!([null, 3.0]));
    assert_eq!(
        stored["cache"]["u_interval"],
        serde_json::json!([4.0, null])
    );
    assert_eq!(stored["discontinuities"][0], serde_json::json!([5.0]));
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    for ok in [
        taper(admitted).is_ok(),
        taper_wire(admitted).is_ok(),
        extrusion(admitted).is_ok(),
        extrusion_wire(admitted).is_ok(),
        revolution(admitted).is_ok(),
        revolution_wire(admitted).is_ok(),
        sum(admitted).is_ok(),
        sum_wire(admitted).is_ok(),
        offset(admitted).is_ok(),
        exact_wire(admitted).is_ok(),
        deformable(admitted).is_ok(),
        deformable_wire(admitted).is_ok(),
    ] {
        assert!(ok);
    }

    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                support_bounds: [None, Some(value), None, None],
                ..admitted
            },
            Fields {
                reference_endpoints: [Some(value), None],
                ..admitted
            },
            Fields {
                second_endpoints: [None, Some(value)],
                ..admitted
            },
            Fields {
                u_interval: [Some(value), None],
                ..admitted
            },
            Fields {
                discontinuity: value,
                ..admitted
            },
        ] {
            for refused in [
                taper(fields).is_err(),
                taper_wire(fields).is_err(),
                extrusion(fields).is_err(),
                extrusion_wire(fields).is_err(),
                revolution(fields).is_err(),
                revolution_wire(fields).is_err(),
                sum(fields).is_err(),
                sum_wire(fields).is_err(),
                offset(fields).is_err(),
                exact(fields).is_err(),
                exact_wire(fields).is_err(),
                deformable(fields).is_err(),
                deformable_wire(fields).is_err(),
            ] {
                assert!(refused);
            }
        }

        // The revolution axis is admitted before `try_new`, which takes the
        // checked axis types, and no interval rule reads it.
        assert!(
            admit_revolution_axis(Point3::new(value, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0))
                .is_err()
        );
        assert!(
            RevolutionSurfaceConstruction::try_from(RevolutionSurfaceConstructionWire {
                directrix: curve(),
                axis_origin: Point3::new(0.0, 0.0, 0.0),
                axis_direction: Vector3::new(0.0, value, 0.0),
                angular_interval: [0.0, 1.0],
                angular_parameter_interval: None,
                parameter_interval: None,
                transposed: false,
                cache: cache(admitted),
            })
            .is_err()
        );
    }
}

#[test]
fn the_sweep_and_vertex_blend_admissions_refuse_their_remaining_optional_scalars() {
    use super::{
        SweepSurfacePayload, SweepSurfacePayloadWire, VertexBlendSurfacePayload,
        VertexBlendSurfacePayloadWire,
    };
    use crate::geometry::{
        CacheContract, FitTolerance, RevisionCacheForm, RevisionSurfaceParameterization,
        SweepRevisionForm, SweepSurfaceConstruction, SweepSurfaceLayout, VertexBlendBoundary,
        VertexBlendBoundaryGeometry, VertexBlendConstruction, VertexBlendTwists,
    };
    use crate::ids::CurveId;
    use crate::math::Vector3;

    // One value per float field family these two admissions left unrefused.
    #[derive(Clone, Copy)]
    struct Fields {
        profile_endpoints: [Option<f64>; 2],
        path_endpoints: [Option<f64>; 2],
        sweep_u_interval: [Option<f64>; 2],
        curve_endpoints: [Option<f64>; 2],
        boundary_support_bounds: [Option<f64>; 4],
    }
    let admitted = Fields {
        profile_endpoints: [Some(0.0), None],
        path_endpoints: [None, Some(1.0)],
        sweep_u_interval: [Some(2.0), None],
        curve_endpoints: [Some(3.0), None],
        boundary_support_bounds: [None, Some(4.0), None, None],
    };
    let curve = || CurveId::mint("synthetic:test:curve#sweep").unwrap();

    let sweep_construction = |fields: Fields| {
        Box::new(SweepSurfaceConstruction {
            primary_kind: 0,
            cache: CacheContract::from_form(Some(SweepRevisionForm {
                revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
                primary_flag: false,
                profile_endpoints: fields.profile_endpoints,
                path_endpoints: fields.path_endpoints,
                cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization {
                    u_interval: fields.sweep_u_interval,
                    v_interval: [None, None],
                    u_closure: 0,
                    v_closure: 0,
                    u_singularity: 0,
                    v_singularity: 0,
                }),
            })),
            layout: SweepSurfaceLayout::ProfileFirst {
                secondary_kind: 0,
                directions: [Vector3::new(0.0, 0.0, 1.0); 5],
                origin: crate::math::Point3::new(0.0, 0.0, 0.0),
                parameters: [0.0; 4],
                formulas: Box::new(std::array::from_fn(
                    |_| crate::geometry::LawFormula::Null {},
                )),
            },
            discontinuities: Default::default(),
            discontinuity_flag: false,
        })
    };
    let sweep = |fields: Fields| {
        SweepSurfacePayload::try_new(curve(), curve(), Some(sweep_construction(fields)))
    };
    let sweep_wire = |fields: Fields| {
        SweepSurfacePayload::try_from(SweepSurfacePayloadWire {
            profile: curve(),
            spine: curve(),
            native: Some(sweep_construction(fields)),
        })
    };

    let vertex_construction = |fields: Fields| {
        Box::new(VertexBlendConstruction {
            revision: Some(crate::scalar::PositiveI64::new(1).expect("positive revision")),
            boundaries: vec![
                VertexBlendBoundary {
                    boundary_type: false,
                    magic: Vector3::new(0.0, 0.0, 1.0),
                    u_smoothing: false,
                    v_smoothing: false,
                    fullness: 0.0,
                    geometry: VertexBlendBoundaryGeometry::Circle {
                        curve: curve(),
                        curve_endpoints: fields.curve_endpoints,
                        twists: VertexBlendTwists::None {},
                        parameters: [0.0, 1.0],
                        sense: false,
                    },
                },
                VertexBlendBoundary {
                    boundary_type: false,
                    magic: Vector3::new(0.0, 0.0, 1.0),
                    u_smoothing: false,
                    v_smoothing: false,
                    fullness: 0.0,
                    geometry: VertexBlendBoundaryGeometry::Pcurve {
                        surface: support(),
                        support_bounds: fields.boundary_support_bounds,
                        pcurve: None,
                        sense: false,
                        fit_tolerance: FitTolerance::try_new(0.0).unwrap(),
                    },
                },
                VertexBlendBoundary {
                    boundary_type: false,
                    magic: Vector3::new(0.0, 0.0, 1.0),
                    u_smoothing: false,
                    v_smoothing: false,
                    fullness: 0.0,
                    geometry: VertexBlendBoundaryGeometry::Plane {
                        normal: Vector3::new(0.0, 0.0, 1.0),
                        parameters: [0.0, 1.0],
                        curve: curve(),
                        curve_endpoints: fields.curve_endpoints,
                    },
                },
            ],
            grid_size: 0,
            fit_tolerance: FitTolerance::try_new(0.0).unwrap(),
        })
    };
    let vertex = |fields: Fields| VertexBlendSurfacePayload::try_new(vertex_construction(fields));
    let vertex_wire = |fields: Fields| {
        VertexBlendSurfacePayload::try_from(VertexBlendSurfacePayloadWire {
            construction: vertex_construction(fields),
        })
    };

    let definition = ProceduralSurfaceDefinition::Sweep(sweep(admitted).unwrap());
    let wire = serde_json::to_value(&definition).unwrap();
    assert_positive_revision_lane(&wire, "/native/cache/form/revision");
    assert_positive_revision_lane(
        &serde_json::to_value(ProceduralSurfaceDefinition::VertexBlend(
            vertex(admitted).unwrap(),
        ))
        .unwrap(),
        "/construction/revision",
    );
    let stored = &wire["native"]["cache"]["form"];
    assert_eq!(stored["profile_endpoints"], serde_json::json!([0.0, null]));
    assert_eq!(stored["path_endpoints"], serde_json::json!([null, 1.0]));
    assert_eq!(
        stored["cache"]["u_interval"],
        serde_json::json!([2.0, null])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    assert!(sweep_wire(admitted).is_ok());
    assert!(vertex(admitted).is_ok());
    assert!(vertex_wire(admitted).is_ok());

    // JSON itself states no infinity or NaN, so the wire cannot spell a
    // refused value; `TryFrom<…Wire>` is the conversion the deserializer
    // runs, and it is exercised directly here.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for fields in [
            Fields {
                profile_endpoints: [Some(value), None],
                ..admitted
            },
            Fields {
                path_endpoints: [None, Some(value)],
                ..admitted
            },
            Fields {
                sweep_u_interval: [Some(value), None],
                ..admitted
            },
        ] {
            assert!(sweep(fields).is_err());
            assert!(sweep_wire(fields).is_err());
        }
        for fields in [
            Fields {
                curve_endpoints: [Some(value), None],
                ..admitted
            },
            Fields {
                boundary_support_bounds: [None, Some(value), None, None],
                ..admitted
            },
        ] {
            assert!(vertex(fields).is_err());
            assert!(vertex_wire(fields).is_err());
        }
    }
}

#[test]
fn the_blend_admission_refuses_a_non_finite_radius_law() {
    use super::{BlendSurfacePayload, BlendSurfacePayloadWire};
    use crate::geometry::{BlendCrossSection, BlendRadiusLaw, CacheContract};

    let blend_new = |radius: BlendRadiusLaw| {
        BlendSurfacePayload::try_new(
            [None, None],
            None,
            radius,
            BlendCrossSection::Circular,
            CacheContract::legacy(),
        )
    };
    let blend_wire = |radius: BlendRadiusLaw| {
        BlendSurfacePayload::try_from(BlendSurfacePayloadWire {
            supports: [None, None],
            spine: None,
            radius,
            cross_section: BlendCrossSection::Circular,
            cache: CacheContract::legacy(),
        })
    };

    // The sign of a radius selects the support offset side, so a negative
    // radius is admitted on both routes.
    let admitted = ProceduralSurfaceDefinition::Blend(
        blend_new(BlendRadiusLaw::linear(-1.5, 2.5).unwrap()).unwrap(),
    );
    let wire = serde_json::to_value(&admitted).unwrap();
    assert_eq!(wire["radius"]["kind"], serde_json::json!("linear"));
    assert_eq!(wire["radius"]["start"], serde_json::json!(-1.5));
    assert_eq!(wire["radius"]["end"], serde_json::json!(2.5));
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        admitted
    );
    assert!(blend_new(BlendRadiusLaw::constant(-3.0).unwrap()).is_ok());
    assert!(blend_wire(BlendRadiusLaw::constant(-3.0).unwrap()).is_ok());

    // The law types state finiteness, so a non-finite radius is refused where
    // the law is admitted, with the refusal the payload stated for it.
    let refusal = Err(crate::geometry::ProceduralGeometryError::Payload(
        "blend radius law is not finite",
    ));
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for law in [
            BlendRadiusLaw::constant(value),
            BlendRadiusLaw::linear(value, 1.0),
            BlendRadiusLaw::linear(1.0, value),
        ] {
            assert_eq!(law, refusal.clone());
        }
    }
}

#[test]
fn an_axis_revolution_replaces_its_origin_and_keeps_the_admitted_direction() {
    use super::AxisRevolutionSurfaceConstruction;
    use crate::features::FinitePoint3;
    use crate::ids::CurveId;
    use crate::math::{Point3, Vector3};

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let direction = Vector3::new(0.0, 0.6, 0.8);
    let mut payload = AxisRevolutionSurfaceConstruction::try_new(
        directrix.clone(),
        Point3::new(1.0, 2.0, 3.0),
        direction,
    )
    .unwrap();
    let moved = Point3::new(-0.0, f64::MAX, 5.0e-324);
    payload.set_axis_origin(FinitePoint3::new(moved).unwrap());
    assert_eq!(
        payload,
        AxisRevolutionSurfaceConstruction::try_new(directrix, moved, direction).unwrap()
    );
    assert_eq!(
        [
            payload.axis_origin().x,
            payload.axis_origin().y,
            payload.axis_origin().z
        ]
        .map(f64::to_bits),
        [moved.x, moved.y, moved.z].map(f64::to_bits)
    );
    assert_eq!(*payload.axis_direction().as_raw(), direction);
}

#[test]
fn a_revolution_admits_a_finite_axis_origin_and_a_unit_axis_direction() {
    use super::{
        admit_revolution_axis, RevolutionSurfaceConstruction, RevolutionSurfaceConstructionWire,
    };
    use crate::features::FinitePoint3;
    use crate::geometry::{CacheContract, ProceduralSurfaceDefinition};
    use crate::ids::CurveId;
    use crate::math::{Point3, Vector3};
    use crate::units::UnitVector3;

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let wire = |axis_origin: Point3, axis_direction: Vector3| RevolutionSurfaceConstructionWire {
        directrix: directrix.clone(),
        axis_origin,
        axis_direction,
        angular_interval: [0.0, 1.0],
        angular_parameter_interval: None,
        parameter_interval: None,
        transposed: false,
        cache: CacheContract::from_form(None),
    };
    let admitted = RevolutionSurfaceConstruction::try_new(
        directrix.clone(),
        (FinitePoint3::ZERO, UnitVector3::Z_AXIS),
        [0.0, 1.0],
        None,
        None,
        false,
        CacheContract::from_form(None),
    )
    .unwrap();
    let admitted_wire =
        serde_json::to_value(ProceduralSurfaceDefinition::Revolution(admitted.clone())).unwrap();
    let origin = Point3::new(0.0, 0.0, 0.0);

    for direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 2.0),
        Vector3::new(0.0, 0.0, 1.0 + 1.0e-6),
        Vector3::new(0.0, f64::NAN, 1.0),
        Vector3::new(0.0, 0.0, f64::INFINITY),
    ] {
        // The raw route into `try_new` and both deserialization routes refuse
        // a direction whose norm is not within the unit tolerance.
        assert!(admit_revolution_axis(origin, direction).is_err());
        assert!(RevolutionSurfaceConstruction::try_from(wire(origin, direction)).is_err());
        let mut definition = admitted_wire.clone();
        definition["axis_direction"] = serde_json::json!([direction.x, direction.y, direction.z]);
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(definition).is_err());
    }
    for axis_origin in [
        Point3::new(f64::NAN, 0.0, 0.0),
        Point3::new(0.0, f64::NEG_INFINITY, 0.0),
    ] {
        assert!(admit_revolution_axis(axis_origin, Vector3::new(0.0, 0.0, 1.0)).is_err());
        assert!(RevolutionSurfaceConstruction::try_from(wire(
            axis_origin,
            Vector3::new(0.0, 0.0, 1.0)
        ))
        .is_err());
    }

    // A direction within the tolerance is stored as given, not renormalized.
    let near_unit = Vector3::new(0.0, 0.0, 1.0 + 1.0e-10);
    let payload = RevolutionSurfaceConstruction::try_from(wire(origin, near_unit)).unwrap();
    assert_eq!(
        payload.axis_direction().as_raw().z.to_bits(),
        near_unit.z.to_bits()
    );
    assert_eq!(
        payload,
        RevolutionSurfaceConstruction::try_new(
            directrix,
            admit_revolution_axis(origin, near_unit).unwrap(),
            [0.0, 1.0],
            None,
            None,
            false,
            CacheContract::from_form(None),
        )
        .unwrap()
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(admitted_wire).unwrap(),
        ProceduralSurfaceDefinition::Revolution(admitted)
    );
}

#[test]
fn a_revolution_replaces_its_origin_and_keeps_the_admitted_fields() {
    use super::RevolutionSurfaceConstruction;
    use crate::features::FinitePoint3;
    use crate::geometry::CacheContract;
    use crate::ids::CurveId;
    use crate::math::Point3;
    use crate::units::UnitVector3;

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let build = |origin: FinitePoint3| {
        RevolutionSurfaceConstruction::try_new(
            directrix.clone(),
            (origin, UnitVector3::Y_AXIS),
            [0.0, 2.0],
            Some([1.0, 3.0]),
            Some([-1.0, 4.0]),
            true,
            CacheContract::from_form(None),
        )
        .unwrap()
    };
    let mut payload = build(FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap());
    let moved = FinitePoint3::new(Point3::new(-0.0, f64::MAX, 5.0e-324)).unwrap();
    payload.set_axis_origin(moved);
    assert_eq!(payload, build(moved));
    assert_eq!(
        [
            payload.axis_origin().x,
            payload.axis_origin().y,
            payload.axis_origin().z
        ]
        .map(f64::to_bits),
        [moved.x, moved.y, moved.z].map(f64::to_bits)
    );
}

#[test]
fn revolution_refusals_name_the_cache_form_first_then_each_interval() {
    use super::RevolutionSurfaceConstruction;
    use crate::features::FinitePoint3;
    use crate::geometry::{
        CacheContract, RevisionCacheForm, RevisionSurfaceForm, RevisionSurfaceParameterization,
    };
    use crate::ids::CurveId;
    use crate::units::UnitVector3;

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let axis = (FinitePoint3::ZERO, UnitVector3::Y_AXIS);
    let raw = |angular: [f64; 2],
               angular_parameter: Option<[f64; 2]>,
               parameter: Option<[f64; 2]>,
               cache: CacheContract<RevisionSurfaceForm>| {
        RevolutionSurfaceConstruction::try_new(
            directrix.clone(),
            axis,
            angular,
            angular_parameter,
            parameter,
            true,
            cache,
        )
    };

    // Each interval refusal names its interval.
    assert_eq!(
        raw([1.0, 1.0], None, None, CacheContract::from_form(None))
            .unwrap_err()
            .to_string(),
        "revolution angular_interval must be finite and strictly increasing"
    );
    assert_eq!(
        raw(
            [0.0, 2.0],
            Some([f64::NAN, 1.0]),
            None,
            CacheContract::from_form(None)
        )
        .unwrap_err()
        .to_string(),
        "revolution angular_parameter_interval must be finite and strictly increasing"
    );
    assert_eq!(
        raw(
            [0.0, 2.0],
            Some([1.0, 3.0]),
            Some([4.0, -1.0]),
            CacheContract::from_form(None)
        )
        .unwrap_err()
        .to_string(),
        "revolution parameter_interval must be finite and strictly increasing"
    );

    // A refused cache form is reported before a refused interval.
    let invalid_cache = || {
        CacheContract::from_form(Some(RevisionSurfaceForm {
            revision: crate::scalar::PositiveI64::new(1).expect("positive revision"),
            support_bounds: [Some(f64::NAN), None, None, None],
            reference_endpoints: [None; 2],
            second_endpoints: [None; 2],
            flags: Vec::new(),
            cache: RevisionCacheForm::Parameterization(RevisionSurfaceParameterization::default()),
            discontinuities: Default::default(),
            tail_flag: false,
            trailing_flags: Vec::new(),
        }))
    };
    let cache_refusal = raw([0.0, 2.0], None, None, invalid_cache()).unwrap_err();
    assert_eq!(
        cache_refusal.to_string(),
        "revolution cache form is invalid"
    );
    assert_eq!(
        raw([1.0, 1.0], None, None, invalid_cache()).unwrap_err(),
        cache_refusal
    );
    assert_eq!(
        raw([0.0, 2.0], Some([f64::NAN, 1.0]), None, invalid_cache()).unwrap_err(),
        cache_refusal
    );
}

#[test]
fn a_legacy_offset_from_admitted_parts_matches_its_raw_admission() {
    use crate::scalar::FiniteReal;

    for (distance, u_sense, v_sense, linear) in
        [(-2.0, None, None, false), (0.0, Some(1), Some(-1), true)]
    {
        let flags = LegacyExtensionFlags::Enabled {
            secondary: true,
            tertiary: None,
        };
        let built = OffsetSurfaceConstruction::legacy(
            support(),
            FiniteReal::new(distance).expect("finite distance"),
            u_sense,
            v_sense,
            linear,
            flags,
            None,
        );
        assert_eq!(
            OffsetSurfaceConstruction::try_new(
                support(),
                distance,
                u_sense,
                v_sense,
                linear,
                OffsetExtension::Legacy { flags, cache: None },
            ),
            Ok(built)
        );
    }
}

#[test]
fn a_legacy_revolution_from_admitted_parts_matches_its_raw_admission() {
    use super::RevolutionSurfaceConstruction;
    use crate::features::FinitePoint3;
    use crate::geometry::CacheContract;
    use crate::ids::CurveId;
    use crate::topology::IncreasingParameterInterval;
    use crate::units::UnitVector3;

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let axis = (FinitePoint3::ZERO, UnitVector3::Y_AXIS);
    let interval = |range: [f64; 2]| IncreasingParameterInterval::new(range).expect("increasing");
    for (angular_parameter, parameter, transposed) in [
        (
            Some(interval([1.0, 3.0])),
            Some(interval([-1.0, 4.0])),
            true,
        ),
        (None, None, false),
    ] {
        assert_eq!(
            RevolutionSurfaceConstruction::try_new(
                directrix.clone(),
                axis,
                [0.5, 0.5 + std::f64::consts::TAU],
                angular_parameter.map(IncreasingParameterInterval::endpoints),
                parameter.map(IncreasingParameterInterval::endpoints),
                transposed,
                CacheContract::from_form(None),
            ),
            Ok(RevolutionSurfaceConstruction::legacy(
                directrix.clone(),
                axis,
                interval([0.5, 0.5 + std::f64::consts::TAU]),
                angular_parameter,
                parameter,
                transposed,
                None,
            ))
        );
    }
}

#[test]
fn a_legacy_extrusion_from_admitted_parts_matches_its_raw_admission() {
    use super::ExtrusionSurfaceConstruction;
    use crate::features::{FinitePoint3, FiniteVector3};
    use crate::geometry::{CacheContract, FitTolerance, LegacyCache};
    use crate::ids::CurveId;
    use crate::math::{Point3, Vector3};
    use crate::topology::IncreasingParameterInterval;
    use crate::units::{FiniteVector, UnitVector3};

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let interval = IncreasingParameterInterval::new([-1.5, 4.0]).unwrap();
    let direction = UnitVector3::new(Vector3::new(0.6, 0.0, 0.8)).unwrap();
    assert_eq!(
        ExtrusionSurfaceConstruction::try_new(
            directrix.clone(),
            Some(interval.endpoints()),
            *direction.as_raw(),
            None,
            CacheContract::from_form(None),
        ),
        Ok(ExtrusionSurfaceConstruction::legacy(
            directrix.clone(),
            Some(FiniteVector::from(interval)),
            FiniteVector3::from(direction),
            None,
            None,
        ))
    );
    let sweep = Vector3::new(0.0, 0.0, 2.0);
    let position = Point3::new(1.0, 2.0, 3.0);
    let cache = LegacyCache::new(FitTolerance::try_new(1.0e-6).unwrap());
    assert_eq!(
        ExtrusionSurfaceConstruction::try_new(
            directrix.clone(),
            None,
            sweep,
            Some(position),
            CacheContract::Legacy { cache: Some(cache) },
        ),
        Ok(ExtrusionSurfaceConstruction::legacy(
            directrix,
            None,
            FiniteVector3::new(sweep).unwrap(),
            FinitePoint3::new(position),
            Some(cache),
        ))
    );
}
