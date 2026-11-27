use super::{
    MeshFaceDistribution, MeshModification, MeshRebuildMode, MeshReduceTarget, MeshRepair,
    MeshSelection,
};
use crate::features::FeatureOperation;
use cadmpeg_core::serde_erase::json;

#[test]
fn separation_wire_keeps_independent_controls_and_selected_category() {
    use super::{MeshSeparation, MeshSeparationGroups};
    assert_eq!(
        json::to_value(MeshSeparation::default()).unwrap(),
        cadmpeg_core::json!({})
    );
    for separation in [
        cadmpeg_core::json!({}),
        cadmpeg_core::json!({"method":"shells", "multiple_bodies":true, "keep_body":false}),
        cadmpeg_core::json!({"groups":{"kind":"all"}, "keep_body":true}),
        cadmpeg_core::json!({"method":"face_groups", "groups":{"kind":"selected"}}),
        cadmpeg_core::json!({"groups":{"kind":"selected", "members":["beta", "alpha"]}, "multiple_bodies":false}),
    ] {
        let wire = cadmpeg_core::json!({"kind": "mesh_modify", "value": {"source":{"kind":"unresolved","value":{"feature":"test:model:feature#separate"}}, "modification":{"kind":"separate", "separation":separation}}});
        let decoded: FeatureOperation = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(decoded).unwrap(), wire);
    }
    let selected: MeshSeparation =
        json::from_value(cadmpeg_core::json!({"groups":{"kind":"selected"}})).unwrap();
    assert_eq!(
        selected.groups,
        Some(MeshSeparationGroups::Selected { members: None })
    );
    for invalid in [
        cadmpeg_core::json!({"method":null}),
        cadmpeg_core::json!({"groups":null}),
        cadmpeg_core::json!({"multiple_bodies":null}),
        cadmpeg_core::json!({"keep_body":null}),
        cadmpeg_core::json!({"method":"other"}),
        cadmpeg_core::json!({"unknown":true}),
        cadmpeg_core::json!({"groups":{"kind":"all", "members":["alpha"]}}),
        cadmpeg_core::json!({"groups":{"kind":"selected", "members":null}}),
        cadmpeg_core::json!({"groups":{"kind":"selected", "members":[]}}),
        cadmpeg_core::json!({"groups":{"kind":"selected", "members":["alpha", "alpha"]}}),
        cadmpeg_core::json!({"groups":{"kind":"selected", "members":[""]}}),
        cadmpeg_core::json!({"groups":{"kind":"selected", "extra":1}}),
        cadmpeg_core::json!({"keep_body":1}),
    ] {
        assert!(json::from_value::<MeshSeparation>(invalid).is_err());
    }
}

#[test]
fn separation_rewrite_traverses_owned_group_selectors_and_source_identity() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};
    let scope = "f3d:Design/BulkStream.dat:design-parameter-scope#57";
    let renamed = "f3d:child/Design/BulkStream.dat:design-parameter-scope#57";
    let replacements = std::collections::BTreeMap::from([(scope.to_owned(), renamed.to_owned())]);
    let wire = cadmpeg_core::json!({"kind": "mesh_modify", "value": {"source":{"kind":"unresolved","value":{"feature":"test:model:feature#separate"}}, "modification":{"kind":"separate", "separation":{"groups":{"kind":"selected", "members":[scope, format!("{scope}/alpha"), "beta"]}, "keep_body":false}}}});
    let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace("test:", "test:child/")))
            .with_text_replacements(&text);
        let rewritten = operation.rewrite_identities(ctx, &mut map)?;
        let mut expected = wire.clone();
        expected["value"]["source"]["value"]["feature"] =
            "test:child/model:feature#separate".into();
        expected["value"]["modification"]["separation"]["groups"]["members"][0] = renamed.into();
        assert_eq!(json::to_value(rewritten).unwrap(), expected);
        map.finish()
    })
    .unwrap();
}

