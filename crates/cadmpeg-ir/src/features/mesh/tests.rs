use super::MeshSelection;

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
        values.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
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
    let target_scope = "f3d:child/Design/BulkStream.dat:design-parameter-scope#7";
    let replacements =
        std::collections::BTreeMap::from([(source_scope.to_owned(), target_scope.to_owned())]);
    crate::test_support::with_service_decode_context(|ctx| {
        let text = TextReplacements::build(replacements.iter())?;
        let mut map = IdentityMap::new(|source: &str| Ok(format!("scoped:{source}")))
            .with_text_replacements(&text);
        let resolved: MeshSelection = serde_json::from_value(serde_json::json!([
            "test:mesh:tessellation#first",
            "test:mesh:tessellation#second"
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
                "test:mesh:tessellation#first",
                "test:mesh:tessellation#second"
            ]
        );
        let resolved = resolved.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            serde_json::to_value(resolved).unwrap(),
            serde_json::json!([
                "scoped:test:mesh:tessellation#first",
                "scoped:test:mesh:tessellation#second"
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
            "feature":"test:model:feature#import"
        }))
        .unwrap();
        let mut visited = Vec::new();
        unresolved.visit_identity_references(ctx, &mut |id| {
            visited.push(id.to_owned());
            Ok(())
        })?;
        assert_eq!(visited, ["test:model:feature#import"]);
        let unresolved = unresolved.rewrite_identities(ctx, &mut map)?;
        assert_eq!(
            serde_json::to_value(unresolved).unwrap(),
            serde_json::json!({"feature":"scoped:test:model:feature#import"})
        );
        map.finish()
    })
    .unwrap();
}
