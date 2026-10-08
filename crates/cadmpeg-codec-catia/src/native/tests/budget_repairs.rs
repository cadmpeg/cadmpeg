// SPDX-License-Identifier: Apache-2.0
//! Native projection storage and traversal admission.

use crate::test_support::{
    with_materialized_limit, with_retained_limit, with_service_context, with_work_limit,
};
use cadmpeg_core::{
    decode::{u64_from_index, ResourceDimension},
    CodecError,
};

#[test]
fn native_rejected_signature_releases_speculative_storage() {
    with_materialized_limit(4096, |ctx| -> Result<_, CodecError> {
        for source in [
            "(#1_ : #In Real,bad) : Real",
            "(#1_ : #In Real,#1_ : #In Real) : Real",
        ] {
            assert!(super::super::relation_type_signature_charged(ctx, None, source)?.is_none());
            let storage = ctx.reserve_scoped(4096, "test released signature")?;
            drop(storage);
        }
        Ok(())
    })
    .expect("rejected signatures release their input copies");
    with_retained_limit(0, |ctx| -> Result<_, CodecError> {
        assert!(super::super::relation_type_signature_charged(
            ctx,
            None,
            "(#1_ : #In Real,bad) : Real"
        )?
        .is_none());
        Ok(())
    })
    .expect("rejected signature retains no input copies");
}

#[test]
fn native_owner_packet_boxes_refuse_retained_storage() {
    use crate::test_support::test_b2::{
        b2_fixed_owner_boundary_cycle_stream, b2_owner_chart_stream,
    };
    for bytes in [
        b2_owner_chart_stream(0x28),
        b2_fixed_owner_boundary_cycle_stream().0,
    ] {
        let records = crate::wire::records::consolidated_records(&bytes);
        let refused = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            "catia_native_owner_packet_box",
            |cap| {
                with_retained_limit(cap, |ctx| {
                    super::super::projection::consolidated_owner_packets(ctx, &bytes, &records)
                })
            },
        );
        assert!(
            matches!(refused, CodecError::ResourceLimit(limit) if limit.operation == "catia_native_owner_packet_box")
        );
        let packets = with_service_context(|ctx| {
            super::super::projection::consolidated_owner_packets(ctx, &bytes, &records)
        })
        .expect("owner packet boxes admitted");
        assert_eq!(packets.len(), 1);
    }
}