#[test]
fn face_group_operations_round_trip_partial_settings_and_rewrite_sources() {
    use crate::features::FeatureOperation;
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    for wire in [
        cadmpeg_core::json!({"kind": "mesh_generate_face_groups", "value": {"source":{"kind":"unresolved","value":{"feature":"test:model:feature#groups"}}, "settings":{"boundary_tolerance":0.047, "minimum_area_fraction":0.17}}}),
        cadmpeg_core::json!({"kind": "mesh_combine_face_groups", "value": {"source":{"kind":"tessellations","value":["test:mesh:tessellation#first"]}}}),
        cadmpeg_core::json!({"kind": "mesh_combine_face_groups", "value": {"source":{"kind":"native","value":{"native":"native-mesh", "members":["second","first"]}}}}),
    ] {
        let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
        assert_eq!(json::to_value(&operation).unwrap(), wire);
        crate::test_support::with_service_decode_context(|ctx| {
            let mut map =
                IdentityMap::new(|source: &str| Ok(source.replace("test:", "test:child/")));
            let rewritten = operation.rewrite_identities(ctx, &mut map)?;
            let rewritten = json::to_value(rewritten).unwrap();
            let source = &wire["value"]["source"];
            if let Some(feature) = source["value"].get("feature") {
                assert_eq!(
                    rewritten["value"]["source"]["value"]["feature"],
                    feature.as_str().unwrap().replace("test:", "test:child/")
                );
            } else if source["value"].is_array() {
                assert_eq!(
                    rewritten["value"]["source"]["value"][0],
                    "test:child/mesh:tessellation#first"
                );
            } else {
                assert_eq!(rewritten, wire);
            }
            Ok(())
        })
        .unwrap();
    }
    for settings in [
        cadmpeg_core::json!({"method":"other"}),
        cadmpeg_core::json!({"boundary_tolerance":-0.5}),
        cadmpeg_core::json!({"minimum_area_fraction":1.1}),
        cadmpeg_core::json!({"angle_threshold":null}),
    ] {
        assert!(json::from_value::<FeatureOperation>(cadmpeg_core::json!({"kind": "mesh_generate_face_groups", "value": {"source":{"kind":"native","value":{"native":"mesh"}}, "settings":settings}})).is_err());
    }
}

#[test]
fn resolved_mesh_selection_preserves_the_tessellation_array_wire_form() {
    let wire = cadmpeg_core::json!({"kind": "tessellations", "value": [
        "test:mesh:tessellation#first",
        "test:mesh:tessellation#second"
    ]});
    let selection: MeshSelection = json::from_value(wire.clone()).unwrap();
    let MeshSelection::Tessellations(values) = &selection else {
        panic!("resolved tessellations");
    };
    assert_eq!(
        values
            .iter()
            .map(crate::tessellation::TessellationId::as_str)
            .collect::<Vec<_>>(),
        [
            "test:mesh:tessellation#first",
            "test:mesh:tessellation#second"
        ]
    );
    assert_eq!(json::to_value(selection).unwrap(), wire);
}

#[test]
fn native_mesh_selection_preserves_source_order() {
    let wire = cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:source", "members":["7", "3"]}});
    let selection: MeshSelection = json::from_value(wire.clone()).unwrap();
    let MeshSelection::Native { native, members } = &selection else {
        panic!("native sources");
    };
    assert_eq!(native.as_str(), "mesh:source");
    assert_eq!(members.as_ref().unwrap().as_slice(), ["7", "3"]);
    assert_eq!(json::to_value(selection).unwrap(), wire);
}

#[test]
fn mesh_selections_reject_empty_repeated_blank_and_mixed_members() {
    for wire in [
        cadmpeg_core::json!({"kind":"tessellations","value":[]}),
        cadmpeg_core::json!({"kind":"tessellations","value":[
            "test:mesh:tessellation#first",
            "test:mesh:tessellation#first"
        ]}),
        cadmpeg_core::json!({"kind":"tessellations","value":["mesh id"]}),
        cadmpeg_core::json!({"kind":"native","value":{"native":" "}}),
        cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:source", "members":[]}}),
        cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:source", "members":null}}),
        cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:source", "members":[" "]}}),
        cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:source", "members":["7", "7"]}}),
        cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:source", "tessellations":["mesh:second"]}}),
        cadmpeg_core::json!({"kind":"unresolved","value":{"feature":" "}}),
        cadmpeg_core::json!({"kind":"unresolved","value":{"feature":"test:model:feature#import", "native":"mesh:source"}}),
    ] {
        assert!(json::from_value::<MeshSelection>(wire).is_err());
    }
}

