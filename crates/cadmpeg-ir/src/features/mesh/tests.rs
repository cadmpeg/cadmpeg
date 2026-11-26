use super::{
    MeshFaceDistribution, MeshModification, MeshRebuildMode, MeshReduceTarget, MeshRepair,
    MeshSelection,
};
use crate::features::FeatureOperation;

#[test]
fn separation_wire_keeps_independent_controls_and_selected_category() {
    use super::{MeshSeparation, MeshSeparationGroups};
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(MeshSeparation::default()).unwrap(),
        serde_json::json!({})
    );
    for separation in [
        serde_json::json!({}),
        serde_json::json!({"method":"shells", "multiple_bodies":true, "keep_body":false}),
        serde_json::json!({"groups":{"kind":"all"}, "keep_body":true}),
        serde_json::json!({"method":"face_groups", "groups":{"kind":"selected"}}),
        serde_json::json!({"groups":{"kind":"selected", "members":["beta", "alpha"]}, "multiple_bodies":false}),
    ] {
        let wire = serde_json::json!({"kind": "mesh_modify", "value": {"source":{"kind":"unresolved","value":{"feature":"test:model:feature#separate"}}, "modification":{"kind":"separate", "separation":separation}}});
        let decoded: FeatureOperation =
            cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(decoded).unwrap(),
            wire
        );
    }
    let selected: MeshSeparation = cadmpeg_core::serde_erase::json::from_value(
        serde_json::json!({"groups":{"kind":"selected"}}),
    )
    .unwrap();
    assert_eq!(
        selected.groups,
        Some(MeshSeparationGroups::Selected { members: None })
    );
    for invalid in [
        serde_json::json!({"method":null}),
        serde_json::json!({"groups":null}),
        serde_json::json!({"multiple_bodies":null}),
        serde_json::json!({"keep_body":null}),
        serde_json::json!({"method":"other"}),
        serde_json::json!({"unknown":true}),
        serde_json::json!({"groups":{"kind":"all", "members":["alpha"]}}),
        serde_json::json!({"groups":{"kind":"selected", "members":null}}),
        serde_json::json!({"groups":{"kind":"selected", "members":[]}}),
        serde_json::json!({"groups":{"kind":"selected", "members":["alpha", "alpha"]}}),
        serde_json::json!({"groups":{"kind":"selected", "members":[""]}}),
        serde_json::json!({"groups":{"kind":"selected", "extra":1}}),
        serde_json::json!({"keep_body":1}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<MeshSeparation>(invalid).is_err());
    }
}

#[test]
fn separation_rewrite_traverses_owned_group_selectors_and_source_identity() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};
    let scope = "f3d:Design/BulkStream.dat:design-parameter-scope#57";
    let renamed = "f3d:child/Design/BulkStream.dat:design-parameter-scope#57";
    let replacements = std::collections::BTreeMap::from([(scope.to_owned(), renamed.to_owned())]);
    let wire = serde_json::json!({"kind": "mesh_modify", "value": {"source":{"kind":"unresolved","value":{"feature":"test:model:feature#separate"}}, "modification":{"kind":"separate", "separation":{"groups":{"kind":"selected", "members":[scope, format!("{scope}/alpha"), "beta"]}, "keep_body":false}}}});
    let operation: FeatureOperation =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|id: &str| Ok(id.replace("test:", "test:child/")))
            .with_text_replacements(&text);
        let rewritten = operation.rewrite_identities(ctx, &mut map)?;
        let mut expected = wire.clone();
        expected["value"]["source"]["value"]["feature"] =
            "test:child/model:feature#separate".into();
        expected["value"]["modification"]["separation"]["groups"]["members"][0] = renamed.into();
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(rewritten).unwrap(),
            expected
        );
        map.finish()
    })
    .unwrap();
}