#[test]
fn native_vertex_reference_membership_preserves_order_with_linear_work() {
    let native = crate::native::CatiaNative::decode(
        &crate::test_support::test_a5_bound::a5_native_edge_run_stream(6, 14, 15),
    );
    let template = &native.consolidated_edge_nodes[0];
    let nodes: Vec<_> = (0..1024)
        .map(|identity| {
            let mut node = template.clone();
            node.endpoint_records = Some([100, 200]);
            node.vertex_refs = [identity, identity + 2000];
            node
        })
        .collect();
    // Two endpoint lookups and insertions, two incident-name comparisons,
    // and geometric table/vector relocation fit within 512 units per node.
    // Scanning the growing reference vectors requires more than 4 million units.
    let vertices = with_work_limit(1024 * 512, |ctx| {
        super::super::edge_node::consolidated_vertex_identities(ctx, &nodes)
    })
    .expect("membership work is linear in distinct distances");
    assert_eq!(vertices.len(), 2);
    assert_eq!(vertices[0].reference_values, (0..1024).collect::<Vec<_>>());
    assert_eq!(
        vertices[1].reference_values,
        (2000..3024).collect::<Vec<_>>()
    );
    let refused = crate::test_support::with_work_refusal(
        "catia_native_vertex_identity_reference_checks",
        |ctx| super::super::edge_node::consolidated_vertex_identities(ctx, &nodes),
    );
    assert!(
        matches!(refused, Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_native_vertex_identity_reference_checks")
    );
}

#[test]
fn native_relation_index_preserves_signature_order_with_bounded_work() {
    use super::super::{
        CatiaEntityReference, CatiaRelationParameterDependency, CatiaRelationTypeInput,
        CatiaRelationTypeSignature,
    };
    let signature = CatiaRelationTypeSignature {
        inputs: (1..=2048)
            .map(|index| CatiaRelationTypeInput {
                parameter: format!("#{index}_"),
                input_type: "Real".into(),
            })
            .collect(),
        result_type: "Real".into(),
    };
    let dependencies: Vec<_> = (1..=2048)
        .rev()
        .map(|index| CatiaRelationParameterDependency {
            source_offset: 0,
            symbol: format!("#{index}_/12"),
            candidates: vec![CatiaEntityReference::Resolved {
                entity_id: index,
                entity: format!("entity:{index}"),
                class_name: None,
            }],
        })
        .collect();
    // Index construction, grouped slots and bounded lookups fit within 1024 units
    // per input. A full signature scan per dependency exceeds four million steps.
    let inputs = with_work_limit(2048 * 1024, |ctx| {
        super::super::resolved_relation_program_inputs(ctx, &signature, &dependencies)
    })
    .expect("indexed relation work")
    .expect("complete inputs");
    assert_eq!(inputs.len(), signature.inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        assert_eq!(input.parameter, signature.inputs[index].parameter);
        assert_eq!(
            input.entity.entity_id(),
            u32::try_from(index + 1).expect("fixture index")
        );
    }
    let mut duplicate = signature;
    duplicate.inputs.push(duplicate.inputs[0].clone());
    assert!(with_service_context(|ctx| {
        super::super::resolved_relation_program_inputs(ctx, &duplicate, &dependencies)
    })
    .expect("duplicate index")
    .is_none());
}

#[test]
fn native_signature_stops_at_first_invalid_clause() {
    let source = format!("(bad,{}) : Real", "#2_ : #In Real,".repeat(2048));
    with_service_context(|ctx| {
        assert!(
            super::super::relation_type_signature_charged(ctx, None, &source)
                .expect("invalid first clause")
                .is_none()
        );
    });
    let refusal =
        crate::test_support::with_work_refusal("catia_native_signature_clause_visits", |ctx| {
            super::super::relation_type_signature_charged(ctx, None, &source)
        });
    assert!(matches!(refusal, Err(CodecError::ResourceLimit(limit)) if limit.additional == 1));
}

#[test]
fn native_extent_index_preserves_strict_containment_and_overlap_boundaries() {
    let sources = [
        (10_usize, 10),
        (2, 20),
        (5, 2),
        (10, 20),
        (8, 0),
        (usize::MAX, 1),
    ];
    with_service_context(|ctx| -> Result<_, CodecError> {
        let (index, _storage) = ctx.with_scoped_storage("test extent index", || {
            super::super::NativeExtentIndex::new(ctx, &sources, |&extent| extent)
        })?;
        for start in (0..32).chain([usize::MAX]) {
            for len in 0..16 {
                assert_eq!(
                    index.contains(ctx, start, len, "test indexed containment")?,
                    sources
                        .iter()
                        .any(|&(owner, width)| crate::object_graph::extent_contains(
                            owner, width, start, len
                        ))
                );
                assert_eq!(
                    index.overlaps(ctx, start, len, "test indexed overlap")?,
                    sources
                        .iter()
                        .any(|&(owner, width)| crate::checked::extents_overlap(
                            owner, width, start, len
                        ))
                );
            }
        }
        Ok(())
    })
    .expect("indexed predicates preserve interval boundaries and overflow");
}

#[test]
fn native_inventory_overlap_filter_has_bounded_work_and_preserves_source_order() {
    let mut graphs: Vec<_> = (0..8192)
        .rev()
        .map(|index| crate::object_graph::ObjectGraph {
            pos: index * 64 + 32,
            total_len: 8,
            catalog_pos: None,
            records: Vec::new(),
        })
        .collect();
    let mut blocks: Vec<_> = (0..8192)
        .map(|index| crate::value_block::ValueBlock {
            pos: index * 64,
            payload: Vec::new(),
        })
        .collect();
    let graph_offsets: Vec<_> = graphs.iter().map(|graph| graph.pos).collect();
    let block_offsets: Vec<_> = blocks.iter().map(|block| block.pos).collect();
    // Ordered index construction and logarithmic queries fit this allowance.
    // The old block-by-graph scan alone needs 8192 squared comparisons.
    with_work_limit(8192 * 4096, |ctx| {
        super::super::filter_nested_inventory(ctx, &mut graphs, &mut blocks, &mut Vec::new())
    })
    .expect("overlap filtering does not scan every graph for every block");
    assert_eq!(
        graphs.iter().map(|graph| graph.pos).collect::<Vec<_>>(),
        graph_offsets
    );
    assert_eq!(
        blocks.iter().map(|block| block.pos).collect::<Vec<_>>(),
        block_offsets
    );
}

#[test]
fn native_alias_overlap_filter_has_bounded_work_and_preserves_source_order() {
    let graphs: Vec<_> = (0..8192)
        .rev()
        .map(|index| super::super::CatiaObjectGraph {
            id: String::new(),
            byte_offset: index * 64 + 32,
            byte_len: 8,
            finjpl_segment: None,
            outer_container: None,
            catalog_byte_offset: None,
            catalog: None,
            records: Vec::new(),
        })
        .collect();
    let mut rows: Vec<_> = (0..8192)
        .map(|index| super::super::CatiaAliasRow {
            id: String::new(),
            byte_offset: index * 64 + u64_from_index(crate::layout::outer_alias_row::MARKER),
            lead_raw: 0,
            tag_raw: 0,
            flag: 0,
            f1: [0; 3],
            object_graph: None,
            object_record: None,
            design_object: None,
            f2: 0,
            f3: 0,
            group: None,
            canonical_surface_tag: None,
        })
        .collect();
    let offsets: Vec<_> = rows.iter().map(|row| row.byte_offset).collect();
    // The old row-by-graph scan needs 8192 squared comparisons.
    with_work_limit(8192 * 2048, |ctx| {
        super::super::filter_nested_alias_rows(ctx, &mut rows, &graphs, &[], &[])
    })
    .expect("alias filtering uses indexed overlap queries");
    assert_eq!(
        rows.iter().map(|row| row.byte_offset).collect::<Vec<_>>(),
        offsets
    );
}

#[test]
fn native_inventory_index_preserves_transitive_and_equal_start_containment() {
    let mut graphs = vec![crate::object_graph::ObjectGraph {
        pos: 10,
        total_len: 20,
        catalog_pos: None,
        records: Vec::new(),
    }];
    // The complete block spans [0, 40), including its header and terminator.
    let mut blocks = vec![crate::value_block::ValueBlock {
        pos: 0,
        payload: vec![0; 33],
    }];
    let mut catalogs: Vec<_> = [(15, 4), (0, 1), (32, 8)]
        .into_iter()
        .map(|(pos, total_len)| crate::catalog::Catalog {
            pos,
            total_len,
            entries: Vec::new(),
        })
        .collect();
    with_service_context(|ctx| {
        super::super::filter_nested_inventory(ctx, &mut graphs, &mut blocks, &mut catalogs)
    })
    .expect("indexed inventory containment");
    assert!(graphs.is_empty());
    assert_eq!(blocks.len(), 1);
    assert_eq!(catalogs.len(), 1);
    assert_eq!(catalogs[0].pos, 0);
}