#[test]
fn mesh_sources_rewrite_resolved_identities_and_declared_native_scopes() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

    let source_scope = "f3d:Design/BulkStream.dat:design-parameter-scope#7";
    let target_scope = "f3d:child@Design/BulkStream.dat:design-parameter-scope#7";
    let replacements =
        std::collections::BTreeMap::from([(source_scope.to_owned(), target_scope.to_owned())]);
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|source: &str| {
            let source = source.strip_prefix("f3d:").unwrap();
            Ok(format!("f3d:child@{source}"))
        })
        .with_text_replacements(&text);
        let resolved: MeshSelection = json::from_value(cadmpeg_core::json!({"kind": "tessellations", "value": [
            "f3d:mesh:tessellation#first",
            "f3d:mesh:tessellation#second"
        ]}))
        .unwrap();
        let mut visited = Vec::new();
        resolved.visit_identity_references(ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })?;
        assert_eq!(
            visited,
            [
                "f3d:mesh:tessellation#first",
                "f3d:mesh:tessellation#second"
            ]
        );
        let resolved = resolved.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            json::to_value(resolved).unwrap(),
            cadmpeg_core::json!({"kind": "tessellations", "value": [
                "f3d:child@mesh:tessellation#first",
                "f3d:child@mesh:tessellation#second"
            ]})
        );
        let native: MeshSelection = json::from_value(cadmpeg_core::json!({"kind": "native", "value": {
            "native":source_scope, "members":["7", "3"]
        }}))
        .unwrap();
        let native = native.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            json::to_value(native).unwrap(),
            cadmpeg_core::json!({"kind": "native", "value": {
                "native":target_scope, "members":["7", "3"]
            }})
        );
        let unresolved: MeshSelection = json::from_value(cadmpeg_core::json!({"kind": "unresolved", "value": {
            "feature":"f3d:model:feature#import"
        }}))
        .unwrap();
        let mut visited = Vec::new();
        unresolved.visit_identity_references(ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })?;
        assert_eq!(visited, ["f3d:model:feature#import"]);
        let unresolved = unresolved.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            json::to_value(unresolved).unwrap(),
            cadmpeg_core::json!({"kind": "unresolved", "value": {"feature":"f3d:child@model:feature#import"}})
        );
        map.finish()
    })
    .unwrap();
}

