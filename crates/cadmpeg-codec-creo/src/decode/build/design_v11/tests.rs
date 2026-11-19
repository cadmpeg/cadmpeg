// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{DistinctMembers, FeatureContent, FeatureEvaluation, FeatureId};
use cadmpeg_ir::report::check::Check;
use cadmpeg_test_support::EditableDecodeResult;

use super::*;
use crate::loss::CreoLossCode;

fn feature(key: &str, ordinal: u64, operation: LegacyOperation) -> LegacyFeature {
    LegacyFeature {
        product_definition: None,
        id: FeatureId::mint(format!("creo:model:feature#{key}")).expect("feature id"),
        ordinal,
        name: Some(format!("Feature {key}")),
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(LegacyDefinition::Operation(operation)),
        native_ref: Some(format!("creo:featdefs:feature_definition#{key}")),
    }
}

fn unresolved(family: UnresolvedFamily) -> LegacyOperation {
    LegacyOperation::Unresolved { family }
}

fn body(key: &str) -> cadmpeg_ir::topology::Body {
    serde_json::from_value(serde_json::json!({
        "id": format!("creo:visibgeom:body#{key}"),
        "kind": "solid",
    }))
    .expect("body")
}

fn part(features: Vec<LegacyFeature>, bodies: &[&str]) -> (CadIr, ProductDefinitionId) {
    let mut ir = CadIr::empty();
    ir.model.features = features;
    ir.geometry.bodies = bodies.iter().map(|key| body(key)).collect();
    let definition = crate::decode::with_service_context(|ctx| {
        root_definition(ctx, &mut ir, ProductDefinitionKind::Part)
    })
    .expect("root definition");
    (ir, definition)
}

fn timeline(ir: &CadIr) -> (&[Member], &Rollback) {
    let root = &ir.design.scopes[0].roots[0];
    match ir.design.containers.iter().find(|c| c.id() == root) {
        Some(Container::Ordered {
            members,
            rollback,
            kind: OrderedKind::Timeline,
            ..
        }) => (members, rollback),
        _ => panic!("the scope root is no timeline"),
    }
}

#[test]
fn timeline_follows_regeneration_order_and_leaves_the_model_unchanged() {
    let extrude = feature("40", 1, unresolved(UnresolvedFamily::Extrude));
    let plane = feature("1", 0, unresolved(UnresolvedFamily::DatumPlane));
    let (mut ir, definition) = part(vec![extrude.clone(), plane.clone()], &["1"]);
    let model = ir.model.clone();
    crate::decode::with_service_context(|ctx| {
        project_timeline(ctx, &mut ir, &definition, &BTreeSet::new())
    })
    .expect("timeline");
    let (members, rollback) = timeline(&ir);
    assert_eq!(
        members,
        [
            Member::Feature(plane.id.clone()),
            Member::Feature(extrude.id.clone())
        ]
    );
    assert_eq!(rollback, &Rollback::End);
    let scope = &ir.design.scopes[0];
    assert_eq!(scope.definition, definition);
    assert_eq!(scope.current.len(), 1);
    assert_eq!(
        scope.current[0].at,
        Position::After(Member::Feature(extrude.id.clone()))
    );
    assert_eq!(scope.current[0].bodies, [ir.geometry.bodies[0].id.clone()]);
    assert_eq!(ir.model, model);
    let projected = ir
        .design
        .features
        .iter()
        .find(|projected| projected.id == extrude.id)
        .expect("projected extrude");
    assert_eq!(projected.name, extrude.name);
    assert_eq!(projected.native_ref, extrude.native_ref);
    assert_eq!(
        projected.suppressed, None,
        "the decoder does not read Creo suppression"
    );
}

#[test]
fn an_empty_timeline_holds_its_current_state_at_its_start() {
    let (mut ir, definition) = part(Vec::new(), &[]);
    crate::decode::with_service_context(|ctx| {
        project_timeline(ctx, &mut ir, &definition, &BTreeSet::new())
    })
    .expect("timeline");
    let root = ir.design.scopes[0].roots[0].clone();
    assert_eq!(ir.design.scopes[0].current[0].at, Position::Start(root));
}

#[test]
fn a_model_with_no_feature_list_states_no_design_records() {
    let (mut ir, definition) = part(Vec::new(), &["1", "2"]);
    let scan = crate::test_support::empty_container_scan();
    crate::decode::with_service_context(|ctx| {
        project(ctx, &scan, &mut ir, &definition, &mut Vec::new())
    })
    .expect("projection");
    assert_eq!(ir.design, CadIr::empty().design);
}

fn sketch_feature(key: &str, ordinal: u64) -> LegacyFeature {
    feature(
        key,
        ordinal,
        LegacyOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                SketchId::mint(format!("creo:model:sketch#{key}")).expect("sketch id"),
            )),
        },
    )
}

fn legacy_sketch(key: &str, placement: serde_json::Value) -> cadmpeg_ir::sketches::Sketch {
    serde_json::from_value(serde_json::json!({
        "id": format!("creo:model:sketch#{key}"),
        "native_ref": format!("creo:featdefs:sketch#{key}"),
        "placement": placement,
        "profiles": [],
    }))
    .expect("v10 sketch")
}

