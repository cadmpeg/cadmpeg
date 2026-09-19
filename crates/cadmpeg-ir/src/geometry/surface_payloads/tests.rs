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
            revision: 1,
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
