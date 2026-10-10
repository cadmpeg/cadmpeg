// SPDX-License-Identifier: Apache-2.0

use super::super::{
    bind_recipe_reference_candidates_from, segment_types_by_entity, CompanionBoundaries,
};
use super::intervals::{owner_at, parameter_at};
use crate::records::decal::DesignRecordHeader;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::entity_header::SegmentType;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::sketch_links::PersistentSubentityTag;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{EdgeId, FaceId};
use std::cell::Cell;

const ORIGINAL: &str = "original F3D admitted iterator refusal";
const ORDINARY: &str = "ordinary F3D admitted iterator error";

fn with_zero_work<T>(test: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, test)
}

fn caught<'a, T>(
    ctx: &'a DecodeContext<'_>,
    value: &'a T,
    calls: &'a Cell<usize>,
    outcome: usize,
) -> impl Iterator<Item = Result<&'a T, CodecError>> + 'a {
    std::iter::from_fn(move || {
        calls.set(calls.get() + 1);
        assert!(matches!(
            ctx.charge_work(1, ORIGINAL),
            Err(CodecError::ResourceLimit(_))
        ));
        match outcome {
            0 => None,
            1 => Some(Err(CodecError::Malformed(ORDINARY.into()))),
            _ => Some(Ok(value)),
        }
    })
}

/// A real IntoIterator callback that catches the context's original refusal.
struct CaughtInput<'a, 'source, T> {
    ctx: &'a DecodeContext<'source>,
    value: &'a T,
    calls: &'a Cell<usize>,
}

impl<'a, T> IntoIterator for CaughtInput<'a, '_, T> {
    type Item = Result<&'a T, CodecError>;
    type IntoIter = std::iter::Once<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.calls.set(self.calls.get() + 1);
        assert!(matches!(
            self.ctx.charge_work(1, ORIGINAL),
            Err(CodecError::ResourceLimit(_))
        ));
        std::iter::once(Ok(self.value))
    }
}

fn assert_original<T>(ctx: &DecodeContext<'_>, result: Result<T, CodecError>, calls: &Cell<usize>) {
    let Err(CodecError::ResourceLimit(original)) = result else {
        panic!("the iterator boundary must return the original refusal");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, ORIGINAL);
    assert_eq!(original.limit, 0);
    assert_eq!(original.used, 0);
    assert_eq!(original.additional, 1);
    assert_eq!(calls.get(), 1);
    assert_eq!(ctx.resource_refusal(), Some(original.clone()));
    assert!(
        matches!(ctx.charge_work(0, "later F3D admitted iterator boundary"), Err(CodecError::ResourceLimit(later)) if later == original)
    );
}

fn scope() -> DesignParameterScope {
    DesignParameterScope::empty(
        "f3d:stream:parameter-scope#42",
        DesignFeatureKind::Sketch,
        42,
    )
}

fn header() -> DesignRecordHeader {
    DesignRecordHeader {
        id: "f3d:stream:record-header#40".into(),
        record_index: 42,
        class_tag: "302".to_owned().try_into().unwrap(),
        byte_offset: 40,
    }
}

fn reference() -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "13".into(),
        token_offset: 4,
        design_reference: 331,
        design_reference_offset: 8,
        candidate_faces: Vec::new(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

fn tag() -> PersistentSubentityTag {
    PersistentSubentityTag {
        id: "f3d:stream:tag#1".into(),
        target: AttributeTarget::Face(FaceId::mint("test:model:face#a").unwrap()),
        selector: 1,
        token: "13".try_into().unwrap(),
        design_references: vec![331],
        ordinal: 0,
    }
}

fn segment_type(version: u32, byte_offset: u64) -> SegmentType {
    let mut data = crate::design::test_support::design_type(
        super::super::DIMENSION_PRESENTATION_V3_TYPE_GUID,
        None,
        version,
        "MSketch",
        vec![42],
    );
    data.byte_offset = byte_offset;
    let id = crate::test_support::with_decode_context(|ctx| {
        crate::ids::native_design_type_id(ctx, "Design/MetaStream.dat", byte_offset).unwrap()
    });
    SegmentType::try_new(id, data).unwrap()
}

#[test]
fn companion_owner_iterator_keeps_original_before_end_error_or_value() {
    let owner = owner_at(10, 13, 12, 11, 10);
    for outcome in 0..3 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            assert_original(
                ctx,
                CompanionBoundaries::build(
                    ctx,
                    caught(ctx, &owner, &calls, outcome),
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                ),
                &calls,
            );
        });
    }
}

#[test]
fn companion_parameter_iterator_keeps_original_before_end_error_or_value() {
    let parameter = parameter_at(12, 20);
    for outcome in 0..3 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            assert_original(
                ctx,
                CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    caught(ctx, &parameter, &calls, outcome),
                    std::iter::empty(),
                    std::iter::empty(),
                ),
                &calls,
            );
        });
    }
}

