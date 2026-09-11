// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::features::TrimCellSelection;
use crate::math::{Point3, Vector3};

#[test]
fn native_feature_kind_preserves_the_source_spelling() {
    use crate::features::NativeFeatureKind;

    for (wire, expected) in [
        ("\"Canvas\"", NativeFeatureKind::Canvas),
        (
            "\"PartDesign::FeatureBase\"",
            NativeFeatureKind::Other("PartDesign::FeatureBase".into()),
        ),
    ] {
        let kind: NativeFeatureKind = serde_json::from_str(wire).unwrap();
        assert_eq!(kind, expected);
        assert_eq!(serde_json::to_string(&kind).unwrap(), wire);
    }
}

#[test]
fn a_face_maker_is_its_class_and_carries_no_mode_key() {
    use crate::features::FaceMaker;

    #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
    struct ExtrusionCarrier {
        #[serde(default)]
        maker: Option<FaceMaker>,
    }

    assert_eq!(
        serde_json::from_value::<FaceMaker>(serde_json::json!("Part::FaceMakerUnified")).unwrap(),
        FaceMaker::Unified,
    );
    assert_eq!(
        serde_json::to_value(FaceMaker::Bullseye).unwrap(),
        serde_json::json!("Part::FaceMakerBullseye"),
    );
    assert!(serde_json::from_value::<FaceMaker>(serde_json::json!("")).is_err());

    let wire = serde_json::json!({ "maker": "Part::FaceMakerBullseye" });
    let carrier = serde_json::from_value::<ExtrusionCarrier>(wire.clone()).unwrap();
    assert_eq!(
        carrier,
        ExtrusionCarrier {
            maker: Some(FaceMaker::Bullseye),
        }
    );
    assert_eq!(serde_json::to_value(carrier).unwrap(), wire);

    let error = serde_json::from_value::<ExtrusionCarrier>(serde_json::json!({
        "maker": { "class": "Part::FaceMakerBullseye", "mode": 3 }
    }))
    .unwrap_err()
    .to_string();
    assert!(error.contains("invalid type: map"), "{error}");
}

#[test]
fn draft_anchor_round_trips_as_a_nested_key() {
    use crate::features::{DraftAnchor, FeatureDefinition};

    let wire = serde_json::json!({
        "definition": "draft",
        "faces": {"kind": "native", "value": "draft:faces"},
        "anchor": {
            "kind": "parting_line",
            "tool": {"kind": "native", "value": "draft:parting-tool"},
            "pull": {
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                "plane": "test:draft:plane#pull"
            }
        },
        "angle": 0.1,
        "outward": false
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Draft {
            anchor: DraftAnchor::PartingLine { .. },
            ..
        }
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
}

#[test]
fn wrap_mode_round_trips_as_a_nested_key() {
    use crate::features::{FeatureDefinition, WrapMode};

    let wire = serde_json::json!({
        "definition": "wrap",
        "profile": {"kind": "native", "value": "wrap:profile"},
        "face": {"kind": "native", "value": "wrap:face"},
        "mode": {"emboss": {"depth": 2.5}}
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        &definition,
        FeatureDefinition::Wrap {
            mode: WrapMode::Emboss { depth: actual_depth },
            ..
        } if actual_depth.get() == 2.5
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);

    let scribe = FeatureDefinition::Wrap {
        profile: crate::features::PlanarProfileRef::Native("wrap:profile".into()),
        face: crate::features::FaceSelection::Native("wrap:face".into()),
        mode: WrapMode::Scribe,
    };
    let encoded = serde_json::to_value(scribe).unwrap();
    assert_eq!(encoded.get("mode"), Some(&serde_json::json!("scribe")));
}

#[test]
fn helix_shape_round_trips_as_a_nested_key() {
    use crate::features::{FeatureDefinition, HelixShape};

    let conical_wire = serde_json::json!({
        "definition": "helix",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "radius": 2.0,
        "shape": {"kind": "conical", "pitch": 3.0, "cone_angle": 0.2},
        "revolutions": 4.0,
        "start_angle": 0.0,
        "clockwise": false
    });
    let conical: FeatureDefinition = serde_json::from_value(conical_wire.clone()).unwrap();
    assert!(matches!(
        &conical,
        FeatureDefinition::Helix {
            shape: HelixShape::Conical { pitch, .. },
            ..
        } if pitch.get() == 3.0
    ));
    assert_eq!(serde_json::to_value(conical).unwrap(), conical_wire);

    let spiral_wire = serde_json::json!({
        "definition": "helix",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "radius": 2.0,
        "shape": {"kind": "spiral", "radial_growth": 1.5},
        "revolutions": 4.0,
        "start_angle": 0.0,
        "clockwise": false
    });
    let spiral: FeatureDefinition = serde_json::from_value(spiral_wire.clone()).unwrap();
    assert!(matches!(
        &spiral,
        FeatureDefinition::Helix {
            shape: HelixShape::Spiral {
                radial_growth: actual_radial_growth
            },
            ..
        } if actual_radial_growth.get() == 1.5
    ));
    assert_eq!(serde_json::to_value(spiral).unwrap(), spiral_wire);
}

#[test]
fn trim_cell_selection_requires_unique_in_range_ordinals() {
    let valid = TrimCellSelection::new(vec![1, 4], 5).unwrap();
    assert_eq!(valid.removed(), &[1, 4]);
    assert_eq!(valid.total(), 5);
    assert!(TrimCellSelection::new(vec![1, 1], 5).is_none());
    assert!(TrimCellSelection::new(vec![6], 5).is_none());
}

#[test]
fn trim_cells_preserve_the_nested_wire_fields_and_reject_invalid_input() {
    use crate::features::{FaceSelection, FeatureDefinition, PathRef, TrimRegion};

    let definition = FeatureDefinition::TrimSurface {
        faces: FaceSelection::Unresolved,
        tool: PathRef::Unresolved("test:trim-tool".into()),
        keep: TrimRegion::Cells(TrimCellSelection::new(vec![1, 4], 5).unwrap()),
    };
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["definition"], "trim_surface");
    assert_eq!(wire["keep"]["cells"]["removed"], serde_json::json!([1, 4]));
    assert_eq!(wire["keep"]["cells"]["total"], 5);
    let decoded: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        decoded,
        FeatureDefinition::TrimSurface {
            keep: TrimRegion::Cells(ref selection),
            ..
        } if selection.removed() == [1, 4] && selection.total() == 5
    ));

    let mut invalid = wire;
    invalid["keep"]["cells"]["removed"] = serde_json::json!([6]);
    assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
}

