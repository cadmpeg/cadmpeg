// SPDX-License-Identifier: Apache-2.0

use super::super::{
    NativeAttributeDefinition, NativeAttributeTableDefinition, NativeAttributeValue, NativeCard,
    NativeDisplayAttributes, NativeEntity, NativeGlyph, NativeIndependentVariable, NativeProperty,
    NativeStoreInputs, NativeTextFontDefinition, ProductOccurrenceLimits, QuarantinedRecords,
};
use crate::directory::DirectoryEntry;
use crate::parameter::{Token, TokenValue};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;
use std::mem::{align_of, size_of};

const EMPTY_ROOT_MATERIALIZED_ALLOWANCE: u64 = 16 * 1024 * 1024;

fn tree_node<K, V>() -> usize {
    11 * (size_of::<K>() + size_of::<V>()) + 16 * size_of::<usize>()
        + 2 * align_of::<K>().max(align_of::<V>()).max(align_of::<usize>())
}

#[derive(Clone, Copy)]
enum Layout { Font, Attribute, Tabular }

fn layout_release(layout: Layout, complete: bool) {
    let name = "A".repeat(1024);
    let (entity_type, form, parameters) = match (layout, complete) {
        (Layout::Font, true) => (310, 0, format!("310,1,1024H{name},0,1,1,65,0,0,0;")),
        (Layout::Font, false) => (310, 0, format!("310,1,1024H{name},0,1,2,65,0,0,0,66,0,0,1;")),
        (Layout::Attribute, true) => (322, 2, format!("322,1024H{name},0,1,1,1,1,42,0;")),
        (Layout::Attribute, false) => (322, 2, format!("322,1024H{name},0,2,1,1,1,42,0,2,1,1;")),
        (Layout::Tabular, true) => {
            let values = (0..64).map(|value| value.to_string()).collect::<Vec<_>>().join(",");
            (406, 11, format!("406,70,0,64,1,1,1,2,{values};"))
        }
        (Layout::Tabular, false) => unreachable!("tabular source requires dependent values"),
    };
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type, form, label: "LAYOUT".into(), status: "00000200", parameters,
    }]);
    let parse_arena = DecodeArena::new();
    let (parse_ctx, _) = DecodeContext::from_root_bytes(&bytes, &parse_arena, &DecodePolicy::service()).unwrap();
    let scan = crate::card::scan_with_context(&bytes, &parse_ctx).unwrap();
    let (global, _, _global_storage) = crate::global::parse(&scan, &parse_ctx).unwrap();
    let (directory, quarantined) = crate::directory::parse(&scan, global.global_table(), &parse_ctx).unwrap();
    assert!(quarantined.is_empty());
    let assembly = crate::parameter::assemble_with_context(&scan, &directory, &quarantined, &global, &parse_ctx).unwrap();
    assert!(assembly.quarantined.is_empty());
    assert_eq!(directory.len(), 1);
    assert_eq!(assembly.records.len(), 1);
    let record = &assembly.records[0];
    // Before these builders only cards, two single-node indexes, one entity,
    // and the four-slot display vector own backing. The entity keeps exact
    // parameter bytes, token slots, and string-token bytes. No pointer is set.
    let string_bytes: usize = record.tokens().iter().map(|token| match &token.value {
        TokenValue::String(value) => value.len(), _ => 0,
    }).sum();
    let prefix = scan.cards().len() * size_of::<NativeCard<'_>>()
        + tree_node::<u32, &DirectoryEntry>() + tree_node::<u32, usize>()
        + size_of::<NativeEntity>() + "iges:entity:directory#1".len()
        + record.bytes.len() + record.tokens().len() * size_of::<Token>()
        + record.comment.len() + string_bytes
        + 4 * size_of::<NativeDisplayAttributes>()
        + "iges:presentation:display-attributes#D1".len() + "iges:entity:directory#1".len();
    let (used, additional, operation, arena_name, payload_field) = match layout {
        Layout::Font => (
            prefix + 4 * size_of::<NativeTextFontDefinition>()
                + if complete { size_of::<NativeGlyph>() } else { 0 }
                + "iges:presentation:text-font#D1".len() + "iges:entity:directory#1".len(),
            name.len(), "iges native text font name", "text_fonts", "characters",
        ),
        Layout::Attribute => (
            prefix + 4 * size_of::<NativeAttributeTableDefinition>()
                + if complete { size_of::<NativeAttributeDefinition>() + size_of::<NativeAttributeValue>() } else { 0 }
                + "iges:product:attribute-definition#D1".len() + "iges:entity:directory#1".len(),
            name.len(), "iges native attribute definition name", "attribute_table_definitions", "attributes",
        ),
        Layout::Tabular => (
            prefix + 4 * size_of::<NativeProperty>() + size_of::<NativeIndependentVariable>() + size_of::<Option<f64>>(),
            64 * size_of::<Option<f64>>(), "iges native tabular dependent values", "properties", "dependent_values",
        ),
    };
    let used = u64_from_index(used);
    let additional = u64_from_index(additional);
    let peak = used + additional;
    let projection = crate::entities::geometry::Projection::default();
    for cap in [Some(peak - 1), Some(peak), None] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap.unwrap_or(EMPTY_ROOT_MATERIALIZED_ALLOWANCE);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut references = BTreeMap::new();
        let mut run = || super::super::store(
            &mut ir,
            NativeStoreInputs {
                scan: &scan, directory: &directory, parameters: &assembly.records,
                trailing_pointer_analysis: &assembly.trailing_pointer_analysis,
                quarantine: QuarantinedRecords { directory: &quarantined, parameters: &assembly.quarantined },
                structure_admitted: Some(&projection), sequences: &projection.sequences,
                boundary_vertex_derivations: &projection.boundary_vertex_derivations,
            },
            &mut references, &global, ProductOccurrenceLimits::new(100_000, 64), &ctx,
        ).map(drop);
        let result = run();
        if let Some(cap) = cap {
            let original = match result {
                Err(CodecError::ResourceLimit(original)) => original,
                Err(error) => panic!("unexpected layout error: {error:?}"),
                Ok(()) => panic!("expected layout phase allocation refusal"),
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            if cap < peak {
                assert_eq!(original.operation, operation);
                assert_eq!((original.limit, original.used, original.additional), (cap, used, additional));
            } else {
                // The phase limit admits the repaired allocation. It does
                // not admit every later store phase. Tabular output next
                // needs its property identity. The other cases reach card
                // serialization: both single-node indexes are destroyed,
                // the occurrence-state identity is live. Record-vector
                // backing is retained; materialized overlap for eight
                // existing slots fits, but sixteen slots do not.
                let (later_used, later_additional, later_operation) = match layout {
                    Layout::Tabular => (peak, u64_from_index("iges:application:property#D1".len()), "iges native property id"),
                    Layout::Font | Layout::Attribute => (
                        peak - u64_from_index(tree_node::<u32, &DirectoryEntry>() + tree_node::<u32, usize>())
                            + u64_from_index("iges:product:occurrence-expansion#state".len()),
                        u64_from_index(16 * size_of::<cadmpeg_ir::NativeRecord>()), "store native record",
                    ),
                };
                if !matches!(layout, Layout::Tabular) {
                    assert!(scan.cards().len() > 16);
                    assert!(later_used + u64_from_index(8 * size_of::<cadmpeg_ir::NativeRecord>()) <= cap);
                }
                assert!(later_used + later_additional > cap);
                assert_eq!(original.operation, later_operation);
                assert_eq!((original.limit, original.used, original.additional), (cap, later_used, later_additional));
            }
            for _ in 0..64 {
                assert!(matches!(run(), Err(CodecError::ResourceLimit(last)) if last == original));
            }
            assert_eq!(ir.model, cadmpeg_ir::CadIr::empty().model);
            if cap < peak || matches!(layout, Layout::Tabular) {
                assert_eq!(ir, cadmpeg_ir::CadIr::empty());
            } else {
                assert!(ir.native.namespace("iges").unwrap().arenas().is_empty());
            }
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == original));
        } else {
            result.unwrap();
            let native = ir.native.namespace("iges").unwrap();
            let fields = native.arenas()[arena_name][0].fields();
            let payload = fields[payload_field].as_array().unwrap();
            match layout {
                Layout::Font => {
                    assert_eq!(payload.len(), usize::from(complete));
                    if complete {
                        assert_eq!(payload[0]["character_code"], 65);
                        assert_eq!(payload[0]["motions"], serde_json::json!([]));
                    }
                }
                Layout::Attribute => {
                    assert_eq!(payload.len(), usize::from(complete));
                    if complete {
                        assert_eq!(payload[0]["attribute_type"], 1);
                        assert_eq!(payload[0]["values"][0]["display_template"], serde_json::Value::Null);
                    }
                }
                Layout::Tabular => {
                    assert_eq!(payload.len(), 64);
                    for (index, value) in payload.iter().enumerate() {
                        assert_eq!(value.as_f64().unwrap(), f64::from(u32::try_from(index).unwrap()));
                    }
                    assert_eq!(fields["independent_variables"][0]["values"], serde_json::json!([2.0]));
                }
            }
            let released = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
                "test native layout scratch release").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn native_complete_glyph_layout_releases_before_font_name() { layout_release(Layout::Font, true); }
#[test]
fn native_incomplete_glyph_layout_releases_before_font_name() { layout_release(Layout::Font, false); }
#[test]
fn native_complete_attribute_layout_releases_before_definition_name() { layout_release(Layout::Attribute, true); }
#[test]
fn native_incomplete_attribute_layout_releases_before_definition_name() { layout_release(Layout::Attribute, false); }
#[test]
fn native_tabular_layout_releases_before_dependent_values() { layout_release(Layout::Tabular, true); }