#[test]
fn mesh_modifications_keep_their_tagged_wire_forms() {
    for (wire, expected) in [
        (
            cadmpeg_core::json!({"kind":"reduce","target":{"kind":"maximum_deviation","value":0.0}}),
            MeshModification::Reduce {
                distribution: None,
                target: Some(MeshReduceTarget::MaximumDeviation {
                    value: crate::scalar::NonNegativeLength::new(0.0).unwrap(),
                }),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"reduce","distribution":"uniform","target":{"kind":"face_count","value":6}}),
            MeshModification::Reduce {
                distribution: Some(MeshFaceDistribution::Uniform),
                target: Some(MeshReduceTarget::FaceCount {
                    value: std::num::NonZeroU32::new(6).unwrap(),
                }),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"reduce","target":{"kind":"face_proportion","value":0.5}}),
            MeshModification::Reduce {
                distribution: None,
                target: Some(MeshReduceTarget::FaceProportion {
                    value: crate::scalar::Fraction::new(0.5).unwrap(),
                }),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"repair","method":{"kind":"rebuild","mode":"accurate","density":8.0,"offset":0.5}}),
            MeshModification::Repair {
                method: Some(MeshRepair::Rebuild {
                    mode: Some(MeshRebuildMode::Accurate),
                    density: Some(crate::scalar::PositiveReal::new(8.0).unwrap()),
                    offset: Some(crate::scalar::Length::new(0.5).unwrap()),
                }),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"repair","method":{"kind":"close_holes"}}),
            MeshModification::Repair {
                method: Some(MeshRepair::CloseHoles {}),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"repair","method":{"kind":"stitch_and_remove"}}),
            MeshModification::Repair {
                method: Some(MeshRepair::StitchAndRemove {}),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"remesh","distribution":"adaptive","edge_length":15.0,"shape_preservation":0.5}),
            MeshModification::Remesh {
                distribution: Some(MeshFaceDistribution::Adaptive),
                edge_length: Some(crate::scalar::PositiveLength::new(15.0).unwrap()),
                shape_preservation: Some(crate::scalar::Fraction::new(0.5).unwrap()),
            },
        ),
        (
            cadmpeg_core::json!({"kind":"smooth"}),
            MeshModification::Smooth { smoothness: None },
        ),
    ] {
        let decoded: MeshModification = json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(json::to_value(decoded).unwrap(), wire);
    }
    for invalid in [
        cadmpeg_core::json!({"kind":"smooth","smoothness":1.5}),
        cadmpeg_core::json!({"kind":"smooth","smoothness":null}),
        cadmpeg_core::json!({"kind":"shell","thickness":2.0}),
        cadmpeg_core::json!({"kind":"reverse_normals"}),
        cadmpeg_core::json!({"kind":"reduce","target":{"kind":"face_count","value":0}}),
    ] {
        assert!(json::from_value::<MeshModification>(invalid).is_err());
    }
}

#[test]
fn mesh_reduction_bounds_and_repair_methods_reject_unknown_keys() {
    for target in [
        cadmpeg_core::json!({"kind":"maximum_deviation","value":1.0,"extra":true}),
        cadmpeg_core::json!({"kind":"face_proportion","value":0.5,"extra":true}),
        cadmpeg_core::json!({"kind":"face_count","value":6,"extra":true}),
        cadmpeg_core::json!({"kind":"face_count","value":0}),
        cadmpeg_core::json!({"kind":"face_count","value":null}),
        cadmpeg_core::json!({"kind":"face_count","value":6.5}),
    ] {
        assert!(json::from_value::<MeshReduceTarget>(target).is_err());
    }
    for method in [
        cadmpeg_core::json!({"kind":"close_holes","extra":true}),
        cadmpeg_core::json!({"kind":"stitch_and_remove","extra":true}),
    ] {
        assert!(json::from_value::<MeshRepair>(method).is_err());
    }
}

#[test]
fn mesh_operations_keep_their_feature_wire_forms() {
    for wire in [
        cadmpeg_core::json!({"kind": "mesh_modify", "value": {
            "source":{"kind":"native","value":{"native":"mesh:scope","members":["9"]}},
            "modification":{"kind":"smooth","smoothness":0.25}
        }}),
        cadmpeg_core::json!({"kind": "mesh_section", "value": {
            "source":{"kind":"tessellations","value":["test:mesh:tessellation#first"]},
            "sketch":"test:model:feature#sketch",
            "plane":{"origin":{"x":1.0,"y":2.0,"z":3.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}
        }}),
        cadmpeg_core::json!({"kind": "mesh_section", "value": {
            "source":{"kind":"unresolved","value":{"feature":"test:model:feature#section"}}
        }}),
    ] {
        let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            operation,
            FeatureOperation::MeshModify { .. } | FeatureOperation::MeshSection { .. }
        ));
        assert_eq!(json::to_value(operation).unwrap(), wire);
    }
}