#[test]
fn revolve_construction_carries_its_axis_reference_inside_the_axis() {
    use crate::features::{FeatureDefinition, PathRef, RevolveConstruction};

    let wire = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "resolved",
            "profile": {"kind": "sketch", "value": "test:model:sketch#profile"},
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                "reference": {"kind": "native", "value": "test:axis"}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": 1.25}
            },
            "solid": true,
            "face_maker_class": "Part::FaceMakerBullseye"
        },
        "op": "new_body"
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Revolve {
            construction: RevolveConstruction::Resolved { ref axis, .. },
            ..
        } if axis.reference == Some(PathRef::Native("test:axis".into()))
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);

    let mut sibling = wire;
    sibling["construction"].as_object_mut().unwrap().insert(
        "axis_reference".to_string(),
        serde_json::json!({"kind": "native", "value": "test:axis"}),
    );
    let error = serde_json::from_value::<FeatureDefinition>(sibling)
        .unwrap_err()
        .to_string();
    assert!(error.contains("axis_reference"), "{error}");
}

#[test]
fn revolve_construction_admits_only_typed_partial_states() {
    use crate::features::{FeatureDefinition, RevolveConstruction};

    let partial_wire = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "unresolved",
            "missing": "axis",
            "profile": {"kind": "native", "value": "test:profile"},
            "solid": false
        },
        "op": "unresolved"
    });
    let partial: FeatureDefinition = serde_json::from_value(partial_wire.clone()).unwrap();
    assert!(matches!(
        partial,
        FeatureDefinition::Revolve {
            construction: RevolveConstruction::Unresolved(_),
            ..
        }
    ));
    assert_eq!(serde_json::to_value(partial).unwrap(), partial_wire);

    let resolved_without_a_profile = serde_json::json!({
        "definition": "revolve",
        "construction": {
            "state": "resolved",
            "axis": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
            },
            "extent": {
                "kind": "one_sided",
                "termination": {"kind": "angle", "angle": 1.25}
            }
        },
        "op": "unresolved"
    });
    let error = serde_json::from_value::<FeatureDefinition>(resolved_without_a_profile)
        .unwrap_err()
        .to_string();
    assert!(error.contains("profile"), "{error}");
}

#[test]
fn an_unresolved_revolve_names_its_first_missing_operand() {
    use crate::features::{PartialRevolveConstruction, RevolveConstruction};

    let axis = serde_json::json!({
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0}
    });
    let extent = serde_json::json!({
        "kind": "one_sided",
        "termination": {"kind": "angle", "angle": 1.25}
    });
    let profile = serde_json::json!({"kind": "native", "value": "test:profile"});

    let contradicting = serde_json::json!({
        "state": "unresolved",
        "missing": "axis",
        "profile": profile,
        "axis": axis,
    });
    let error = serde_json::from_value::<RevolveConstruction>(contradicting)
        .unwrap_err()
        .to_string();
    assert!(error.contains("axis"), "{error}");

    let untyped = serde_json::json!({
        "state": "unresolved",
        "profile": profile,
        "axis": axis,
        "extent": extent,
    });
    let error = serde_json::from_value::<RevolveConstruction>(untyped)
        .unwrap_err()
        .to_string();
    assert!(error.contains("missing"), "{error}");

    let named = serde_json::json!({
        "state": "unresolved",
        "missing": "profile",
        "axis": axis,
    });
    let construction: RevolveConstruction =
        serde_json::from_value(named.clone()).expect("reads the named partial state");
    assert!(matches!(
        construction,
        RevolveConstruction::Unresolved(PartialRevolveConstruction::Profile {
            axis: Some(_),
            extent: None,
            ..
        })
    ));
    assert_eq!(serde_json::to_value(&construction).unwrap(), named);
}

#[test]
fn extrude_direction_round_trips_as_a_nested_key() {
    use crate::features::{ExtrudeDirection, ExtrusionDirectionSource, FeatureDefinition, PathRef};

    let wire = serde_json::json!({
        "definition": "extrude",
        "profile": {"kind": "native", "value": "test:profile"},
        "direction": {
            "kind": "explicit",
            "vector": {"x": 0.0, "y": 1.0, "z": 0.0},
            "source": {
                "kind": "edge",
                "reference": {"kind": "native", "value": "test:direction-edge"}
            }
        },
        "start": {"kind": "profile_plane"},
        "extent": {
            "kind": "one_sided",
            "side": {
                "termination": {"kind": "blind", "length": 4.0}
            }
        },
        "op": "new_body",
        "solid": true
    });
    let definition: FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Extrude {
            direction: ExtrudeDirection::Explicit {
                source: Some(ExtrusionDirectionSource::Edge {
                    reference: PathRef::Native(ref reference),
                }),
                ..
            },
            ..
        } if reference == "test:direction-edge"
    ));
    assert_eq!(serde_json::to_value(definition).unwrap(), wire);
}

