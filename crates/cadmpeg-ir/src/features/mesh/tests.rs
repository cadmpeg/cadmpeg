use super::{
    MeshFaceDistribution, MeshModification, MeshRebuildMode, MeshReduceTarget, MeshRepair,
    MeshSelection,
};
use crate::features::FeatureOperation;

#[test]
fn resolved_mesh_selection_preserves_the_tessellation_array_wire_form() {
    let wire = serde_json::json!([
        "test:mesh:tessellation#first",
        "test:mesh:tessellation#second"
    ]);
    let selection: MeshSelection = serde_json::from_value(wire.clone()).unwrap();
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
    assert_eq!(serde_json::to_value(selection).unwrap(), wire);
}

#[test]
fn native_mesh_selection_preserves_source_order() {
    let wire = serde_json::json!({"native":"mesh:source", "members":["7", "3"]});
    let selection: MeshSelection = serde_json::from_value(wire.clone()).unwrap();
    let MeshSelection::Native { native, members } = &selection else {
        panic!("native sources");
    };
    assert_eq!(native.as_str(), "mesh:source");
    assert_eq!(members.as_ref().unwrap().as_slice(), ["7", "3"]);
    assert_eq!(serde_json::to_value(selection).unwrap(), wire);
}

#[test]
fn mesh_selections_reject_empty_repeated_blank_and_mixed_members() {
    for wire in [
        serde_json::json!([]),
        serde_json::json!([
            "test:mesh:tessellation#first",
            "test:mesh:tessellation#first"
        ]),
        serde_json::json!(["mesh id"]),
        serde_json::json!({"native":" "}),
        serde_json::json!({"native":"mesh:source", "members":[]}),
        serde_json::json!({"native":"mesh:source", "members":null}),
        serde_json::json!({"native":"mesh:source", "members":[" "]}),
        serde_json::json!({"native":"mesh:source", "members":["7", "7"]}),
        serde_json::json!({"native":"mesh:source", "tessellations":["mesh:second"]}),
        serde_json::json!({"feature":" "}),
        serde_json::json!({"feature":"test:model:feature#import", "native":"mesh:source"}),
    ] {
        assert!(serde_json::from_value::<MeshSelection>(wire).is_err());
    }
}

#[test]
fn mesh_sources_rewrite_resolved_identities_and_declared_native_scopes() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

    let source_scope = "f3d:Design/BulkStream.dat:design-parameter-scope#7";
    let target_scope = "f3d:xref/child/Design/BulkStream.dat:design-parameter-scope#7";
    let replacements =
        std::collections::BTreeMap::from([(source_scope.to_owned(), target_scope.to_owned())]);
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|source: &str| {
            let source = source.strip_prefix("f3d:").unwrap();
            Ok(format!("f3d:xref/child/{source}"))
        })
        .with_text_replacements(&text);
        let resolved: MeshSelection = serde_json::from_value(serde_json::json!([
            "f3d:mesh:tessellation#first",
            "f3d:mesh:tessellation#second"
        ]))
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
            serde_json::to_value(resolved).unwrap(),
            serde_json::json!([
                "f3d:xref/child/mesh:tessellation#first",
                "f3d:xref/child/mesh:tessellation#second"
            ])
        );
        let native: MeshSelection = serde_json::from_value(serde_json::json!({
            "native":source_scope, "members":["7", "3"]
        }))
        .unwrap();
        let native = native.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            serde_json::to_value(native).unwrap(),
            serde_json::json!({
                "native":target_scope, "members":["7", "3"]
            })
        );
        let unresolved: MeshSelection = serde_json::from_value(serde_json::json!({
            "feature":"f3d:model:feature#import"
        }))
        .unwrap();
        let mut visited = Vec::new();
        unresolved.visit_identity_references(ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })?;
        assert_eq!(visited, ["f3d:model:feature#import"]);
        let unresolved = unresolved.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            serde_json::to_value(unresolved).unwrap(),
            serde_json::json!({"feature":"f3d:xref/child/model:feature#import"})
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
        let decoded: MeshModification = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    for invalid in [
        serde_json::json!({"kind":"smooth","smoothness":1.5}),
        serde_json::json!({"kind":"smooth","smoothness":null}),
        serde_json::json!({"kind":"shell","thickness":2.0}),
        serde_json::json!({"kind":"reverse_normals"}),
        serde_json::json!({"kind":"reduce","target":{"kind":"face_count","value":0}}),
    ] {
        assert!(serde_json::from_value::<MeshModification>(invalid).is_err());
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
        assert!(serde_json::from_value::<MeshReduceTarget>(target).is_err());
    }
    for method in [
        serde_json::json!({"kind":"close_holes","extra":true}),
        serde_json::json!({"kind":"stitch_and_remove","extra":true}),
    ] {
        assert!(serde_json::from_value::<MeshRepair>(method).is_err());
    }
}

