// SPDX-License-Identifier: Apache-2.0

use super::super::exact_edge_flange_operation;
use crate::layout::edge_flange_class286_two_sided_per_edge_fixed_operation as layout;
use crate::records::feature::sheet_metal::{DesignEdgeFlangeEdge, DesignFlangeEdgeWidth};
use crate::test_support::write_marked_reference;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const REFERENCES: [u32; 28] = [
    201, 204, 207, 210, 213, 216, 219, 222, 225, 228, 231, 234, 237, 240, 243, 246,
    249, 252, 255, 258, 261, 264, 267, 270, 273, 276, 279, 282,
];

// The original class-286 extended fixture values and settled layout constants.
fn extended_frame() -> Vec<u8> {
    let mut bytes = vec![0; 801];
    for (at, value) in [
        (layout::BEND_POSITION, 3u32),
        (layout::EDGE_COUNT, 2),
        (layout::HEIGHT_DATUM, 2),
        (layout::REFERENCE_SIDE, 4),
        (layout::RESULT_COUNT, 5),
        (layout::RESULT_SEPARATOR, 1),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (layout::EDGE_WRAPPER_ONE_REFERENCE, 201),
        (layout::EDGE_WRAPPER_TWO_REFERENCE, 213),
        (layout::SETTINGS_REFERENCE, 240),
        (layout::ANGLE_OWNER_REFERENCE, 228),
        (layout::HEIGHT_OWNER_REFERENCE, 225),
        (layout::AGGREGATE_GROUP_REFERENCE, 231),
        (layout::EDGE_GROUP_ONE_REFERENCE, 204),
        (layout::EDGE_GROUP_TWO_REFERENCE, 216),
    ] {
        write_marked_reference(&mut bytes, at, value);
    }
    bytes[layout::INSIDE_BEND_RADIUS..layout::INSIDE_BEND_RADIUS + 8]
        .copy_from_slice(&0.254f64.to_le_bytes());
    for (ordinal, (record, trailer)) in [(501u32, 1u32), (502, 1), (503, 1), (504, 1), (505, 0)].into_iter().enumerate() {
        write_marked_reference(&mut bytes, layout::RESULT_ONE_REFERENCE + ordinal * 15, record);
        let at = layout::RESULT_ONE_TRAILER + ordinal * 15;
        bytes[at..at + 4].copy_from_slice(&trailer.to_le_bytes());
    }
    bytes
}

fn output_bytes() -> u64 {
    // Exact vector_storage: two surviving edge/owner rows and twelve u32s.
    u64::try_from(2 * std::mem::size_of::<DesignFlangeEdgeWidth<[u32; 2]>>() + 12 * std::mem::size_of::<u32>()).unwrap()
}