#[test]
fn per_edge_flange_widths_reject_empty_rosters_and_preserve_array_wire() {
    let wire = serde_json::json!({
        "kind": "two_sides_per_edge",
        "value": {"widths": [{"first": 3.0, "second": 1.5}, {"first": 2.0, "second": 4.0}]}
    });
    let width: super::SheetMetalFlangeWidth = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(width).unwrap(), wire);
    let mut empty = wire;
    empty["value"]["widths"] = serde_json::json!([]);
    let error = serde_json::from_value::<super::SheetMetalFlangeWidth>(empty).unwrap_err();
    assert!(error
        .to_string()
        .contains("widths must contain at least one pair"));
    assert!(super::SheetMetalFlangeEdgeWidths::new(Vec::new()).is_err());
}

#[test]
fn sketch_binding_preserves_known_planar_space_without_geometry() {
    for wire in [
        serde_json::json!({"definition": "sketch", "sketch": {"space": "unresolved"}}),
        serde_json::json!({"definition": "sketch", "sketch": {"space": "planar"}}),
        serde_json::json!({
            "definition": "sketch",
            "sketch": {"space": "planar", "sketch": "test:model:sketch#1"}
        }),
    ] {
        let definition: super::FeatureDefinition = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(definition).unwrap(), wire);
    }
    for wire in [
        serde_json::json!({
            "definition": "sketch",
            "sketch": {"space": "unresolved", "sketch": "test:model:sketch#1"}
        }),
        serde_json::json!({"definition": "sketch", "sketch": {"space": "spatial"}}),
    ] {
        assert!(serde_json::from_value::<super::FeatureDefinition>(wire).is_err());
    }
    assert_ne!(
        super::SketchFeatureBinding::Unresolved,
        super::SketchFeatureBinding::Planar(None)
    );
}

mod body_selection;

#[test]
fn body_selection_admission_rejects_invalid_members() {
    use super::{BodySelection, GeneratedBodyRef, NativeSelections};
    use crate::ids::FeatureInputTopologyId;
    for names in [
        vec![],
        vec![" ".to_owned()],
        vec!["a".to_owned(), "a".to_owned()],
    ] {
        assert!(BodySelection::local(names.clone(), "native".into()).is_err());
        assert!(NativeSelections::try_from(names).is_err());
    }
    assert!(BodySelection::local(vec!["body".into()], " ".into()).is_err());
    let state = FeatureInputTopologyId::mint("test:model:feature-input#1").unwrap();
    assert!(BodySelection::historical(state, vec![], "native".into()).is_err());
    assert!(GeneratedBodyRef::new(
        super::FeatureId::mint("test:test:feature#1").unwrap(),
        " ".into()
    )
    .is_err());
    for value in [
        serde_json::json!({"kind":"local","value":{"bodies":[],"native":"source"}}),
        serde_json::json!({"kind":"local","value":{"bodies":["a","a"],"native":"source"}}),
        serde_json::json!({"kind":"generated","value":{"bodies":[],"native":"source"}}),
        serde_json::json!({"kind":"native_set","value":[" "]}),
    ] {
        assert!(serde_json::from_value::<BodySelection>(value).is_err());
    }
}

#[test]
fn topology_membership_admission() {
    use super::{DistinctMembers, FeatureResultTopology};
    let id = crate::ids::FeatureResultTopologyId::mint("test:model:feature-result#1").unwrap();
    let feature = super::FeatureId::mint("test:test:feature#1").unwrap();
    assert!(FeatureResultTopology::new(
        id.clone(),
        feature.clone(),
        vec![],
        vec![],
        vec![],
        vec![],
        None
    )
    .is_err());
    for bodies in [vec![" ".into()], vec!["a".into(), "a".into()]] {
        assert!(FeatureResultTopology::new(
            id.clone(),
            feature.clone(),
            bodies,
            vec![],
            vec![],
            vec![],
            None
        )
        .is_err());
    }
    assert!(DistinctMembers::<String>::try_from(vec!["a".into(), "a".into()]).is_err());
    assert!(
        serde_json::from_value::<super::ConfigurationBodies>(serde_json::json!([
            "test:model:body#1",
            "test:model:body#1"
        ]))
        .is_err()
    );
    assert!(serde_json::from_value::<FeatureResultTopology>(
        serde_json::json!({"id":"test:model:feature-result#1","output_of":"test:test:feature#1"})
    )
    .is_err());
}

#[test]
fn primitive_dimensions_are_checked_at_construction_and_deserialization() {
    use super::{PrimitiveSolid, PrimitiveSolidKind};
    use serde_json::{json, Value};

    let cases: [(Value, &[&str]); 8] = [
        (
            json!({"kind":"box","length":1.0,"width":2.0,"height":3.0}),
            &["length", "width", "height"],
        ),
        (
            json!({"kind":"cylinder","radius":1.0,"height":2.0,"angle":0.0}),
            &["radius", "height"],
        ),
        (
            json!({"kind":"cone","radius1":0.0,"radius2":1.0,"height":2.0,"angle":-1.0}),
            &["height"],
        ),
        (
            json!({"kind":"sphere","radius":1.0,"latitude1":-1.0,"latitude2":1.0,"longitude":0.0}),
            &["radius"],
        ),
        (
            json!({"kind":"ellipsoid","x_radius":1.0,"y_radius":2.0,"z_radius":3.0,"latitude1":-1.0,"latitude2":1.0,"longitude":-1.0}),
            &["x_radius", "y_radius", "z_radius"],
        ),
        (
            json!({"kind":"torus","major_radius":1.0,"minor_radius":2.0,"latitude1":-1.0,"latitude2":1.0,"longitude":0.0}),
            &["major_radius", "minor_radius"],
        ),
        (
            json!({"kind":"prism","sides":3,"circumradius":1.0,"height":2.0}),
            &["circumradius", "height"],
        ),
        (
            json!({"kind":"wedge","xmin":-1.0,"ymin":-1.0,"zmin":-1.0,"x2min":0.0,"z2min":0.0,"xmax":1.0,"ymax":1.0,"zmax":1.0,"x2max":0.0,"z2max":0.0}),
            &[],
        ),
    ];
    for (wire, positive_fields) in cases {
        let kind: PrimitiveSolidKind = serde_json::from_value(wire.clone()).unwrap();
        let solid = PrimitiveSolid::new(kind).unwrap();
        assert_eq!(serde_json::to_value(&solid).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<PrimitiveSolid>(wire.clone()).unwrap(),
            solid
        );
        let mut invalid = Vec::new();
        for field in positive_fields {
            for value in [0.0, -1.0] {
                let mut changed = wire.clone();
                changed[field] = json!(value);
                invalid.push(changed);
            }
        }
        match wire["kind"].as_str().unwrap() {
            "cone" => {
                let mut both_zero = wire.clone();
                both_zero["radius2"] = json!(0.0);
                invalid.push(both_zero);
                for field in ["radius1", "radius2"] {
                    let mut changed = wire.clone();
                    changed[field] = json!(-1.0);
                    invalid.push(changed);
                }
            }
            "sphere" | "ellipsoid" | "torus" => {
                for lower in [1.0, 2.0] {
                    let mut changed = wire.clone();
                    changed["latitude1"] = json!(lower);
                    invalid.push(changed);
                }
            }
            "prism" => {
                for sides in [0, 1, 2] {
                    let mut changed = wire.clone();
                    changed["sides"] = json!(sides);
                    invalid.push(changed);
                }
            }
            "wedge" => {
                for field in ["xmax", "ymax", "zmax", "x2max", "z2max"] {
                    let mut changed = wire.clone();
                    changed[field] = json!(-1.0);
                    invalid.push(changed);
                }
            }
            _ => {}
        }
        for changed in invalid {
            let kind: PrimitiveSolidKind = serde_json::from_value(changed.clone()).unwrap();
            assert!(PrimitiveSolid::new(kind).is_err(), "{changed}");
            assert!(
                serde_json::from_value::<PrimitiveSolid>(changed.clone()).is_err(),
                "{changed}"
            );
        }
    }
}

