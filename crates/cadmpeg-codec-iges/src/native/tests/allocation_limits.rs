// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::test_support::test_drawing_and_trimming::{
    associativity_definition_file, bounded_associativity_forms_file,
    flow_associativity_forms_file, legacy_associativity_forms_file,
    recalculable_dimension_associativity_file,
};
use crate::IgesCodec;

fn native_entity(entity_type: i64, form: i64, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type,
        form,
        label: "NATIVE".into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

fn assert_collection_refusal_at(bytes: &[u8], operation: &str) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let result = IgesCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        );
        match result {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                if limit.operation == operation {
                    return;
                }
                let next = limit.used.checked_add(limit.additional).unwrap();
                assert!(next > cap, "limit did not advance from {cap}: {limit:?}");
                cap = next;
            }
            Ok(_) => panic!("did not reach {operation} at collection cap {cap}: decode succeeded"),
            other => panic!("did not reach {operation} at collection cap {cap}: {other:?}"),
        }
    }
    panic!("did not reach {operation} within 4096 admission boundaries");
}

fn assert_retained_refusal_at(bytes: &[u8], operation: &str) {
    let mut cap = 0_u64;
    for _ in 0..4096 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let result = IgesCodec.decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                policy,
                ..DecodeOptions::default()
            },
        );
        match result {
            Err(DecodeFailure::Codec(CodecError::ResourceLimit(limit))) => {
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                if limit.operation == operation {
                    return;
                }
                let next = limit.used.checked_add(limit.additional).unwrap();
                assert!(next > cap, "limit did not advance from {cap}: {limit:?}");
                cap = next;
            }
            Ok(_) => panic!("did not reach {operation} at retained cap {cap}: decode succeeded"),
            other => panic!("did not reach {operation} at retained cap {cap}: {other:?}"),
        }
    }
    panic!("did not reach {operation} within 4096 admission boundaries");
}

fn assert_native_arena(bytes: &[u8], arena: &str) {
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas()[arena].len(),
        1,
    );
}

