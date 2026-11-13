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
            serde_json::json!({"kind":"reduce","distribution":"uniform","target":{"kind":"face_count","value":6}}),
            MeshModification::Reduce {
                distribution: Some(MeshFaceDistribution::Uniform),
                target: Some(MeshReduceTarget::FaceCount(
                    std::num::NonZeroU32::new(6).unwrap(),
                )),
            },
        ),
        (
            serde_json::json!({"kind":"reduce","target":{"kind":"face_proportion","value":0.5}}),
            MeshModification::Reduce {
                distribution: None,
                target: Some(MeshReduceTarget::FaceProportion(
                    crate::scalar::Fraction::new(0.5).unwrap(),
                )),
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
                method: Some(MeshRepair::CloseHoles),
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
        (
            serde_json::json!({"kind":"shell","thickness":2.0}),
            MeshModification::Shell {
                thickness: Some(crate::scalar::PositiveLength::new(2.0).unwrap()),
            },
        ),
        (
            serde_json::json!({"kind":"reverse_normals"}),
            MeshModification::ReverseNormals,
        ),
    ] {
        let decoded: MeshModification = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, expected);
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    for invalid in [
        serde_json::json!({"kind":"smooth","smoothness":1.5}),
        serde_json::json!({"kind":"smooth","smoothness":null}),
        serde_json::json!({"kind":"shell","thickness":0.0}),
        serde_json::json!({"kind":"reduce","target":{"kind":"face_count","value":0}}),
    ] {
        assert!(serde_json::from_value::<MeshModification>(invalid).is_err());
    }
}

#[test]
fn mesh_operations_keep_their_feature_wire_forms() {
    for wire in [
        serde_json::json!({
            "definition":"mesh_modify",
            "source":{"native":"mesh:scope","members":["9"]},
            "modification":{"kind":"reverse_normals"}
        }),
        serde_json::json!({
            "definition":"mesh_section",
            "source":["test:mesh:tessellation#first"],
            "sketch":"test:model:feature#sketch"
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