#[test]
fn fixed_flange_selected_storage_is_exact_with_free_constant_grammar_and_native_order() {
    let bytes = extended_frame();
    let native = bytes.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = output_bytes();
    policy.limits.max_retained_bytes = output_bytes();
    policy.limits.max_collection_items = 14;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let operation = exact_edge_flange_operation(&ctx, &bytes, 0, 801, "286", "258", &REFERENCES).unwrap().expect("selected native flange");
    assert_eq!(operation.selection.aggregate_group_record_index(), 231);
    assert_eq!(operation.selection.shape().edges().map(|edge| edge.wrapper).collect::<Vec<_>>(), [201, 213]);
    assert_eq!(operation.selection.shape().edges().map(DesignEdgeFlangeEdge::operand_record_index).collect::<Vec<_>>(), [207, 219]);
    assert_eq!(operation.selection.shape().edges().map(|edge| edge.aggregate_operand_record_index).collect::<Vec<_>>(), [279, 282]);
    assert_eq!(operation.selection.shape().owner_indices().copied().collect::<Vec<_>>(), [210, 222, 234, 237]);
    assert_eq!(operation.auxiliary_reference_record_indices, [243, 246, 249, 252, 255, 258, 261, 264, 267, 270, 273, 276]);
    assert_eq!(operation.auxiliary_reference_record_indices.capacity(), 12);
    match operation.selection.shape() {
        crate::records::feature::sheet_metal::DesignEdgeFlangeShape::TwoSidesPerEdge { edges, source } => {
            assert_eq!(edges.capacity(), 2);
            assert_eq!(edges.iter().map(|row| row.owners).collect::<Vec<_>>(), [[210, 222], [234, 237]]);
            assert_eq!(*source, crate::records::feature::sheet_metal::DesignEdgeFlangeWidthParameterSource::EdgeOffset);
        }
        _ => panic!("native two-sided per-edge width"),
    }
    assert_eq!(operation.bend_radius.get(), 0.254);
    assert_eq!(operation.bend_radius_offset, u64::try_from(layout::INSIDE_BEND_RADIUS).unwrap());
    assert_eq!(bytes, native);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn fixed_flange_real_backing_items_and_retained_handoff_keep_original_refusal() {
    let bytes = extended_frame();
    let native = bytes.clone();
    let width_bytes = u64::try_from(2 * std::mem::size_of::<DesignFlangeEdgeWidth<[u32; 2]>>()).unwrap();
    for (dimension, cap, operation) in [
        (ResourceDimension::MaterializedBytes, width_bytes - 1, "F3D fixed flange width edges"),
        (ResourceDimension::MaterializedBytes, output_bytes() - 1, "F3D fixed flange auxiliary references"),
        (ResourceDimension::CollectionItems, 0, "F3D fixed flange width edges"),
        (ResourceDimension::CollectionItems, 2, "F3D fixed flange auxiliary references"),
        (ResourceDimension::RetainedBytes, output_bytes() - 1, "F3D fixed flange operation"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            _ => panic!("fixed flange admission dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = exact_edge_flange_operation(&ctx, &bytes, 0, 801, "286", "258", &REFERENCES) else { panic!("actual flange admission must refuse"); };
        assert_eq!(original.dimension, dimension);
        assert_eq!(original.limit, cap);
        assert_eq!(original.operation, operation);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(ctx.charge_work(0, "later flange boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
    }
    assert_eq!(bytes, native);
}

#[test]
fn fixed_flange_rejected_duplicate_frames_allocate_no_candidate_backing() {
    let bytes = extended_frame();
    for ordinal in 1..5 {
        let mut rejected = bytes.clone();
        write_marked_reference(&mut rejected, layout::RESULT_ONE_REFERENCE + ordinal * 15, 501);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&rejected, &arena, &policy).unwrap();
        assert!(exact_edge_flange_operation(&ctx, &rejected, 0, 801, "286", "258", &REFERENCES).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn fixed_flange_candidate_backing_follows_actual_parent_and_drops_before_refund() {
    let bytes = extended_frame();
    for keep_live in [true, false] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = output_bytes();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let (operation, storage) = ctx.with_scoped_storage("actual outer flange candidate", || exact_edge_flange_operation(&ctx, &bytes, 0, 801, "286", "258", &REFERENCES)).unwrap();
        let operation = operation.expect("native flange candidate");
        assert_eq!(operation.auxiliary_reference_record_indices.len(), 12);
        if keep_live {
            let Err(CodecError::ResourceLimit(original)) = ctx.reserve_scoped(1, "flange backing remains live") else { panic!("live native backing must remain owned"); };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(ctx.resource_refusal(), Some(original));
            drop(operation);
            drop(storage);
        } else {
            drop(operation);
            drop(storage);
            drop(ctx.reserve_scoped(output_bytes(), "after actual flange destruction").unwrap());
            assert!(ctx.resource_refusal().is_none());
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn fixed_flange_preexisting_original_fuse_dominates_invalid_and_valid_grammar() {
    let bytes = extended_frame();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original fixed flange refusal") else { panic!("original flange refusal"); };
        for (input, paired) in [(&[][..], usize::MAX), (bytes.as_slice(), 801)] {
            assert!(matches!(exact_edge_flange_operation(ctx, input, 0, paired, "286", "258", &REFERENCES), Err(CodecError::ResourceLimit(later)) if later == original));
        }
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