#[test]
fn native_direction_slots_and_components_refuse_collection_limits() {
    let bytes = owned_test_file(&[native_entity(123, 0, "123,1,0,0;")]);
    assert_native_arena(&bytes, "directions");
    for operation in [
        "iges native direction slots",
        "iges native direction components",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_flash_slots_refuse_collection_limit() {
    let bytes = owned_test_file(&[native_entity(125, 0, "125,0,0,1,1,0,0;")]);
    assert_native_arena(&bytes, "flashes");
    assert_collection_refusal_at(&bytes, "iges native flash slots");
}

#[test]
fn native_transform_slots_and_coefficients_refuse_collection_limits() {
    let bytes = owned_test_file(&[native_entity(124, 0, "124,1,0,0,0,0,1,0,0,0,0,1,0;")]);
    assert_native_arena(&bytes, "transformations");
    for operation in [
        "iges native transformation slots",
        "iges native transformation coefficients",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_copious_outer_tuple_and_component_slots_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(106, 1, "106,1,1,0.5,1,2;")]);
    assert_native_arena(&bytes, "copious_data");
    for operation in [
        "iges native copious data slots",
        "iges native copious tuple slots",
        "iges native copious component slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_color_and_display_slots_refuse_collection_limits() {
    let bytes = owned_test_file(&[native_entity(314, 0, "314,100,0,0,3Hred;")]);
    assert_native_arena(&bytes, "colors");
    assert_native_arena(&bytes, "display_attributes");
    for operation in [
        "iges native color slots",
        "iges native display attribute slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_line_font_outer_lengths_and_pattern_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(304, 2, "304,1,2,3HFFF;")]);
    assert_native_arena(&bytes, "line_fonts");
    for operation in [
        "iges native line font slots",
        "iges native line font lengths",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    assert_retained_refusal_at(&bytes, "iges native line font pattern");
}

#[test]
fn native_text_template_outer_slot_refuses_limit() {
    let bytes = owned_test_file(&[native_entity(312, 0, "312,1,1,1,0,0,0,0,0,0,0;")]);
    assert_native_arena(&bytes, "text_templates");
    assert_collection_refusal_at(&bytes, "iges native text template slots");
}

#[test]
fn native_text_font_outer_glyph_and_motion_slots_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(310, 0, "310,1,1HA,0,1,1,65,0,0,1,0,1,2;")]);
    assert_native_arena(&bytes, "text_fonts");
    for operation in [
        "iges native text font slots",
        "iges native text font glyph slots",
        "iges native text font motion slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    assert_retained_refusal_at(&bytes, "iges native text font name");
}

#[test]
fn native_definition_level_outer_and_value_slots_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(406, 1, "406,2,3,5;")]);
    assert_native_arena(&bytes, "definition_levels");
    for operation in [
        "iges native definition level slots",
        "iges native definition level values",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_basic_record_ids_and_payloads_refuse_retained_limits() {
    let cases: &[(i64, i64, &str, &[&str])] = &[
        (
            123,
            0,
            "123,1,0,0;",
            &["iges native direction id", "iges native direction source"],
        ),
        (
            125,
            0,
            "125,0,0,1,1,0,0;",
            &["iges native flash id", "iges native flash source"],
        ),
        (
            124,
            0,
            "124,1,0,0,0,0,1,0,0,0,0,1,0;",
            &[
                "iges native transformation id",
                "iges native transformation source",
            ],
        ),
        (
            106,
            1,
            "106,1,1,0.5,1,2;",
            &[
                "iges native copious data id",
                "iges native copious data source",
            ],
        ),
        (
            314,
            0,
            "314,100,0,0,3Hred;",
            &[
                "iges native color id",
                "iges native color source",
                "iges native color name",
            ],
        ),
        (
            304,
            2,
            "304,1,2,3HFFF;",
            &[
                "iges native line font id",
                "iges native line font source",
                "iges native line font pattern",
            ],
        ),
        (
            312,
            0,
            "312,1,1,1,0,0,0,0,0,0,0;",
            &[
                "iges native text template id",
                "iges native text template source",
            ],
        ),
        (
            310,
            0,
            "310,1,1HA,0,1,1,65,0,0,1,0,1,2;",
            &[
                "iges native text font id",
                "iges native text font source",
                "iges native text font name",
            ],
        ),
        (
            406,
            1,
            "406,2,3,5;",
            &[
                "iges native definition levels id",
                "iges native definition levels source",
            ],
        ),
    ];
    for (entity_type, form, parameters, operations) in cases {
        let bytes = owned_test_file(&[native_entity(*entity_type, *form, parameters)]);
        for operation in *operations {
            assert_retained_refusal_at(&bytes, operation);
        }
    }
}

#[test]
fn native_display_definition_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};

    let bytes = crate::test_support::test_owned::owned_test_file_with_colors(
        &[
            native_entity(116, 0, "116,1,2,3,0;"),
            native_entity(314, 0, "314,100,0,0,3Hred;"),
        ],
        &[(1, -3)],
    );
    let scan = crate::card::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let (parse_ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let (global, _) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, _) =
        crate::directory::parse(&scan, global.global_table(), Some(&parse_ctx)).unwrap();
    let graph = crate::graph::build(&directory, &parse_ctx).unwrap();

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        super::super::resolve_display_ref(
            &ctx, &graph, 1, -3, crate::graph::ReferenceKind::Color, "color"
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "iges native display definition"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let reference = super::super::resolve_display_ref(
        &ctx,
        &graph,
        1,
        -3,
        crate::graph::ReferenceKind::Color,
        "color",
    )
    .unwrap();
    assert_eq!(reference.definition(), Some("iges:presentation:color#D3"));
}