#[test]
fn section_plane_rejects_explicit_null_and_nonperpendicular_axes() {
    for plane in [
        serde_json::Value::Null,
        cadmpeg_core::json!({
            "origin":{"x":0.0,"y":0.0,"z":0.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":0.0,"y":0.0,"z":1.0}
        }),
    ] {
        let wire = cadmpeg_core::json!({"kind": "mesh_section", "value": {
            "source":{"kind":"unresolved","value":{"feature":"test:model:feature#section"}},
            "plane":plane
        }});
        assert!(json::from_value::<FeatureOperation>(wire).is_err());
    }
}

#[test]
fn section_identity_rewrite_keeps_its_plane_and_linked_sketch() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    let wire = cadmpeg_core::json!({"kind": "mesh_section", "value": {
        "source":{"kind":"unresolved","value":{"feature":"test:model:feature#section"}},
        "plane":{
            "origin":{"x":1.0,"y":2.0,"z":3.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0}
        },
        "sketch":"test:model:feature#sketch"
    }});
    let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
    crate::test_support::with_service_decode_context(|ctx| {
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace("test:", "test:child/")));
        let mut identities = Vec::new();
        operation.visit_identity_references(ctx, &mut |id| {
            identities.push(id.to_owned());
            Ok(())
        })?;
        assert_eq!(
            identities,
            ["test:model:feature#section", "test:model:feature#sketch"]
        );
        let rewritten = operation.rewrite_identities(ctx, &mut map)?;
        let mut expected = wire.clone();
        expected["value"]["source"]["value"]["feature"] = "test:child/model:feature#section".into();
        expected["value"]["sketch"] = "test:child/model:feature#sketch".into();
        assert_eq!(json::to_value(rewritten).unwrap(), expected);
        map.finish()
    })
    .unwrap();
}

#[test]
fn conversion_wire_preserves_ordered_source_contexts() {
    let wire = cadmpeg_core::json!({"kind": "mesh_convert", "value": {
        "sources":[
            {"kind":"native","value":{"native":"mesh:scope","members":["9", "3"]}},
            {"kind":"native","value":{"native":"mesh:scope","members":["9", "3"]}},
            {"kind":"unresolved","value":{"feature":"test:model:feature#convert"}}
        ]
    }});
    let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
    let FeatureOperation::MeshConvert(conversion) = &operation else {
        panic!("mesh conversion");
    };
    assert_eq!(conversion.sources.len(), 3);
    assert_eq!(conversion.sources[0], conversion.sources[1]);
    assert_eq!(json::to_value(operation).unwrap(), wire);
    for method in ["faceted", "prismatic"] {
        let mut known = wire.clone();
        known["value"]["method"] = method.into();
        let operation: FeatureOperation = json::from_value(known.clone()).unwrap();
        assert_eq!(json::to_value(operation).unwrap(), known);
    }
    for method in [serde_json::Value::Null, cadmpeg_core::json!("unruled")] {
        let mut invalid = wire.clone();
        invalid["value"]["method"] = method;
        assert!(json::from_value::<FeatureOperation>(invalid).is_err());
    }
    for sources in [
        serde_json::Value::Null,
        cadmpeg_core::json!({"kind":"native","value":{"native":"mesh:scope"}}),
        cadmpeg_core::json!([{"kind":"native","value":{"native":"mesh:scope","members":[]}}]),
        cadmpeg_core::json!([{"kind":"unresolved","value":{"feature":" "}}]),
    ] {
        assert!(json::from_value::<FeatureOperation>(
            cadmpeg_core::json!({"kind": "mesh_convert", "value": {
                "sources":sources
            }})
        )
        .is_err());
    }
}

#[test]
fn conversion_rewrites_declared_native_and_typed_source_identities() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

    let source_scope = "f3d:Design/BulkStream.dat:design-parameter-scope#7";
    let target_scope = "f3d:child@Design/BulkStream.dat:design-parameter-scope#7";
    let replacements =
        std::collections::BTreeMap::from([(source_scope.to_owned(), target_scope.to_owned())]);
    let wire = cadmpeg_core::json!({"kind": "mesh_convert", "value": {
        "sources":[
            {"kind":"native","value":{"native":source_scope,"members":["9", "3"]}},
            {"kind":"tessellations","value":["f3d:mesh:tessellation#source"]},
            {"kind":"unresolved","value":{"feature":"f3d:model:feature#convert"}}
        ]
    }});
    let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|id: &str| {
            Ok(format!("f3d:child@{}", id.strip_prefix("f3d:").unwrap()))
        })
        .with_text_replacements(&text);
        let mut visited = Vec::new();
        operation.visit_identity_references(ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })?;
        assert_eq!(
            visited,
            ["f3d:mesh:tessellation#source", "f3d:model:feature#convert"]
        );
        let rewritten = operation.rewrite_identities(ctx, &mut map)?;
        let mut expected = wire.clone();
        let sources = &mut expected["value"]["sources"];
        sources[0]["value"]["native"] = target_scope.into();
        sources[1]["value"][0] = "f3d:child@mesh:tessellation#source".into();
        sources[2]["value"]["feature"] = "f3d:child@model:feature#convert".into();
        assert_eq!(json::to_value(rewritten).unwrap(), expected);
        map.finish()
    })
    .unwrap();
}