#[test]
fn scale_factors_admit_only_finite_nonzero_components() {
    use crate::{features::ScaleFactors, scalar::NonZeroReal};
    for value in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(NonZeroReal::new(value).is_none());
    }
    for value in [-2.0, 0.5] {
        let factors = ScaleFactors::Uniform(NonZeroReal::new(value).unwrap());
        let wire = serde_json::to_value(factors).unwrap();
        assert_eq!(wire, serde_json::json!({"uniform": value}));
        assert_eq!(
            serde_json::from_value::<ScaleFactors>(wire).unwrap(),
            factors
        );
    }
    let factors =
        ScaleFactors::PerAxis([-1.0, 2.0, -3.0].map(|value| NonZeroReal::new(value).unwrap()));
    let wire = serde_json::to_value(factors).unwrap();
    assert_eq!(wire, serde_json::json!({"x": -1.0, "y": 2.0, "z": -3.0}));
    assert_eq!(
        serde_json::from_value::<ScaleFactors>(wire).unwrap(),
        factors
    );
    for wire in [
        serde_json::json!({"uniform": 0.0}),
        serde_json::json!({"x": 0.0, "y": 1.0, "z": 1.0}),
        serde_json::json!({"x": 1.0, "y": 0.0, "z": 1.0}),
        serde_json::json!({"x": 1.0, "y": 1.0, "z": 0.0}),
        serde_json::json!({"x": 1.0, "y": 1.0}),
        serde_json::json!({"uniform": 1.0, "x": 1.0}),
    ] {
        assert!(serde_json::from_value::<ScaleFactors>(wire).is_err());
    }
}

#[test]
fn blind_and_coil_lengths_reject_zero_without_losing_signed_wire_values() {
    use crate::features::{CoilExtent, LinearTermination};
    for value in [-4.0, 4.0] {
        let wire = serde_json::json!({"kind": "blind", "length": value});
        let admitted: LinearTermination = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    }
    for value in [-0.0, 0.0] {
        assert!(serde_json::from_value::<LinearTermination>(
            serde_json::json!({"kind":"blind","length":value})
        )
        .is_err());
        for wire in [
            serde_json::json!({"kind":"revolutions_pitch","revolutions":1.0,"pitch":value}),
            serde_json::json!({"kind":"height_pitch","height":value,"pitch":1.0}),
            serde_json::json!({"kind":"height_pitch","height":1.0,"pitch":value}),
            serde_json::json!({"kind":"spiral","revolutions":1.0,"radial_pitch":value}),
        ] {
            assert!(serde_json::from_value::<CoilExtent>(wire).is_err());
        }
    }
    let wire = serde_json::json!({"kind":"revolutions_height","revolutions":1.0,"height":0.0});
    let admitted: CoilExtent = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
}

#[test]
fn hole_profile_filters_admit_exactly_the_nonempty_family_sets() {
    use crate::features::HoleProfileFilter;
    let filters = [
        HoleProfileFilter::Points,
        HoleProfileFilter::Circles,
        HoleProfileFilter::PointsAndCircles,
        HoleProfileFilter::Arcs,
        HoleProfileFilter::PointsAndArcs,
        HoleProfileFilter::CirclesAndArcs,
        HoleProfileFilter::All,
    ];
    for (bits, filter) in (1..=7).zip(filters) {
        let wire = serde_json::json!({"points": bits & 1 != 0, "circles": bits & 2 != 0, "arcs": bits & 4 != 0});
        assert_eq!(serde_json::to_value(filter).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<HoleProfileFilter>(wire).unwrap(),
            filter
        );
    }
    assert!(serde_json::from_value::<HoleProfileFilter>(
        serde_json::json!({"points":false,"circles":false,"arcs":false})
    )
    .is_err());
}

#[test]
fn polygon_side_counts_reject_degenerate_polygons_at_admission() {
    use crate::features::PolygonSideCount;
    for value in [0, 1, 2] {
        assert!(PolygonSideCount::new(value).is_none());
        assert!(serde_json::from_value::<PolygonSideCount>(serde_json::json!(value)).is_err());
    }
    for value in [3, 7, u32::MAX] {
        let sides = PolygonSideCount::new(value).unwrap();
        assert_eq!(sides.get(), value);
        assert_eq!(
            serde_json::to_value(sides).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::from_value::<PolygonSideCount>(serde_json::json!(value)).unwrap(),
            sides
        );
    }
}