#[cfg(feature = "schema")]
#[test]
fn separation_schema_matches_optional_nonnull_settings() {
    use super::{MeshSeparation, MeshSeparationGroups};
    let schema =
        cadmpeg_core::serde_erase::json::to_value(schemars::schema_for!(MeshSeparation)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    for name in ["method", "groups", "multiple_bodies", "keep_body"] {
        assert!(!schema["required"]
            .as_array()
            .is_some_and(|required| required.contains(&serde_json::json!(name))));
        assert!(!schema["properties"][name].to_string().contains("\"null\""));
    }
    assert_eq!(schema["properties"]["keep_body"]["type"], "boolean");
    let groups =
        cadmpeg_core::serde_erase::json::to_value(schemars::schema_for!(MeshSeparationGroups))
            .unwrap();
    assert!(!groups.to_string().contains("\"null\""));
}

#[test]
fn face_group_operations_round_trip_partial_settings_and_rewrite_sources() {
    use crate::features::FeatureOperation;
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    for wire in [
        serde_json::json!({"kind": "mesh_generate_face_groups", "value": {"source":{"kind":"unresolved","value":{"feature":"test:model:feature#groups"}}, "settings":{"boundary_tolerance":0.047, "minimum_area_fraction":0.17}}}),
        serde_json::json!({"kind": "mesh_combine_face_groups", "value": {"source":{"kind":"tessellations","value":["test:mesh:tessellation#first"]}}}),
        serde_json::json!({"kind": "mesh_combine_face_groups", "value": {"source":{"kind":"native","value":{"native":"native-mesh", "members":["second","first"]}}}}),
    ] {
        let operation: FeatureOperation =
            cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(&operation).unwrap(),
            wire
        );
        crate::test_support::with_service_decode_context(|ctx| {
            let mut map =
                IdentityMap::new(|source: &str| Ok(source.replace("test:", "test:child/")));
            let rewritten = operation.rewrite_identities(ctx, &mut map)?;
            let rewritten = cadmpeg_core::serde_erase::json::to_value(rewritten).unwrap();
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
        serde_json::json!({"method":"other"}),
        serde_json::json!({"boundary_tolerance":-0.5}),
        serde_json::json!({"minimum_area_fraction":1.1}),
        serde_json::json!({"angle_threshold":null}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(serde_json::json!({"kind": "mesh_generate_face_groups", "value": {"source":{"kind":"native","value":{"native":"mesh"}}, "settings":settings}})).is_err());
    }
}

#[test]
fn resolved_mesh_selection_preserves_the_tessellation_array_wire_form() {
    let wire = serde_json::json!({"kind": "tessellations", "value": [
        "test:mesh:tessellation#first",
        "test:mesh:tessellation#second"
    ]});
    let selection: MeshSelection =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
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
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(selection).unwrap(),
        wire
    );
}

#[test]
fn native_mesh_selection_preserves_source_order() {
    let wire =
        serde_json::json!({"kind":"native","value":{"native":"mesh:source", "members":["7", "3"]}});
    let selection: MeshSelection =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
    let MeshSelection::Native { native, members } = &selection else {
        panic!("native sources");
    };
    assert_eq!(native.as_str(), "mesh:source");
    assert_eq!(members.as_ref().unwrap().as_slice(), ["7", "3"]);
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(selection).unwrap(),
        wire
    );
}