#[test]
fn companion_scope_iterator_keeps_original_before_end_error_or_value() {
    let scope = scope();
    let native = serde_json::to_value(&scope).unwrap();
    for outcome in 0..3 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            assert_original(
                ctx,
                CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    std::iter::empty(),
                    caught(ctx, &scope, &calls, outcome),
                    std::iter::empty(),
                ),
                &calls,
            );
        });
    }
    assert_eq!(serde_json::to_value(&scope).unwrap(), native);
}

#[test]
fn companion_header_iterator_keeps_original_before_end_error_or_value() {
    let header = header();
    let native = serde_json::to_value(&header).unwrap();
    for outcome in 0..3 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            assert_original(
                ctx,
                CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                    caught(ctx, &header, &calls, outcome),
                ),
                &calls,
            );
        });
    }
    assert_eq!(serde_json::to_value(&header).unwrap(), native);
}

#[test]
fn companion_into_iterator_keeps_original_before_first_record_at_each_input() {
    let owner = owner_at(10, 13, 12, 11, 10);
    let parameter = parameter_at(12, 20);
    let scope = scope();
    let header = header();
    for stage in 0..4 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            let result = match stage {
                0 => CompanionBoundaries::build(
                    ctx,
                    CaughtInput {
                        ctx,
                        value: &owner,
                        calls: &calls,
                    },
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                ),
                1 => CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    CaughtInput {
                        ctx,
                        value: &parameter,
                        calls: &calls,
                    },
                    std::iter::empty(),
                    std::iter::empty(),
                ),
                2 => CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    std::iter::empty(),
                    CaughtInput {
                        ctx,
                        value: &scope,
                        calls: &calls,
                    },
                    std::iter::empty(),
                ),
                _ => CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                    CaughtInput {
                        ctx,
                        value: &header,
                        calls: &calls,
                    },
                ),
            };
            assert_original(ctx, result, &calls);
        });
    }
}

#[test]
fn recipe_tag_iterator_keeps_original_before_end_error_or_candidate() {
    let tag = tag();
    let native = serde_json::to_value(&tag).unwrap();
    for outcome in 0..3 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            let mut reference = reference();
            assert_original(
                ctx,
                bind_recipe_reference_candidates_from(
                    ctx,
                    &mut reference,
                    caught(ctx, &tag, &calls, outcome),
                    None,
                ),
                &calls,
            );
            assert_eq!(reference, self::reference());
        });
    }
    assert_eq!(serde_json::to_value(&tag).unwrap(), native);
}

#[test]
fn recipe_tag_into_iterator_keeps_original_before_first_candidate() {
    let tag = tag();
    with_zero_work(|ctx| {
        let calls = Cell::new(0);
        let mut reference = reference();
        assert_original(
            ctx,
            bind_recipe_reference_candidates_from(
                ctx,
                &mut reference,
                CaughtInput {
                    ctx,
                    value: &tag,
                    calls: &calls,
                },
                None,
            ),
            &calls,
        );
        assert_eq!(reference, self::reference());
    });
}