#[test]
fn plane_cut_round_trips_independent_components_and_rewrites_sources() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    for cut in [
        cadmpeg_core::json!({}),
        cadmpeg_core::json!({"origin":{"x":2.0,"y":3.0,"z":5.0}}),
        cadmpeg_core::json!({"normal":{"x":0.0,"y":0.0,"z":2.0},"mode":"split_faces"}),
        cadmpeg_core::json!({"mode":"trim","fill":"no_fill","retained_side":"negative"}),
        cadmpeg_core::json!({"mode":"split_bodies","fill":"uniform","retained_side":"positive"}),
    ] {
        for source in [
            cadmpeg_core::json!({"kind":"unresolved","value":{"feature":"test:model:feature#cut"}}),
            cadmpeg_core::json!({"kind":"tessellations","value":["test:mesh:tessellation#source"]}),
            cadmpeg_core::json!({"kind":"native","value":{"native":"scope","members":["9","7"]}}),
        ] {
            let wire = cadmpeg_core::json!({"kind": "mesh_modify", "value": {"source":source,"modification":{"kind":"plane_cut","cut":cut}}});
            let operation: FeatureOperation = json::from_value(wire.clone()).unwrap();
            assert_eq!(json::to_value(&operation).unwrap(), wire);
            crate::test_support::with_service_decode_context(|ctx| {
                let mut identities =
                    IdentityMap::new(|source: &str| Ok(source.replace("test:", "test:child/")));
                let cloned = operation.rewrite_identities(ctx, &mut identities)?;
                let cloned = json::to_value(cloned).unwrap();
                assert_eq!(cloned["value"]["modification"], wire["value"]["modification"]);
                assert_eq!(
                    cloned["value"]["source"],
                    if source["value"].get("feature").is_some() {
                        cadmpeg_core::json!({"kind":"unresolved","value":{"feature":"test:child/model:feature#cut"}})
                    } else if source["value"].is_array() {
                        cadmpeg_core::json!({"kind":"tessellations","value":["test:child/mesh:tessellation#source"]})
                    } else {
                        source.clone()
                    }
                );
                Ok(())
            })
            .unwrap();
        }
    }
    assert_eq!(
        json::to_value(super::MeshPlaneCut::default()).unwrap(),
        cadmpeg_core::json!({})
    );
}

#[test]
fn plane_cut_rejects_null_unknown_keys_and_invalid_checked_geometry() {
    for key in ["origin", "normal", "mode", "fill", "retained_side"] {
        let mut wire = cadmpeg_core::json!({});
        wire[key] = serde_json::Value::Null;
        let error = json::from_value::<super::MeshPlaneCut>(wire).unwrap_err();
        assert!(error.to_string().contains(key));
    }
    for wire in [
        cadmpeg_core::json!({"normal":{"x":0,"y":0,"z":0}}),
        cadmpeg_core::json!({"normal":{"x":1e308,"y":0,"z":0}}),
        cadmpeg_core::json!({"mode":"other"}),
        cadmpeg_core::json!({"fill":"other"}),
        cadmpeg_core::json!({"retained_side":"other"}),
        cadmpeg_core::json!({"u_axis":{"x":1,"y":0,"z":0}}),
    ] {
        assert!(json::from_value::<super::MeshPlaneCut>(wire).is_err());
    }
}
