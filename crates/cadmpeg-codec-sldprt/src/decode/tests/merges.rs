// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::topology::Point;

use super::super::{merge_brep, Brep};

#[test]
fn brep_merge_propagates_annotation_refusal_from_the_decode_context() {
    let mut target = Brep::default();
    let mut source = Brep::default();
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("source"));
    let mut first = AnnotationBuilder::new();
    first.note(&cadmpeg_test_support::service_decode_context(), "sldprt:model:point#first", &stream, 1, None).unwrap();
    target.annotations = first.build();
    let before = target.annotations.clone();
    let mut second = AnnotationBuilder::new();
    second.note(&cadmpeg_test_support::service_decode_context(), "sldprt:model:point#second", &stream, 2, None).unwrap();
    source.annotations = second.build();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = merge_brep(&ctx, &mut target, source) else { panic!("the annotation merge must retain the caller refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "merge SLDPRT annotation identities");
    assert_eq!(target.annotations, before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn brep_merge_admits_destination_arena_storage_and_moves() {
    for dimension in [ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes, ResourceDimension::WorkUnits] {
        let mut target = Brep::default();
        let mut source = Brep::default();
        source.points.push(Point::new("sldprt:model:point#one".try_into().unwrap(), FinitePoint3::ZERO, None));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = merge_brep(&ctx, &mut target, source) else { panic!("arena merge must retain the caller refusal"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, if dimension == ResourceDimension::WorkUnits { "merge SLDPRT B-rep arena moves" } else { "merge SLDPRT B-rep arena" });
        assert!(target.points.is_empty());
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
    let mut target = Brep::default();
    let mut source = Brep::default();
    source.points.push(Point::new("sldprt:model:point#one".try_into().unwrap(), FinitePoint3::ZERO, None));
    let identity_storage = source.points[0].id.as_str().as_ptr();
    merge_brep(&cadmpeg_test_support::service_decode_context(), &mut target, source).unwrap();
    assert_eq!(target.points[0].id.as_str(), "sldprt:model:point#one");
    assert_eq!(target.points[0].id.as_str().as_ptr(), identity_storage);
}