#[test]
fn feature_geometry_admission_preserves_nonunit_directions_and_zero_displacements() {
    use crate::features::{FeatureDirection3, FinitePoint3, FiniteVector3};
    for point in [Point3::new(0.0, 0.0, 0.0), Point3::new(-1.0, 2.0, f64::MAX)] {
        let admitted = FinitePoint3::new(point).unwrap();
        let wire = serde_json::to_value(point).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FinitePoint3>(wire).unwrap(),
            admitted
        );
    }
    for vector in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(-1.0, 2.0, f64::MAX),
    ] {
        let admitted = FiniteVector3::new(vector).unwrap();
        let wire = serde_json::to_value(vector).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FiniteVector3>(wire).unwrap(),
            admitted
        );
    }
    for vector in [Vector3::new(0.0, 0.0, -2.0), Vector3::new(3.0, 4.0, 0.0)] {
        let admitted = FeatureDirection3::new(vector).unwrap();
        let wire = serde_json::to_value(vector).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDirection3>(wire).unwrap(),
            admitted
        );
    }
    for vector in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::MAX, 0.0, 0.0),
    ] {
        assert!(FeatureDirection3::new(vector).is_none());
        assert!(
            serde_json::from_value::<FeatureDirection3>(serde_json::to_value(vector).unwrap())
                .is_err()
        );
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut components = [0.0; 3];
            components[axis] = invalid;
            assert!(FinitePoint3::new(Point3::from(components)).is_none());
            assert!(FiniteVector3::new(Vector3::from(components)).is_none());
            assert!(FeatureDirection3::new(Vector3::from(components)).is_none());
            let deserialize_point = || {
                serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
                    [
                        ("x", components[0]),
                        ("y", components[1]),
                        ("z", components[2]),
                    ]
                    .into_iter(),
                )
            };
            assert!(
                <FinitePoint3 as serde::Deserialize>::deserialize(deserialize_point()).is_err()
            );
            assert!(
                <FiniteVector3 as serde::Deserialize>::deserialize(deserialize_point()).is_err()
            );
            assert!(
                <FeatureDirection3 as serde::Deserialize>::deserialize(deserialize_point())
                    .is_err()
            );
        }
    }
}

#[test]
fn feature_lines_and_polylines_close_geometry_bounds_without_changing_wire_fields() {
    use crate::features::{FeatureDefinition, FeatureLineSegment, FeaturePolyline};
    let first = Point3::new(0.0, 1.0, 2.0);
    let second = Point3::new(3.0, 4.0, 5.0);
    let segment = FeatureLineSegment::new(first, second).unwrap();
    let wire =
        serde_json::json!({"definition":"line_segment", "segment":{"start":first, "end":second}});
    let definition = FeatureDefinition::LineSegment { segment };
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
        definition
    );
    assert!(FeatureLineSegment::new(first, first).is_none());
    assert!(FeatureLineSegment::new(Point3::new(f64::NAN, 0.0, 0.0), second).is_none());
    assert!(serde_json::from_value::<FeatureDefinition>(
        serde_json::json!({"definition":"line_segment", "segment":{"start":first, "end":first}})
    )
    .is_err());

    for (points, closed) in [
        (vec![first, second], false),
        (vec![first, second, first], true),
        (vec![first, second, first], false),
    ] {
        let chain = FeaturePolyline::new(points.clone(), closed).unwrap();
        let wire = serde_json::json!({"definition":"polyline", "chain":{"points":points, "closed":closed}});
        let definition = FeatureDefinition::Polyline { chain };
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire).unwrap(),
            definition
        );
    }
    for (points, closed) in [
        (vec![], false),
        (vec![first], false),
        (vec![first, second], true),
        (vec![first, first, second], false),
    ] {
        assert!(FeaturePolyline::new(points.clone(), closed).is_none());
        assert!(serde_json::from_value::<FeatureDefinition>(
            serde_json::json!({"definition":"polyline", "chain":{"points":points, "closed":closed}})
        )
        .is_err());
    }
    assert!(
        FeaturePolyline::new(vec![first, Point3::new(f64::INFINITY, 0.0, 0.0)], false).is_none()
    );
}