#[test]
fn mesh_selections_reject_empty_repeated_blank_and_mixed_members() {
    for wire in [
        serde_json::json!({"kind":"tessellations","value":[]}),
        serde_json::json!({"kind":"tessellations","value":[
            "test:mesh:tessellation#first",
            "test:mesh:tessellation#first"
        ]}),
        serde_json::json!({"kind":"tessellations","value":["mesh id"]}),
        serde_json::json!({"kind":"native","value":{"native":" "}}),
        serde_json::json!({"kind":"native","value":{"native":"mesh:source", "members":[]}}),
        serde_json::json!({"kind":"native","value":{"native":"mesh:source", "members":null}}),
        serde_json::json!({"kind":"native","value":{"native":"mesh:source", "members":[" "]}}),
        serde_json::json!({"kind":"native","value":{"native":"mesh:source", "members":["7", "7"]}}),
        serde_json::json!({"kind":"native","value":{"native":"mesh:source", "tessellations":["mesh:second"]}}),
        serde_json::json!({"kind":"unresolved","value":{"feature":" "}}),
        serde_json::json!({"kind":"unresolved","value":{"feature":"test:model:feature#import", "native":"mesh:source"}}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<MeshSelection>(wire).is_err());
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
        let resolved: MeshSelection = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"kind": "tessellations", "value": [
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
            cadmpeg_core::serde_erase::json::to_value(resolved).unwrap(),
            serde_json::json!({"kind": "tessellations", "value": [
                "f3d:child@mesh:tessellation#first",
                "f3d:child@mesh:tessellation#second"
            ]})
        );
        let native: MeshSelection = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"kind": "native", "value": {
            "native":source_scope, "members":["7", "3"]
        }}))
        .unwrap();
        let native = native.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(native).unwrap(),
            serde_json::json!({"kind": "native", "value": {
                "native":target_scope, "members":["7", "3"]
            }})
        );
        let unresolved: MeshSelection = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({"kind": "unresolved", "value": {
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
            cadmpeg_core::serde_erase::json::to_value(unresolved).unwrap(),
            serde_json::json!({"kind": "unresolved", "value": {"feature":"f3d:child@model:feature#import"}})
        );
        map.finish()
    })
    .unwrap();
}

#[test]
fn mesh_modifications_keep_their_tagged_wire_forms() {
    for (wire, expected) in [
        (
            serde_json::json!({"kind":"reduce","target":{"kind":"maximum_deviation","value":0.0}}),
            MeshModification::Reduce {
                distribution: None,
                target: Some(MeshReduceTarget::MaximumDeviation {
                    value: crate::scalar::NonNegativeLength::new(0.0).unwrap(),
                }),
            },
        ),
        (
            serde_json::json!({"kind":"reduce","distribution":"uniform","target":{"kind":"face_count","value":6}}),
            MeshModification::Reduce {
                distribution: Some(MeshFaceDistribution::Uniform),
                target: Some(MeshReduceTarget::FaceCount {
                    value: std::num::NonZeroU32::new(6).unwrap(),
                }),
            },
        ),
        (
            serde_json::json!({"kind":"reduce","target":{"kind":"face_proportion","value":0.5}}),
            MeshModification::Reduce {
                distribution: None,
                target: Some(MeshReduceTarget::FaceProportion {
                    value: crate::scalar::Fraction::new(0.5).unwrap(),
                }),
            },
        ),
        (
            serde_json::json!({"kind":"repair","method":{"kind":"rebuild","mode":"accurate","density":8.0,"offset":0.5}}),
            MeshModification::Repair {
                method: Some(MeshRepair::Rebuild {
                    mode: Some(MeshRebuildMode::Accurate),
                    density: Some(crate::scalar::PositiveReal::new(8.0).unwrap()),
                    offset: Some(crate::scalar::Length::new(0.5).unwrap()),
                }),
            },
        ),
        (
            serde_json::json!({"kind":"repair","method":{"kind":"close_holes"}}),
            MeshModification::Repair {
                method: Some(MeshRepair::CloseHoles {}),
            },
        ),
        (
            serde_json::json!({"kind":"repair","method":{"kind":"stitch_and_remove"}}),
            MeshModification::Repair {
                method: Some(MeshRepair::StitchAndRemove {}),
            },
        ),
        (
            serde_json::json!({"kind":"remesh","distribution":"adaptive","edge_length":15.0,"shape_preservation":0.5}),
            MeshModification::Remesh {
                distribution: Some(MeshFaceDistribution::Adaptive),
                edge_length: Some(crate::scalar::PositiveLength::new(15.0).unwrap()),
                shape_preservation: Some(crate::scalar::Fraction::new(0.5).unwrap()),
            },
        ),
        (
            serde_json::json!({"kind":"smooth"}),
            MeshModification::Smooth { smoothness: None },
        ),
    ] {
        let decoded: MeshModification =
            cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(decoded).unwrap(),
            wire
        );
    }
    for invalid in [
        serde_json::json!({"kind":"smooth","smoothness":1.5}),
        serde_json::json!({"kind":"smooth","smoothness":null}),
        serde_json::json!({"kind":"shell","thickness":2.0}),
        serde_json::json!({"kind":"reverse_normals"}),
        serde_json::json!({"kind":"reduce","target":{"kind":"face_count","value":0}}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<MeshModification>(invalid).is_err());
    }
}

