use super::{blend_support_bipartition, native_feature_kind, unique_simple_hole_template};
use cadmpeg_core::decode::{ResourceDimension, ResourceFailure};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::NativeFeatureKind;
use cadmpeg_ir::ids::SurfaceId;

#[test]
fn native_feature_kind_releases_canonical_tag_storage() {
    for (text, expected) in [
        ("Canvas", NativeFeatureKind::Canvas),
        ("Decal", NativeFeatureKind::Decal),
        ("Draft", NativeFeatureKind::Draft),
        ("Fillet", NativeFeatureKind::Fillet),
        ("Chamfer", NativeFeatureKind::Chamfer),
        ("Extrude", NativeFeatureKind::Extrude),
        ("DeleteFace", NativeFeatureKind::DeleteFace),
        ("SurfaceDeleteFace", NativeFeatureKind::SurfaceDeleteFace),
    ] {
        let bytes = cadmpeg_core::decode::u64_from_index(text.len());
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_materialized_bytes = bytes;
            },
            |ctx| {
                let kind = native_feature_kind(ctx, text).unwrap();
                assert_eq!(kind, expected);
                assert_eq!(
                    serde_json::to_string(&kind).unwrap(),
                    serde_json::to_string(text).unwrap()
                );
                let storage = ctx
                    .reserve_scoped(bytes, "canonical tag storage released")
                    .unwrap();
                drop(storage);
            },
        );
    }
}

#[test]
fn native_feature_kind_retains_unknown_tag_bytes_once() {
    let text = "CUSTOM μ";
    let bytes = cadmpeg_core::decode::u64_from_index(text.len());
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = bytes,
        |ctx| {
            let kind = native_feature_kind(ctx, text).unwrap();
            assert_eq!(kind, NativeFeatureKind::Other(text.to_owned()));
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                serde_json::to_string(text).unwrap()
            );
            let CodecError::ResourceLimit(limit) =
                ctx.charge_retained(1, "retained tag probe").unwrap_err()
            else {
                panic!("the retained tag occupies the byte limit");
            };
            // Retained bytes count the unknown tag's UTF-8 payload.
            assert_eq!(limit.used, bytes);
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        },
    );
}

fn native_feature_kind_refusal(dimension: ResourceDimension) {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("tag copies use work, scoped bytes and retained bytes"),
        },
        |ctx| {
            let CodecError::ResourceLimit(limit) = native_feature_kind(ctx, "BLEND").unwrap_err()
            else {
                panic!("tag copies must propagate resource refusals");
            };
            assert_eq!(limit.operation, "NX native feature kind");
            assert_eq!(limit.dimension, dimension);
            assert_eq!(limit.reason, ResourceFailure::BudgetExceeded);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        },
    );
}

#[test]
fn native_feature_kind_copy_refuses_work() {
    native_feature_kind_refusal(ResourceDimension::WorkUnits);
}

#[test]
fn native_feature_kind_copy_refuses_scoped_bytes() {
    native_feature_kind_refusal(ResourceDimension::MaterializedBytes);
}

#[test]
fn native_feature_kind_copy_refuses_retained_bytes() {
    native_feature_kind_refusal(ResourceDimension::RetainedBytes);
}

#[test]
fn hole_template_payload_search_stops_at_second_candidate() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 2,
        |ctx| {
            let payloads = ["Hole_first", "Hole_second", "unvisited"];
            assert!(unique_simple_hole_template(ctx, &payloads)
                .unwrap()
                .is_none());
        },
    );
}

#[test]
fn hole_template_literal_bound_preserves_valid_grammar() {
    crate::test_support::with_decode_context(|ctx| {
        let valid = "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer";
        assert_eq!(
            unique_simple_hole_template(ctx, &[valid]).unwrap(),
            crate::native::features::holes::parse_simple_hole_template(valid),
        );
        let long = format!("{valid}_{}", "x".repeat(4096));
        assert!(unique_simple_hole_template(ctx, &[&long])
            .unwrap()
            .is_none());
    });
}