#[test]
fn equation_curve_admission_preserves_expression_text_and_requires_an_increasing_domain() {
    use crate::features::{FeatureDefinition, FeatureEquationCurve};
    let curve = FeatureEquationCurve::new(
        " t ".into(),
        " t*t ".into(),
        "0".into(),
        " -t ".into(),
        -2.0,
        3.0,
    )
    .unwrap();
    let definition = FeatureDefinition::EquationCurve { curve };
    let wire = serde_json::json!({"definition":"equation_curve", "curve":{"parameter":" t ", "x_expression":" t*t ", "y_expression":"0", "z_expression":" -t ", "start":-2.0, "end":3.0}});
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );
    for field in ["parameter", "x_expression", "y_expression", "z_expression"] {
        for blank in ["", " \t\n"] {
            let mut invalid = wire.clone();
            invalid[field] = serde_json::json!(blank);
            assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    for [start, end] in [
        [1.0, 1.0],
        [2.0, 1.0],
        [f64::NAN, 2.0],
        [0.0, f64::INFINITY],
        [f64::NEG_INFINITY, 0.0],
    ] {
        assert!(FeatureEquationCurve::new(
            "t".into(),
            "t".into(),
            "0".into(),
            "0".into(),
            start,
            end
        )
        .is_none());
    }
    for [start, end] in [[1.0, 1.0], [2.0, 1.0]] {
        let mut invalid = wire.clone();
        invalid["curve"]["start"] = serde_json::json!(start);
        invalid["curve"]["end"] = serde_json::json!(end);
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
}

#[test]
fn feature_arcs_preserve_directed_spans_and_admit_only_valid_frames_and_radii() {
    use crate::geometry::DirectedParameterRange;
    use crate::{
        features::{FeatureCircularArc, FeatureDefinition, FeatureEllipticArc},
        scalar::PositiveLength,
    };
    let center = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let major_axis = Vector3::new(3.0, 0.0, 0.0);
    let major = PositiveLength::new(4.0).unwrap();
    let minor = PositiveLength::new(2.0).unwrap();
    for endpoints in [[0.0, std::f64::consts::TAU], [2.0, -1.0]] {
        let angles = DirectedParameterRange::new(endpoints).unwrap();
        let arc = FeatureCircularArc::new(center, normal, major, angles).unwrap();
        let definition = FeatureDefinition::CircularArc { arc };
        let wire = serde_json::json!({"definition":"circular_arc", "arc":{"center":center, "normal":normal, "radius":4.0, "start_angle":endpoints[0], "end_angle":endpoints[1]}});
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
            definition
        );
        let mut invalid = wire;
        invalid["arc"]["end_angle"] = invalid["arc"]["start_angle"].clone();
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());

        let arc =
            FeatureEllipticArc::new(center, normal, major_axis, [major, minor], angles).unwrap();
        let definition = FeatureDefinition::EllipticArc { arc };
        let wire = serde_json::json!({"definition":"elliptic_arc", "arc":{"center":center, "normal":normal, "major_axis":major_axis, "major_radius":4.0, "minor_radius":2.0, "start_angle":endpoints[0], "end_angle":endpoints[1]}});
        assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
            definition
        );
        for (field, value) in [
            ("minor_radius", serde_json::json!(5.0)),
            ("major_axis", serde_json::json!({"x":0.0,"y":0.0,"z":1.0})),
            ("end_angle", serde_json::json!(endpoints[0])),
        ] {
            let mut invalid = wire.clone();
            invalid["arc"][field] = value;
            assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
        }
    }
    let angles = DirectedParameterRange::new([0.0, 1.0]).unwrap();
    assert!(FeatureCircularArc::new(center, Vector3::new(0.0, 0.0, 0.0), major, angles).is_none());
    assert!(FeatureEllipticArc::new(center, normal, major_axis, [minor, major], angles).is_none());
    assert!(FeatureEllipticArc::new(center, normal, major_axis, [major, major], angles).is_some());
    assert!(FeatureEllipticArc::new(center, normal, normal, [major, minor], angles).is_none());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        Vector3::new(1.0, 0.0, super::EPS_FEATURE_ELLIPSE_AXES_ORTHO / 2.0),
        [major, minor],
        angles
    )
    .is_some());
    assert!(FeatureEllipticArc::new(
        center,
        normal,
        Vector3::new(1.0, 0.0, super::EPS_FEATURE_ELLIPSE_AXES_ORTHO),
        [major, minor],
        angles
    )
    .is_none());
}