#[test]
fn recipe_tag_prefused_entry_preserves_all_prior_candidates_without_visiting_input() {
    let tag = tag();
    let mut reference = reference();
    reference
        .candidate_faces
        .push(FaceId::mint("test:model:face#a").unwrap());
    reference
        .candidate_edges
        .push(EdgeId::mint("test:model:edge#a").unwrap());
    reference
        .alternate_selector_faces
        .push(FaceId::mint("test:model:face#b").unwrap());
    reference
        .alternate_selector_edges
        .push(EdgeId::mint("test:model:edge#b").unwrap());
    let native = reference.clone();
    with_zero_work(|ctx| {
        let calls = Cell::new(0);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, ORIGINAL) else {
            panic!("original refusal");
        };
        assert!(
            matches!(bind_recipe_reference_candidates_from(ctx, &mut reference, CaughtInput { ctx, value: &tag, calls: &calls }, None), Err(CodecError::ResourceLimit(later)) if later == original)
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(reference, native);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn segment_type_iterator_keeps_original_before_end_error_or_registration() {
    let design_type = segment_type(7, 0);
    let native = serde_json::to_value(&design_type).unwrap();
    for outcome in 0..3 {
        with_zero_work(|ctx| {
            let calls = Cell::new(0);
            assert_original(
                ctx,
                segment_types_by_entity(ctx, caught(ctx, &design_type, &calls, outcome)),
                &calls,
            );
        });
    }
    assert_eq!(serde_json::to_value(&design_type).unwrap(), native);
}

#[test]
fn segment_type_into_iterator_keeps_original_before_first_registration() {
    let design_type = segment_type(7, 0);
    with_zero_work(|ctx| {
        let calls = Cell::new(0);
        assert_original(
            ctx,
            segment_types_by_entity(
                ctx,
                CaughtInput {
                    ctx,
                    value: &design_type,
                    calls: &calls,
                },
            ),
            &calls,
        );
    });
}

#[test]
fn admitted_dimension_iterators_preserve_ordinary_errors_without_a_refusal() {
    crate::test_support::with_decode_context(|ctx| {
        let mut reference = reference();
        assert!(
            matches!(bind_recipe_reference_candidates_from(ctx, &mut reference, [Err(CodecError::Malformed(ORDINARY.into()))], None), Err(CodecError::Malformed(message)) if message == ORDINARY)
        );
        assert!(
            matches!(segment_types_by_entity(ctx, [Err(CodecError::Malformed(ORDINARY.into()))]), Err(CodecError::Malformed(message)) if message == ORDINARY)
        );
        for stage in 0..4 {
            let result = match stage {
                0 => CompanionBoundaries::build(
                    ctx,
                    [Err(CodecError::Malformed(ORDINARY.into()))],
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                ),
                1 => CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    [Err(CodecError::Malformed(ORDINARY.into()))],
                    std::iter::empty(),
                    std::iter::empty(),
                ),
                2 => CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    std::iter::empty(),
                    [Err(CodecError::Malformed(ORDINARY.into()))],
                    std::iter::empty(),
                ),
                _ => CompanionBoundaries::build(
                    ctx,
                    std::iter::empty(),
                    std::iter::empty(),
                    std::iter::empty(),
                    [Err(CodecError::Malformed(ORDINARY.into()))],
                ),
            };
            assert!(matches!(result, Err(CodecError::Malformed(message)) if message == ORDINARY));
        }
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn admitted_dimension_empty_iterators_need_no_positive_resource() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let arena = DecodeArena::new();
    let (context, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let ctx = &context;
        let mut reference = reference();
        bind_recipe_reference_candidates_from(ctx, &mut reference, std::iter::empty(), None)
            .unwrap();
        let boundaries = CompanionBoundaries::build(
            ctx,
            std::iter::empty(),
            std::iter::empty(),
            std::iter::empty(),
            std::iter::empty(),
        )
        .unwrap();
        assert!(boundaries.owner_scopes.is_empty());
        assert!(boundaries.offsets.is_empty());
        assert!(boundaries.referenced_headers.is_empty());
        assert!(boundaries.run_starts.is_empty());
        assert!(segment_types_by_entity(ctx, std::iter::empty())
            .unwrap()
            .is_empty());
        assert_eq!(reference, self::reference());
        assert!(ctx.resource_refusal().is_none());
    }
    assert!(context.finish_session().is_ok());
}

#[test]
fn admitted_dimension_iterators_preserve_first_owner_last_type_and_native_candidates() {
    let owners = [owner_at(10, 13, 12, 11, 30), owner_at(10, 14, 12, 11, 10)];
    let parameters = [parameter_at(12, 20)];
    let types = [segment_type(7, 0), segment_type(29, 1)];
    let tags = [tag()];
    let native = serde_json::to_value((&owners, &parameters, &types, &tags)).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let boundaries = CompanionBoundaries::build(
            ctx,
            ctx.admit_iter(&owners, "test admitted owners")
                .unwrap()
                .map(Ok),
            ctx.admit_iter(&parameters, "test admitted parameters")
                .unwrap()
                .map(Ok),
            std::iter::empty(),
            std::iter::empty(),
        )
        .unwrap();
        assert_eq!(boundaries.owner_scopes.get(&10), Some(&13));
        assert_eq!(boundaries.offsets, [10, 20, 30]);
        let registered = segment_types_by_entity(
            ctx,
            ctx.admit_iter(&types, "test admitted types")
                .unwrap()
                .map(Ok),
        )
        .unwrap();
        assert_eq!(
            registered.get(&42),
            Some(&(super::super::DIMENSION_PRESENTATION_V3_TYPE_GUID, 29))
        );
        let mut reference = reference();
        bind_recipe_reference_candidates_from(
            ctx,
            &mut reference,
            ctx.admit_iter(&tags, "test admitted tags").unwrap().map(Ok),
            None,
        )
        .unwrap();
        assert_eq!(
            reference.candidate_faces,
            [FaceId::mint("test:model:face#a").unwrap()]
        );
        assert!(reference.candidate_edges.is_empty());
        assert!(reference.alternate_selector_faces.is_empty());
        assert!(reference.alternate_selector_edges.is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
    assert_eq!(
        serde_json::to_value((&owners, &parameters, &types, &tags)).unwrap(),
        native
    );
}