#[test]
fn blend_graph_refusals_reach_keyed_operations() {
    let first = SurfaceId::mint("test:model:entity#first").unwrap();
    let second = SurfaceId::mint("test:model:entity#second").unwrap();
    let pairs = [[first, second]];
    for operation in [
        "NX blend support graph nodes",
        "NX blend support graph edges",
        "NX blend queued support side",
        "NX blend queued support neighbors",
        "NX blend complete graph edge",
    ] {
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            operation,
            |ctx| blend_support_bipartition(ctx, &pairs).map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn blend_graph_scratch_does_not_consume_retained_bytes() {
    let pairs = [[
        SurfaceId::mint("test:model:entity#first").unwrap(),
        SurfaceId::mint("test:model:entity#second").unwrap(),
    ]];
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let sides = blend_support_bipartition(ctx, &pairs).unwrap().unwrap();
            assert_eq!(sides.first, vec![pairs[0][0].clone()]);
            assert_eq!(sides.second, vec![pairs[0][1].clone()]);
            drop(sides);
        },
    );
}

#[test]
fn typed_definition_skips_native_parameters_and_hole_payloads() {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            let called = std::cell::Cell::new(false);
            let definition = super::non_boolean_feature_definition_with_parameters(
                ctx,
                "BLOCK",
                &["Hole_unvisited"],
                Some([1.0, 2.0, 3.0]),
                None,
                super::HoleProjection::default(),
                || {
                    called.set(true);
                    Ok(std::collections::BTreeMap::new())
                },
            )
            .unwrap();
            assert!(matches!(
                definition,
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Block {
                        dimensions: Some(_),
                        ..
                    }
                )
            ));
            assert!(!called.get());
        },
    );
}

#[test]
fn discarded_native_parameter_selection_copies_no_text() {
    let expression = |id: &str, name: &str| crate::native::om::ParameterFormula {
        id: id.into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(name.into()),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: "12.5".into(),
        value: None,
        source_entry: "entry".into(),
        source_table: cadmpeg_core::nonblank_literal!("nx:test:expression-table#table"),
        source_offset: 0,
    };
    let parameter_use = |expression: &str| crate::native::features::FeatureParameterUse {
        id: "use".into(),
        operation_label: "operation".into(),
        expression: expression.into(),
        bindings: Vec::new(),
    };
    let expressions = [
        expression("expression-a", "same"),
        expression("expression-b", "same"),
    ];
    let first = parameter_use("expression-a");
    let duplicate = parameter_use("expression-b");
    let missing = parameter_use("missing");
    for uses in [[&first, &duplicate], [&first, &missing]] {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_retained_bytes = 0,
            |ctx| {
                let (parameters, _nodes) =
                    super::native_feature_parameters(ctx, &uses, &expressions).unwrap();
                assert!(parameters.is_empty());
            },
        );
    }
}

#[test]
fn loop_edge_sets_preserve_empty_and_duplicate_membership() {
    use cadmpeg_ir::ids::{CoedgeId, EdgeId, LoopId};
    use cadmpeg_ir::topology::{Coedge, Sense};
    let first = LoopId::mint("test:model:entity#first-loop").unwrap();
    let second = LoopId::mint("test:model:entity#second-loop").unwrap();
    let shared = EdgeId::mint("test:model:entity#shared-edge").unwrap();
    let other = EdgeId::mint("test:model:entity#other-edge").unwrap();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let add = |ir: &mut cadmpeg_ir::document::CadIr, owner: &LoopId, edge: &EdgeId| {
        let id = CoedgeId::mint(format!(
            "test:model:entity#coedge-{}",
            ir.model.coedges.len()
        ))
        .unwrap();
        ir.model.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: owner.clone(),
            edge: edge.clone(),
            radial_next: id,
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    };
    crate::test_support::with_decode_context(|ctx| {
        assert!(super::same_loop_edges(ctx, &ir, &first, &second).unwrap());
        add(&mut ir, &first, &shared);
        assert!(!super::same_loop_edges(ctx, &ir, &first, &second).unwrap());
        add(&mut ir, &second, &shared);
        add(&mut ir, &first, &shared);
        assert!(super::same_loop_edges(ctx, &ir, &first, &second).unwrap());
        add(&mut ir, &second, &other);
        assert!(!super::same_loop_edges(ctx, &ir, &first, &second).unwrap());
    });
}