#[test]
fn block_placement_admission_requires_a_right_handed_rigid_transform() {
    use crate::features::FeatureRigidPlacement;
    use crate::transform::Transform;
    for rows in [
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [0.0, -1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0, -2.0],
            [0.0, 0.0, 1.0, 5.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ] {
        let transform = Transform::from_rows(rows).unwrap();
        let placement = FeatureRigidPlacement::new(transform).unwrap();
        let wire = serde_json::to_value(transform).unwrap();
        assert_eq!(serde_json::to_value(placement).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureRigidPlacement>(wire).unwrap(),
            placement
        );
    }
    for rows in [
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.25, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    ] {
        let transform = Transform::from_rows(rows).unwrap();
        assert!(FeatureRigidPlacement::new(transform).is_none());
        assert!(serde_json::from_value::<FeatureRigidPlacement>(
            serde_json::to_value(transform).unwrap()
        )
        .is_err());
    }
}

#[test]
fn helical_sweep_travel_preserves_signed_and_planar_values_but_rejects_zero_travel() {
    use crate::{features::HelicalSweepTravel, scalar::Length};
    for [height, radial_growth] in [[-2.0, 0.0], [0.0, -3.0], [2.0, -3.0]] {
        let travel = HelicalSweepTravel::new(
            Length::new(height).unwrap(),
            Length::new(radial_growth).unwrap(),
        )
        .unwrap();
        let wire = serde_json::json!({"height":height,"radial_growth":radial_growth});
        assert_eq!(serde_json::to_value(travel).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<HelicalSweepTravel>(wire).unwrap(),
            travel
        );
    }
    assert!(HelicalSweepTravel::new(Length::ZERO, Length::ZERO).is_none());
    assert!(serde_json::from_value::<HelicalSweepTravel>(
        serde_json::json!({"height":0.0,"radial_growth":0.0})
    )
    .is_err());
}

#[test]
fn feature_coordinate_frame_admission_preserves_wire_and_handedness_bound() {
    use crate::features::{FeatureCoordinateFrame, FeatureDefinition, EPS_FEATURE_UNIT_FRAME};
    use crate::math::{Point3, Vector3};
    let origin = Point3::new(1.0, 2.0, 3.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    let y = Vector3::new(0.0, 1.0, 0.0);
    let z = Vector3::new(0.0, 0.0, 1.0);
    let frame = FeatureCoordinateFrame::new(origin, x, y, z).unwrap();
    let wire = serde_json::json!({"definition":"datum_coordinate_system","frame":{"origin":origin,"x_axis":x,"y_axis":y,"z_axis":z}});
    let definition = FeatureDefinition::DatumCoordinateSystem { frame };
    assert_eq!(serde_json::to_value(&definition).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(wire.clone()).unwrap(),
        definition
    );
    for (key, value) in [
        ("x_axis", y),
        ("z_axis", Vector3::new(0.0, 0.0, -1.0)),
        ("x_axis", Vector3::new(2.0, 0.0, 0.0)),
    ] {
        let mut invalid = wire.clone();
        invalid["frame"][key] = serde_json::to_value(value).unwrap();
        assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    }
    assert!(FeatureCoordinateFrame::new(Point3::new(f64::NAN, 0.0, 0.0), x, y, z).is_none());
    let expanded = 1.0 + EPS_FEATURE_UNIT_FRAME * 0.75;
    let frame = FeatureCoordinateFrame::new(
        origin,
        Vector3::new(expanded, 0.0, 0.0),
        Vector3::new(0.0, expanded, 0.0),
        Vector3::new(0.0, 0.0, expanded),
    )
    .unwrap();
    assert!(
        frame.x_axis().cross(frame.y_axis()).dot(frame.z_axis()) > 1.0 + EPS_FEATURE_UNIT_FRAME
    );
}

#[test]
fn feature_unit_plane_and_image_bounds_reject_degenerate_geometry() {
    use crate::features::{FeatureImageBounds, FeatureUnitPlaneFrame, EPS_FEATURE_UNIT_FRAME};
    use crate::math::{Point2, Point3, Vector3};
    let origin = Point3::new(0.0, 0.0, 0.0);
    let x = Vector3::new(1.0, 0.0, 0.0);
    for other in [
        Vector3::new(0.0, 0.0, 0.0),
        x,
        Vector3::new(f64::INFINITY, 0.0, 0.0),
    ] {
        assert!(FeatureUnitPlaneFrame::new(origin, x, other).is_none());
    }
    assert!(FeatureUnitPlaneFrame::new(
        origin,
        x,
        Vector3::new(EPS_FEATURE_UNIT_FRAME * 0.5, 1.0, 0.0)
    )
    .is_some());
    assert!(FeatureUnitPlaneFrame::new(
        origin,
        x,
        Vector3::new(EPS_FEATURE_UNIT_FRAME * 2.0, 1.0, 0.0)
    )
    .is_none());
    assert!(serde_json::from_value::<FeatureUnitPlaneFrame>(
        serde_json::json!({"origin":origin,"u_axis":x,"v_axis":x})
    )
    .is_err());
    for corners in [
        [Point2::new(2.0, 3.0), Point2::new(-2.0, -3.0)],
        [Point2::new(-2.0, 3.0), Point2::new(2.0, -3.0)],
    ] {
        let bounds = FeatureImageBounds::new(corners).unwrap();
        assert_eq!(bounds.corners(), corners);
        let wire = serde_json::to_value(corners).unwrap();
        assert_eq!(serde_json::to_value(bounds).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<FeatureImageBounds>(wire).unwrap(),
            bounds
        );
    }
    for corners in [
        [Point2::new(0.0, 0.0), Point2::new(0.0, 1.0)],
        [Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
    ] {
        assert!(FeatureImageBounds::new(corners).is_none());
        assert!(serde_json::from_value::<FeatureImageBounds>(
            serde_json::to_value(corners).unwrap()
        )
        .is_err());
    }
    assert!(FeatureImageBounds::new([Point2::new(f64::NAN, 0.0), Point2::new(1.0, 1.0)]).is_none());
}

#[test]
fn reference_image_and_coil_frames_preserve_wire_fields_and_reject_invalid_frames() {
    use crate::features::{CoilPlacement, FeatureDefinition};
    let image = serde_json::json!({
        "definition":"reference_image", "asset":"synthetic:test:asset#frame",
        "visible":true, "mirror_u":false, "mirror_v":false,
        "frame":{
            "origin":{"x":1.0,"y":2.0,"z":3.0},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0},
            "v_axis":{"x":0.0,"y":1.0,"z":0.0}
        },
        "bounds":[{"u":2.0,"v":3.0},{"u":-2.0,"v":-3.0}]
    });
    let decoded = serde_json::from_value::<FeatureDefinition>(image.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), image);
    let mut invalid = image.clone();
    invalid["frame"]["v_axis"] = invalid["frame"]["u_axis"].clone();
    assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());
    let mut invalid = image;
    invalid["bounds"][1]["u"] = serde_json::json!(2.0);
    assert!(serde_json::from_value::<FeatureDefinition>(invalid).is_err());

    let coil = serde_json::json!({"kind":"explicit",
        "origin":{"x":1.0,"y":2.0,"z":3.0},
        "axis":{"x":0.0,"y":0.0,"z":1.0},
        "radial":{"x":1.0,"y":0.0,"z":0.0}
    });
    let decoded = serde_json::from_value::<CoilPlacement>(coil.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), coil);
    let mut invalid = coil;
    invalid["radial"] = invalid["axis"].clone();
    assert!(serde_json::from_value::<CoilPlacement>(invalid).is_err());
    assert!(serde_json::from_value::<CoilPlacement>(
        serde_json::json!({"kind":"native","native_ref":" \t "})
    )
    .is_err());
    let native = serde_json::json!({"kind":"native","native_ref":" source:placement "});
    let decoded = serde_json::from_value::<CoilPlacement>(native.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), native);
}