#[test]
fn mesh_reduction_bounds_and_repair_methods_reject_unknown_keys() {
    for target in [
        serde_json::json!({"kind":"maximum_deviation","value":1.0,"extra":true}),
        serde_json::json!({"kind":"face_proportion","value":0.5,"extra":true}),
        serde_json::json!({"kind":"face_count","value":6,"extra":true}),
        serde_json::json!({"kind":"face_count","value":0}),
        serde_json::json!({"kind":"face_count","value":null}),
        serde_json::json!({"kind":"face_count","value":6.5}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<MeshReduceTarget>(target).is_err());
    }
    for method in [
        serde_json::json!({"kind":"close_holes","extra":true}),
        serde_json::json!({"kind":"stitch_and_remove","extra":true}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<MeshRepair>(method).is_err());
    }
}

#[test]
fn mesh_operations_keep_their_feature_wire_forms() {
    for wire in [
        serde_json::json!({"kind": "mesh_modify", "value": {
            "source":{"kind":"native","value":{"native":"mesh:scope","members":["9"]}},
            "modification":{"kind":"smooth","smoothness":0.25}
        }}),
        serde_json::json!({"kind": "mesh_section", "value": {
            "source":{"kind":"tessellations","value":["test:mesh:tessellation#first"]},
            "sketch":"test:model:feature#sketch",
            "plane":{"origin":{"x":1.0,"y":2.0,"z":3.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}
        }}),
        serde_json::json!({"kind": "mesh_section", "value": {
            "source":{"kind":"unresolved","value":{"feature":"test:model:feature#section"}}
        }}),
    ] {
        let operation: FeatureOperation =
            cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            operation,
            FeatureOperation::MeshModify { .. } | FeatureOperation::MeshSection { .. }
        ));
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(operation).unwrap(),
            wire
        );
    }
}

#[test]
fn section_plane_rejects_explicit_null_and_nonperpendicular_axes() {
    for plane in [
        serde_json::Value::Null,
        serde_json::json!({
            "origin":{"x":0.0,"y":0.0,"z":0.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":0.0,"y":0.0,"z":1.0}
        }),
    ] {
        let wire = serde_json::json!({"kind": "mesh_section", "value": {
            "source":{"kind":"unresolved","value":{"feature":"test:model:feature#section"}},
            "plane":plane
        }});
        assert!(cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(wire).is_err());
    }
}

#[test]
fn section_identity_rewrite_keeps_its_plane_and_linked_sketch() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    let wire = serde_json::json!({"kind": "mesh_section", "value": {
        "source":{"kind":"unresolved","value":{"feature":"test:model:feature#section"}},
        "plane":{
            "origin":{"x":1.0,"y":2.0,"z":3.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0}
        },
        "sketch":"test:model:feature#sketch"
    }});
    let operation: FeatureOperation =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
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
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(rewritten).unwrap(),
            expected
        );
        map.finish()
    })
    .unwrap();
}