#[test]
fn native_primitive_dimension_nodes_and_names_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(158, 0, "158,2,0,0,0;")]);
    assert_native_arena(&bytes, "primitive_solids");
    for operation in [
        "iges native primitive solid slots",
        "iges native primitive dimension node",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native primitive dimension name",
        "iges native primitive solid id",
        "iges native primitive solid source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_procedural_solid_slots_and_ids_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(164, 0, "164,0,1,0,0,1;")]);
    assert_native_arena(&bytes, "procedural_solids");
    assert_collection_refusal_at(&bytes, "iges native procedural solid slots");
    for operation in [
        "iges native procedural solid id",
        "iges native procedural solid source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_boolean_tree_slots_and_terms_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(180, 0, "180,1,1;")]);
    assert_native_arena(&bytes, "boolean_trees");
    for operation in [
        "iges native boolean tree slots",
        "iges native boolean term slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native boolean tree id",
        "iges native boolean tree source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_selected_component_slot_and_ids_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(182, 0, "182,0,0,0,0;")]);
    assert_native_arena(&bytes, "selected_components");
    assert_collection_refusal_at(&bytes, "iges native selected component slots");
    for operation in [
        "iges native selected component id",
        "iges native selected component source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_solid_assembly_outer_and_item_slots_refuse_limits() {
    let bytes = owned_test_file(&[
        native_entity(158, 0, "158,2,0,0,0;"),
        native_entity(184, 0, "184,1,1,0;"),
    ]);
    assert_native_arena(&bytes, "solid_assemblies");
    for operation in [
        "iges native solid assembly slots",
        "iges native solid assembly item slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native solid assembly id",
        "iges native solid assembly source",
        "iges native solid assembly member",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_manifold_outer_and_void_slots_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(186, 0, "186,0,1,1,0,1;")]);
    assert_native_arena(&bytes, "manifold_solids");
    for operation in [
        "iges native manifold solid slots",
        "iges native manifold void slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native manifold solid id",
        "iges native manifold solid source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_solid_instance_slot_and_ids_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(430, 0, "430,0;")]);
    assert_native_arena(&bytes, "solid_instances");
    assert_collection_refusal_at(&bytes, "iges native solid instance slots");
    for operation in [
        "iges native solid instance id",
        "iges native solid instance source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_subfigure_definition_members_and_instance_slots_refuse_limits() {
    let bytes = owned_test_file(&[
        native_entity(116, 0, "116,1,2,3,0;"),
        native_entity(308, 0, "308,0,3HDEF,1,1;"),
        native_entity(408, 0, "408,3,0,0,0,1;"),
    ]);
    assert_native_arena(&bytes, "subfigure_definitions");
    assert_native_arena(&bytes, "subfigure_instances");
    for operation in [
        "iges native subfigure definition slots",
        "iges native subfigure member slots",
        "iges native subfigure instance slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native subfigure definition id",
        "iges native subfigure definition source",
        "iges native subfigure name",
        "iges native subfigure member",
        "iges native subfigure instance id",
        "iges native subfigure instance source",
        "iges native subfigure instance definition",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_network_definition_member_and_connect_point_slots_refuse_limits() {
    let bytes = owned_test_file(&[
        native_entity(116, 0, "116,1,2,3,0;"),
        native_entity(132, 0, "132,0,0,0,0,0,0,0,0,0,0,0,0,0,0;"),
        native_entity(320, 0, "320,0,3HNET,1,1,0,2HR1,0,1,3;"),
    ]);
    assert_native_arena(&bytes, "network_definitions");
    for operation in [
        "iges native network definition slots",
        "iges native network member slots",
        "iges native network connect point slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native network definition id",
        "iges native network definition source",
        "iges native network name",
        "iges native network member",
        "iges native network designator",
        "iges native network connect point",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_network_instance_connect_point_slots_and_designator_refuse_limits() {
    let bytes = owned_test_file(&[
        native_entity(320, 0, "320,0,3HNET,0,0,2HR1,0,0;"),
        native_entity(420, 0, "420,1,0,0,0,1,1,1,0,2HR1,0,1,0;"),
    ]);
    assert_native_arena(&bytes, "network_instances");
    for operation in [
        "iges native network instance slots",
        "iges native network instance connect point slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native network instance id",
        "iges native network instance source",
        "iges native network instance definition",
        "iges native network instance designator",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_connect_point_slot_and_function_bytes_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(
        132,
        0,
        "132,0,0,0,0,0,0,2HID,0,4HNAME,0,1,0,0,0;",
    )]);
    assert_native_arena(&bytes, "connect_points");
    assert_collection_refusal_at(&bytes, "iges native connect point slots");
    for operation in [
        "iges native connect point id",
        "iges native connect point source",
        "iges native connect function identifier",
        "iges native connect function name",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_rectangular_array_outer_and_position_slots_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(412, 0, "412,0,1,0,0,0,1,1,1,1,0,1,0,1;")]);
    assert_native_arena(&bytes, "rectangular_arrays");
    for operation in [
        "iges native rectangular array slots",
        "iges native rectangular position slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native rectangular array id",
        "iges native rectangular array source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_circular_array_outer_and_position_slots_refuse_limits() {
    let bytes = owned_test_file(&[native_entity(414, 0, "414,0,1,0,0,0,1,0,1,1,0,1;")]);
    assert_native_arena(&bytes, "circular_arrays");
    for operation in [
        "iges native circular array slots",
        "iges native circular position slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native circular array id",
        "iges native circular array source",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_external_reference_slots_and_copied_names_refuse_limits() {
    for (form, parameters, payload_operation) in [
        (
            0,
            "416,4Hfile,3Hsym;",
            "iges native external file identifier",
        ),
        (4, "416,3Hlib,3Hsym;", "iges native external library name"),
    ] {
        let bytes = owned_test_file(&[native_entity(416, form, parameters)]);
        assert_native_arena(&bytes, "external_references");
        assert_collection_refusal_at(&bytes, "iges native external reference slots");
        for operation in [
            "iges native external reference id",
            "iges native external reference source",
            "iges native external symbolic name",
            payload_operation,
        ] {
            assert_retained_refusal_at(&bytes, operation);
        }
    }
}

#[test]
fn native_group_outer_and_member_slots_refuse_limits() {
    let bytes = owned_test_file(&[
        native_entity(116, 0, "116,1,2,3,0;"),
        native_entity(402, 1, "402,1,1;"),
    ]);
    assert_native_arena(&bytes, "groups");
    for operation in ["iges native group slots", "iges native group member slots"] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native group id",
        "iges native group source",
        "iges native group member",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_associativity_definition_classes_and_item_types_refuse_limits() {
    let bytes = associativity_definition_file();
    for operation in [
        "iges native associativity definition slots",
        "iges native associativity classes",
        "iges native associativity class item types",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    assert_retained_refusal_at(&bytes, "iges native associativity definition id");
}

#[test]
fn native_bounded_associativity_nested_lists_refuse_limits() {
    let bytes = bounded_associativity_forms_file();
    for operation in [
        "iges native associativities",
        "iges native label placement slots",
        "iges native single-parent child slots",
        "iges native external index entries",
        "iges native dimension geometry slots",
        "iges native planar entity slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native associativity id",
        "iges native associativity source",
        "iges native label placement view",
        "iges native label placement leader",
        "iges native external index name",
        "iges native associativity link",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_legacy_associativity_nested_values_refuse_limits() {
    let bytes = legacy_associativity_forms_file();
    for operation in [
        "iges native signal connection slots",
        "iges native signal geometry slots",
        "iges native signal name slots",
        "iges native text node geometry slots",
        "iges native connect node point slots",
        "iges native connect node data slots",
    ] {
        assert_collection_refusal_at(&bytes, operation);
    }
    for operation in [
        "iges native signal name",
        "iges native signal connection",
        "iges native signal geometry",
        "iges native text node geometry",
        "iges native connect node point",
        "iges native token value bytes",
    ] {
        assert_retained_refusal_at(&bytes, operation);
    }
}

#[test]
fn native_flow_and_recalculable_dimension_lists_refuse_limits() {
    let flow = flow_associativity_forms_file();
    let linked_flow = owned_test_file(&[native_entity(402, 18, "402,2,1,0,0,0,0,0,1,2,1;")]);
    assert_collection_refusal_at(&linked_flow, "iges native flow link slots");
    for operation in [
        "iges native flow connection slots",
        "iges native flow join slots",
        "iges native flow name slots",
        "iges native flow name display slots",
        "iges native flow continuation slots",
    ] {
        assert_collection_refusal_at(&flow, operation);
    }
    assert_retained_refusal_at(&flow, "iges native flow name");
    let dimension = recalculable_dimension_associativity_file();
    assert_collection_refusal_at(&dimension, "iges native recalculable geometry slots");
    assert_retained_refusal_at(&dimension, "iges native recalculable dimension");
}