#[test]
fn datum_and_support_plane_frames_preserve_nonunit_geometry_and_wire_fields() {
    use crate::features::{
        DatumPlaneReference, FeatureDatumPlaneFrame, FeatureDefinition, FeatureSupportPlaneFrame,
    };
    let origin = Point3::new(1.0, 2.0, 3.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    let u_axis = Vector3::new(-3.0, 0.0, 0.0);
    let datum = FeatureDatumPlaneFrame::new(origin, normal, u_axis).unwrap();
    let support = FeatureSupportPlaneFrame::new(origin, normal, u_axis).unwrap();
    let geometry = serde_json::json!({"origin":origin,"normal":normal,"u_axis":u_axis});
    assert_eq!(serde_json::to_value(datum).unwrap(), geometry);
    assert_eq!(serde_json::to_value(support).unwrap(), geometry);
    let datum_wire = serde_json::json!({"definition":"datum_plane","frame":geometry.clone()});
    let definition = FeatureDefinition::DatumPlane { frame: datum };
    assert_eq!(serde_json::to_value(&definition).unwrap(), datum_wire);
    assert_eq!(
        serde_json::from_value::<FeatureDefinition>(datum_wire).unwrap(),
        definition
    );
    let resolved = DatumPlaneReference::ResolvedPlane { frame: support };
    let resolved_wire =
        serde_json::json!({"reference": "resolved_plane", "frame": geometry.clone()});
    assert_eq!(serde_json::to_value(&resolved).unwrap(), resolved_wire);
    assert_eq!(
        serde_json::from_value::<DatumPlaneReference>(resolved_wire).unwrap(),
        resolved
    );
}

#[test]
fn plane_frame_admission_rejects_nonfinite_degenerate_and_nonorthogonal_directions() {
    use crate::features::{FeatureDatumPlaneFrame, FeatureSupportPlaneFrame};
    let origin = Point3::new(0.0, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 2.0);
    for u_axis in [Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 1.0)] {
        assert!(FeatureDatumPlaneFrame::new(origin, normal, u_axis).is_none());
        assert!(FeatureSupportPlaneFrame::new(origin, normal, u_axis).is_none());
        let wire = serde_json::json!({"origin":origin,"normal":normal,"u_axis":u_axis});
        assert!(serde_json::from_value::<FeatureDatumPlaneFrame>(wire.clone()).is_err());
        assert!(serde_json::from_value::<FeatureSupportPlaneFrame>(wire).is_err());
    }
    let u_axis = Vector3::new(3.0, 0.0, 0.0);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            FeatureDatumPlaneFrame::new(Point3::new(value, 0.0, 0.0), normal, u_axis).is_none()
        );
        assert!(
            FeatureSupportPlaneFrame::new(origin, Vector3::new(0.0, 0.0, value), u_axis).is_none()
        );
    }
    let small = f64::EPSILON / 2.0;
    assert!(FeatureDatumPlaneFrame::new(origin, Vector3::new(0.0, 0.0, small), u_axis).is_some());
    assert!(FeatureSupportPlaneFrame::new(origin, normal, Vector3::new(small, 0.0, 0.0)).is_some());
}

#[test]
fn plane_frame_owners_preserve_their_distinct_floating_point_thresholds() {
    use crate::features::{
        FeatureDatumPlaneFrame, FeatureSupportPlaneFrame, EPS_FEATURE_PLANE_ORTHOGONAL,
    };
    let origin = Point3::new(0.0, 0.0, 0.0);
    let normal = Vector3::new(0.0, 0.0, 0.1);
    let u_axis = Vector3::new(0.7, 0.0, EPS_FEATURE_PLANE_ORTHOGONAL * 0.7);
    let dot = normal.dot(u_axis).abs();
    let datum_bound = EPS_FEATURE_PLANE_ORTHOGONAL * (normal.norm() * u_axis.norm());
    let support_bound = (EPS_FEATURE_PLANE_ORTHOGONAL * normal.norm()) * u_axis.norm();
    assert!(dot > datum_bound);
    assert!(dot <= support_bound);
    assert!(FeatureDatumPlaneFrame::new(origin, normal, u_axis).is_none());
    assert!(FeatureSupportPlaneFrame::new(origin, normal, u_axis).is_some());
}

#[test]
fn a_resolved_plane_reference_checks_the_geometry_it_carries() {
    use crate::features::{DatumPlaneReference, FaceSelection};
    let degenerate = serde_json::json!({
        "reference": "resolved_plane",
        "frame": {
            "origin":{"x":0.0,"y":0.0,"z":0.0},
            "normal":{"x":0.0,"y":0.0,"z":0.0},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0}
        }
    });
    assert!(serde_json::from_value::<DatumPlaneReference>(degenerate).is_err());
    let native = serde_json::json!({
        "reference": "face",
        "face": {"kind":"native","value":"face:retained"}
    });
    assert_eq!(
        serde_json::from_value::<DatumPlaneReference>(native).unwrap(),
        DatumPlaneReference::Face {
            face: FaceSelection::Native("face:retained".into())
        }
    );
}

mod edge_treatments;
mod patterns;

mod selections;

mod parameters;

mod configuration_states;

mod source_content;

mod profile_selections;

mod profile_regions;

mod flange_widths;

mod local_admission;

mod configurations;
mod wire_forms;

#[test]
fn every_payload_free_feature_variant_the_freecad_sweep_reached_refuses_an_unknown_key() {
    use crate::features::{ExtrusionDirectionSource, FuzzyTolerance, SweepOrientation};

    for kind in ["custom", "profile_normal"] {
        let wire = serde_json::json!({"kind": kind, "zz_bogus": 1});
        let error = serde_json::from_value::<ExtrusionDirectionSource>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{kind}: {error}");
    }
    for kind in ["corrected_frenet", "fixed", "frenet"] {
        let wire = serde_json::json!({"kind": kind, "zz_bogus": 1});
        let error = serde_json::from_value::<SweepOrientation>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{kind}: {error}");
    }
    for kind in ["kernel_default", "automatic"] {
        let wire = serde_json::json!({"kind": kind, "zz_bogus": 1});
        let error = serde_json::from_value::<FuzzyTolerance>(wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{kind}: {error}");
    }

    assert_eq!(
        serde_json::to_value(ExtrusionDirectionSource::Custom {}).unwrap(),
        serde_json::json!({"kind": "custom"})
    );
    assert_eq!(
        serde_json::to_value(SweepOrientation::Frenet {}).unwrap(),
        serde_json::json!({"kind": "frenet"})
    );
    assert_eq!(
        serde_json::to_value(FuzzyTolerance::KernelDefault).unwrap(),
        serde_json::json!({"kind": "kernel_default"})
    );
}