fn design_findings(ir: &CadIr) -> Vec<String> {
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .expect("validation")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn a_sketch_whose_frame_is_unread_states_no_sketch_and_charges_a_loss() {
    let (mut ir, definition) = part(vec![sketch_feature("1", 0), sketch_feature("2", 1)], &[]);
    ir.model.sketches = vec![
        legacy_sketch(
            "1",
            serde_json::json!({
                "kind": "resolved",
                "frame": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                    "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
                },
            }),
        ),
        legacy_sketch("2", serde_json::json!({"kind": "unresolved"})),
    ];
    let model = ir.model.clone();
    let scan = crate::test_support::empty_container_scan();
    let mut losses = Vec::new();
    crate::decode::with_service_context(|ctx| {
        project(ctx, &scan, &mut ir, &definition, &mut losses)
    })
    .expect("projection");
    assert_eq!(ir.model, model);
    assert_eq!(
        ir.design
            .sketches
            .iter()
            .map(|sketch| sketch.id.as_str())
            .collect::<Vec<_>>(),
        ["creo:model:sketch#1"]
    );
    let operation = |key: &str| {
        ir.design
            .features
            .iter()
            .find(|feature| feature.id.as_str() == format!("creo:model:feature#{key}"))
            .map(|feature| feature.definition.operation.clone())
            .expect("projected feature")
    };
    assert_eq!(
        operation("1"),
        FeatureOperation::Sketch(SketchId::mint("creo:model:sketch#1").expect("sketch id"))
    );
    assert_eq!(
        operation("2"),
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        }
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == CreoLossCode::DesignSketchPlaneUnread.kind())
            .count(),
        1
    );
    assert_eq!(design_findings(&ir), Vec::<String>::new());
}

#[test]
fn the_root_definition_is_stated_once_and_owns_every_body() {
    let (mut ir, definition) = part(Vec::new(), &["1"]);
    assert_eq!(definition.as_str(), "creo:model:product_definition#root");
    assert!(ir.model.product_definitions[0].is_root());
    assert_eq!(
        ir.model.product_definitions[0].kind,
        Some(ProductDefinitionKind::Part)
    );
    assert_eq!(
        ir.model.product_definitions[0].bodies,
        [ir.geometry.bodies[0].id.clone()]
    );
    let again = crate::decode::with_service_context(|ctx| {
        root_definition(ctx, &mut ir, ProductDefinitionKind::Part)
    })
    .expect("root definition");
    assert_eq!(again, definition);
    assert_eq!(ir.model.product_definitions.len(), 1);
}

#[test]
fn operations_keep_their_family_or_their_native_kind() {
    assert_eq!(
        operation(&unresolved(UnresolvedFamily::Fillet), &BTreeSet::new()).expect("family"),
        FeatureOperation::Unresolved {
            family: OperationFamily::Fillet
        }
    );
    assert_eq!(
        operation(
            &LegacyOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Future Feature".to_owned()),
                parameters: BTreeMap::new(),
            },
            &BTreeSet::new()
        )
        .expect("native"),
        native("Future Feature").expect("native")
    );
    assert_eq!(
        operation(&LegacyOperation::StoredGeometry {}, &BTreeSet::new()).expect("stored"),
        native("stored_geometry").expect("native")
    );
}

#[test]
fn a_sketch_feature_names_its_sketch_only_where_the_sketch_is_stated() {
    let sketch = SketchId::mint("creo:model:sketch#917").expect("sketch id");
    let feature = LegacyOperation::Sketch {
        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch.clone())),
    };
    assert_eq!(
        operation(&feature, &BTreeSet::from([sketch.clone()])).expect("sketch"),
        FeatureOperation::Sketch(sketch)
    );
    assert_eq!(
        operation(&feature, &BTreeSet::new()).expect("sketch"),
        FeatureOperation::Unresolved {
            family: OperationFamily::Sketch
        }
    );
}

/// Every committed fixture with a feature list decodes to one scope over the
/// root definition, a fixture with none to no design records, and the design
/// records pass the part 10 design checks.
#[test]
fn fixture_decodes_state_a_scope_that_passes_the_design_checks() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");
    let mut paths = std::fs::read_dir(&fixtures)
        .expect("fixture directory")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "prt"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let result = crate::CreoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("fixture decode");
        let result = EditableDecodeResult::from(result);
        let ir = result.ir();
        if ir.model.features.is_empty() {
            assert_eq!(ir.design, CadIr::empty().design, "{}", path.display());
            continue;
        }
        assert_eq!(ir.design.scopes.len(), 1, "{}", path.display());
        assert_eq!(
            ir.design.scopes[0].definition.as_str(),
            "creo:model:product_definition#root"
        );
        assert_eq!(ir.design.features.len(), ir.model.features.len());
        assert_eq!(ir.design.parameters.len(), ir.model.parameters.len());
        assert_eq!(
            ir.design.sketches.len(),
            ir.model
                .sketches
                .iter()
                .filter(|sketch| sketch.placement.resolved().is_some())
                .count()
        );
        let findings = design_findings(ir);
        assert!(findings.is_empty(), "{}: {findings:?}", path.display());
    }
}
