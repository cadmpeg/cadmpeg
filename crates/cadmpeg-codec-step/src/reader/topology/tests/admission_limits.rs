// SPDX-License-Identifier: Apache-2.0
//! Collection refusals in the STEP topology reader.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn relationship_refusal(collection_limit: u64) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy)
        .expect("empty root fits policy");
    super::super::push_topology_group(
        &mut BTreeMap::new(), 1u64, 2u64, &ctx,
        "step_shape_relationship_groups", "step_shape_relationship_members",
    ).expect_err("relationship exceeds collection limit")
}

#[test]
fn shape_relationship_groups_refuse_collection_limit() {
    assert!(matches!(relationship_refusal(0), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_shape_relationship_groups"));
}

#[test]
fn shape_relationship_members_refuse_collection_limit() {
    assert!(matches!(relationship_refusal(1), CodecError::ResourceLimit(refusal)
        if refusal.dimension == ResourceDimension::CollectionItems
            && refusal.operation == "step_shape_relationship_members"));
}

#[test]
fn shape_relationship_graph_preserves_bidirectional_edges() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=SHAPE_REPRESENTATION('',(),$);#2=SHAPE_REPRESENTATION('',(),$);#3=SHAPE_REPRESENTATION_RELATIONSHIP('','',#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::parse::parse(source).expect("valid relationship graph");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let graph = super::super::shape_representation_relationships(&exchange, &ctx)
        .expect("relationship graph fits policy");
    assert_eq!(graph.get(&1).map(Vec::as_slice), Some(&[2][..]));
    assert_eq!(graph.get(&2).map(Vec::as_slice), Some(&[1][..]));
}