#[test]
fn mesh_operations_keep_their_feature_wire_forms() {
    for wire in [
        serde_json::json!({
            "definition":"mesh_modify",
            "source":{"native":"mesh:scope","members":["9"]},
            "modification":{"kind":"smooth","smoothness":0.25}
        }),
        serde_json::json!({
            "definition":"mesh_section",
            "source":["test:mesh:tessellation#first"],
            "sketch":"test:model:feature#sketch",
            "plane":{"origin":{"x":1.0,"y":2.0,"z":3.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}
        }),
        serde_json::json!({
            "definition":"mesh_section",
            "source":{"feature":"test:model:feature#section"}
        }),
    ] {
        let operation: FeatureOperation = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(
            operation,
            FeatureOperation::MeshModify { .. } | FeatureOperation::MeshSection { .. }
        ));
        assert_eq!(serde_json::to_value(operation).unwrap(), wire);
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
        let wire = serde_json::json!({
            "definition":"mesh_section",
            "source":{"feature":"test:model:feature#section"},
            "plane":plane
        });
        assert!(serde_json::from_value::<FeatureOperation>(wire).is_err());
    }
}

#[test]
fn section_identity_rewrite_keeps_its_plane_and_linked_sketch() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

    let wire = serde_json::json!({
        "definition":"mesh_section",
        "source":{"feature":"test:model:feature#section"},
        "plane":{
            "origin":{"x":1.0,"y":2.0,"z":3.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":1.0,"y":0.0,"z":0.0}
        },
        "sketch":"test:model:feature#sketch"
    });
    let operation: FeatureOperation = serde_json::from_value(wire.clone()).unwrap();
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
        expected["source"]["feature"] = "test:child/model:feature#section".into();
        expected["sketch"] = "test:child/model:feature#sketch".into();
        assert_eq!(serde_json::to_value(rewritten).unwrap(), expected);
        map.finish()
    })
    .unwrap();
}

#[test]
fn conversion_wire_preserves_ordered_source_contexts() {
    let wire = serde_json::json!({
        "definition":"mesh_convert",
        "sources":[
            {"native":"mesh:scope","members":["9", "3"]},
            {"native":"mesh:scope","members":["9", "3"]},
            {"feature":"test:model:feature#convert"}
        ]
    });
    let operation: FeatureOperation = serde_json::from_value(wire.clone()).unwrap();
    let FeatureOperation::MeshConvert { sources, .. } = &operation else {
        panic!("mesh conversion");
    };
    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0], sources[1]);
    assert_eq!(serde_json::to_value(operation).unwrap(), wire);
    for method in ["faceted", "prismatic"] {
        let mut known = wire.clone();
        known["method"] = method.into();
        let operation: FeatureOperation = serde_json::from_value(known.clone()).unwrap();
        assert_eq!(serde_json::to_value(operation).unwrap(), known);
    }
    for method in [serde_json::Value::Null, serde_json::json!("unruled")] {
        let mut invalid = wire.clone();
        invalid["method"] = method;
        assert!(serde_json::from_value::<FeatureOperation>(invalid).is_err());
    }
    for sources in [
        serde_json::Value::Null,
        serde_json::json!({"native":"mesh:scope"}),
        serde_json::json!([{"native":"mesh:scope","members":[]}]),
        serde_json::json!([{"feature":" "}]),
    ] {
        assert!(
            serde_json::from_value::<FeatureOperation>(serde_json::json!({
                "definition":"mesh_convert", "sources":sources
            }))
            .is_err()
        );
    }
}

#[test]
fn conversion_rewrites_declared_native_and_typed_source_identities() {
    use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

    let source_scope = "f3d:Design/BulkStream.dat:design-parameter-scope#7";
    let target_scope = "f3d:xref/child/Design/BulkStream.dat:design-parameter-scope#7";
    let replacements =
        std::collections::BTreeMap::from([(source_scope.to_owned(), target_scope.to_owned())]);
    let wire = serde_json::json!({
        "definition":"mesh_convert",
        "sources":[
            {"native":source_scope,"members":["9", "3"]},
            ["f3d:mesh:tessellation#source"],
            {"feature":"f3d:model:feature#convert"}
        ]
    });
    let operation: FeatureOperation = serde_json::from_value(wire.clone()).unwrap();
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|id: &str| {
            Ok(format!(
                "f3d:xref/child/{}",
                id.strip_prefix("f3d:").unwrap()
            ))
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
        expected["sources"][0]["native"] = target_scope.into();
        expected["sources"][1][0] = "f3d:xref/child/mesh:tessellation#source".into();
        expected["sources"][2]["feature"] = "f3d:xref/child/model:feature#convert".into();
        assert_eq!(serde_json::to_value(rewritten).unwrap(), expected);
        map.finish()
    })
    .unwrap();
}