#[test]
fn conversion_wire_preserves_ordered_source_contexts() {
    let wire = serde_json::json!({"kind": "mesh_convert", "value": {
        "sources":[
            {"kind":"native","value":{"native":"mesh:scope","members":["9", "3"]}},
            {"kind":"native","value":{"native":"mesh:scope","members":["9", "3"]}},
            {"kind":"unresolved","value":{"feature":"test:model:feature#convert"}}
        ]
    }});
    let operation: FeatureOperation =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
    let FeatureOperation::MeshConvert(conversion) = &operation else {
        panic!("mesh conversion");
    };
    assert_eq!(conversion.sources.len(), 3);
    assert_eq!(conversion.sources[0], conversion.sources[1]);
    assert_eq!(
        cadmpeg_core::serde_erase::json::to_value(operation).unwrap(),
        wire
    );
    for method in ["faceted", "prismatic"] {
        let mut known = wire.clone();
        known["value"]["method"] = method.into();
        let operation: FeatureOperation =
            cadmpeg_core::serde_erase::json::from_value(known.clone()).unwrap();
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(operation).unwrap(),
            known
        );
    }
    for method in [serde_json::Value::Null, serde_json::json!("unruled")] {
        let mut invalid = wire.clone();
        invalid["value"]["method"] = method;
        assert!(cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(invalid).is_err());
    }
    for sources in [
        serde_json::Value::Null,
        serde_json::json!({"kind":"native","value":{"native":"mesh:scope"}}),
        serde_json::json!([{"kind":"native","value":{"native":"mesh:scope","members":[]}}]),
        serde_json::json!([{"kind":"unresolved","value":{"feature":" "}}]),
    ] {
        assert!(
            cadmpeg_core::serde_erase::json::from_value::<FeatureOperation>(
                serde_json::json!({"kind": "mesh_convert", "value": {
                    "sources":sources
                }})
            )
            .is_err()
        );
    }
}

#[test]
fn conversion_rewrites_declared_native_and_typed_source_identities() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

    let source_scope = "f3d:Design/BulkStream.dat:design-parameter-scope#7";
    let target_scope = "f3d:child@Design/BulkStream.dat:design-parameter-scope#7";
    let replacements =
        std::collections::BTreeMap::from([(source_scope.to_owned(), target_scope.to_owned())]);
    let wire = serde_json::json!({"kind": "mesh_convert", "value": {
        "sources":[
            {"kind":"native","value":{"native":source_scope,"members":["9", "3"]}},
            {"kind":"tessellations","value":["f3d:mesh:tessellation#source"]},
            {"kind":"unresolved","value":{"feature":"f3d:model:feature#convert"}}
        ]
    }});
    let operation: FeatureOperation =
        cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
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
        assert_eq!(
            cadmpeg_core::serde_erase::json::to_value(rewritten).unwrap(),
            expected
        );
        map.finish()
    })
    .unwrap();
}

#[cfg(feature = "schema")]
#[test]
fn conversion_schema_exposes_sources_on_the_tagged_operation() {
    fn find_conversion(value: &serde_json::Value) -> Option<&serde_json::Value> {
        if value["properties"]["kind"]["const"] == "mesh_convert"
            || value["properties"]["kind"]["enum"] == serde_json::json!(["mesh_convert"])
        {
            return Some(&value["properties"]["value"]);
        }
        match value {
            serde_json::Value::Object(fields) => fields.values().find_map(find_conversion),
            serde_json::Value::Array(values) => values.iter().find_map(find_conversion),
            _ => None,
        }
    }
    let schema =
        cadmpeg_core::serde_erase::json::to_value(schemars::schema_for!(FeatureOperation)).unwrap();
    let conversion = find_conversion(&schema).unwrap();
    let conversion = match conversion.get("$ref").and_then(serde_json::Value::as_str) {
        Some(reference) => schema
            .pointer(reference.strip_prefix('#').unwrap())
            .unwrap(),
        None => conversion,
    };
    assert_eq!(conversion["properties"]["sources"]["type"], "array");
    assert!(conversion["required"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("sources")));
    assert!(conversion["properties"]["method"].is_object());
    assert_eq!(conversion["additionalProperties"], false);
}

#[test]
fn plane_cut_round_trips_independent_components_and_rewrites_sources() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    for cut in [
        serde_json::json!({}),
        serde_json::json!({"origin":{"x":2.0,"y":3.0,"z":5.0}}),
        serde_json::json!({"normal":{"x":0.0,"y":0.0,"z":2.0},"mode":"split_faces"}),
        serde_json::json!({"mode":"trim","fill":"no_fill","retained_side":"negative"}),
        serde_json::json!({"mode":"split_bodies","fill":"uniform","retained_side":"positive"}),
    ] {
        for source in [
            serde_json::json!({"kind":"unresolved","value":{"feature":"test:model:feature#cut"}}),
            serde_json::json!({"kind":"tessellations","value":["test:mesh:tessellation#source"]}),
            serde_json::json!({"kind":"native","value":{"native":"scope","members":["9","7"]}}),
        ] {
            let wire = serde_json::json!({"kind": "mesh_modify", "value": {"source":source,"modification":{"kind":"plane_cut","cut":cut}}});
            let operation: FeatureOperation =
                cadmpeg_core::serde_erase::json::from_value(wire.clone()).unwrap();
            assert_eq!(
                cadmpeg_core::serde_erase::json::to_value(&operation).unwrap(),
                wire
            );
            crate::test_support::with_service_decode_context(|ctx| {
                let mut identities =
                    IdentityMap::new(|source: &str| Ok(source.replace("test:", "test:child/")));
                let cloned = operation.rewrite_identities(ctx, &mut identities)?;
                let cloned = cadmpeg_core::serde_erase::json::to_value(cloned).unwrap();
                assert_eq!(cloned["value"]["modification"], wire["value"]["modification"]);
                assert_eq!(
                    cloned["value"]["source"],
                    if source["value"].get("feature").is_some() {
                        serde_json::json!({"kind":"unresolved","value":{"feature":"test:child/model:feature#cut"}})
                    } else if source["value"].is_array() {
                        serde_json::json!({"kind":"tessellations","value":["test:child/mesh:tessellation#source"]})
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
        cadmpeg_core::serde_erase::json::to_value(super::MeshPlaneCut::default()).unwrap(),
        serde_json::json!({})
    );
}

#[test]
fn plane_cut_rejects_null_unknown_keys_and_invalid_checked_geometry() {
    for key in ["origin", "normal", "mode", "fill", "retained_side"] {
        let mut wire = serde_json::json!({});
        wire[key] = serde_json::Value::Null;
        let error =
            cadmpeg_core::serde_erase::json::from_value::<super::MeshPlaneCut>(wire).unwrap_err();
        assert!(error.to_string().contains(key));
    }
    for wire in [
        serde_json::json!({"normal":{"x":0,"y":0,"z":0}}),
        serde_json::json!({"normal":{"x":1e308,"y":0,"z":0}}),
        serde_json::json!({"mode":"other"}),
        serde_json::json!({"fill":"other"}),
        serde_json::json!({"retained_side":"other"}),
        serde_json::json!({"u_axis":{"x":1,"y":0,"z":0}}),
    ] {
        assert!(cadmpeg_core::serde_erase::json::from_value::<super::MeshPlaneCut>(wire).is_err());
    }
}

#[cfg(feature = "schema")]
#[test]
fn plane_cut_schema_matches_omitted_optional_values_and_control_vocabulary() {
    let schema =
        cadmpeg_core::serde_erase::json::to_value(schemars::schema_for!(super::MeshPlaneCut))
            .unwrap();
    for key in ["origin", "normal", "mode", "fill", "retained_side"] {
        assert!(schema["properties"][key].is_object());
        assert!(!schema["properties"][key].to_string().contains("null"));
        assert!(!schema["required"]
            .as_array()
            .is_some_and(|required| required.contains(&serde_json::json!(key))));
    }
    assert_eq!(schema["additionalProperties"], false);
    for value in [
        "trim",
        "split_bodies",
        "split_faces",
        "no_fill",
        "minimal",
        "uniform",
        "positive",
        "negative",
    ] {
        assert!(schema.to_string().contains(value));
    }
}
