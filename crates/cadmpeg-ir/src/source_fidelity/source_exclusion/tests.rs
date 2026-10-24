// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::ids::{Identity, PointId};
use crate::source_fidelity::parts::{DecodeParts, PartOutput, PartSpec};
use cadmpeg_core::decode::{ByteRange, DecodeArena, DecodeDepth, DecodePolicy};
use serde::{Deserialize, Serialize};

const PROPERTY: &[u8] =
    b"<Property name='Length' type='App::PropertyLength'><Float value='NaN'/></Property>";
const FIRST: &str = "test:native:property#first";
const SECOND: &str = "test:native:property#second";

#[derive(Serialize, Deserialize)]
struct Source {
    id: Identity,
}
rewrite_record!(Source, []; {id});

fn part_level(name: &str) -> PartLevel {
    PartLevel::parse(name).unwrap()
}

fn produced(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
) -> (
    crate::native::Native,
    SourceFidelity,
    Vec<crate::report::loss::LossNote>,
) {
    let mut output = PartOutput::new(crate::CodecFormat::Fcstd);
    for (position, id) in [FIRST, SECOND].iter().enumerate() {
        let start = position * PROPERTY.len();
        let property = root.child(start, start + PROPERTY.len()).unwrap();
        let value_start = PROPERTY
            .windows(b"<Float".len())
            .position(|bytes| bytes == b"<Float")
            .unwrap();
        let value = root
            .child(
                start + value_start,
                start + PROPERTY.len() - "</Property>".len(),
            )
            .unwrap();
        ctx.decode_part(
            &mut output,
            property,
            PartSpec::new(part_level("freecad.property"), Vec::new()),
            |stage| {
                ctx.record(id, property, || {
                    let failed: Option<()> = ctx.decode_part(
                        stage,
                        value,
                        PartSpec::new(part_level("freecad.property_value"), Vec::new()),
                        |_| {
                            assert_eq!(
                                value.read_slice(value.start()..value.end()),
                                Some(b"<Float value='NaN'/>".as_slice())
                            );
                            Err(CodecError::malformed("non-finite source Float"))
                        },
                    )?;
                    assert!(failed.is_none());
                    stage.insert_native(
                        ctx,
                        "properties",
                        Source {
                            id: Identity::new((*id).to_owned()).unwrap(),
                        },
                    )
                })
            },
        )
        .unwrap()
        .unwrap();
    }
    let (_, native, fidelity, losses) = output.into_parts(ctx).unwrap().into_parts();
    (native, fidelity, losses)
}

fn assert_parent_layouts(fidelity: &SourceFidelity) {
    for (position, id) in [FIRST, SECOND].iter().enumerate() {
        let layout = &fidelity.record_layout()[*id];
        assert_eq!(layout.stream(), "file");
        assert_eq!(
            layout.offset(),
            u64::try_from(position * PROPERTY.len()).unwrap()
        );
        assert_eq!(layout.byte_len(), u64::try_from(PROPERTY.len()).unwrap());
        assert!(layout.reads().is_empty());
    }
}

fn first<'a>(root: View<'a>) -> View<'a> {
    root.child(root.start(), root.start() + PROPERTY.len())
        .unwrap()
}

#[test]
fn proven_design_link_removal_preserves_failure_facts_and_the_original_source_layout() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, losses) = produced(&ctx, root);
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .unwrap();
    let before = serde_json::to_value(&fidelity).unwrap();
    let before_losses = losses.clone();
    let old = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    let available = ctx.retained_bytes_available();
    fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof])
        .unwrap();
    assert_eq!(ctx.retained_bytes_available(), available);
    native
        .namespace_mut("fcstd")
        .arenas_mut()
        .get_mut("properties")
        .unwrap()
        .retain(|record| record.id() != FIRST);
    let unknowns = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), old.len());
    for (old, current) in old.iter().zip(&unknowns) {
        assert_eq!(current.id, old.id);
        if old.links.iter().any(|link| link.as_str() == FIRST) {
            assert!(current.links.is_empty());
        } else {
            assert_eq!(current, old);
        }
        assert!(
            serde_json::to_vec(current).unwrap().len() <= serde_json::to_vec(old).unwrap().len()
        );
    }
    assert_eq!(serde_json::to_value(&fidelity).unwrap(), before);
    assert_eq!(losses, before_losses);
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
    assert!(fidelity.record_layout().contains_key(FIRST));
    assert!(unknowns
        .iter()
        .flat_map(|unknown| &unknown.links)
        .all(|link| link.as_str() == SECOND));
}

#[test]
fn a_mixed_failure_keeps_its_held_model_and_other_source_references_in_original_order() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    let point = crate::topology::Point::new(
        PointId::mint("test:model:point#held").unwrap(),
        crate::features::FinitePoint3::ZERO,
        None,
    );
    let point_before = point.clone();
    let mut unknowns = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    let affected = unknowns
        .iter_mut()
        .find(|unknown| unknown.links.iter().any(|link| link.as_str() == FIRST))
        .unwrap();
    affected
        .links
        .insert(0, Identity::new(point.id.as_str().to_owned()).unwrap());
    affected
        .links
        .push(Identity::new(SECOND.to_owned()).unwrap());
    let affected_id = affected.id.clone();
    native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "unknowns", &unknowns)
        .unwrap();
    let mut ir = crate::CadIr::empty();
    ir.model.points.push(point);
    ir.native = native;
    ir.set_scope(cadmpeg_core::decode::ContentScope::geometry())
        .unwrap();
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .unwrap();
    let available = ctx.retained_bytes_available();
    fidelity
        .exclude_requested_native_sources(&ctx, ir.native.namespace_mut("fcstd"), &[proof])
        .unwrap();
    assert_eq!(ctx.retained_bytes_available(), available);
    let unknowns = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    let affected = unknowns
        .iter()
        .find(|unknown| unknown.id == affected_id)
        .unwrap();
    assert_eq!(
        affected
            .links
            .iter()
            .map(|link| link.as_str())
            .collect::<Vec<_>>(),
        [point_before.id.as_str(), SECOND]
    );
    assert_eq!(ir.model.points, [point_before.clone()]);
    assert!(SourceRecordExclusion::new(
        &ctx,
        point_before.id.as_str(),
        ArenaClass::Model,
        part_level("freecad.property"),
        first(root)
    )
    .is_err());
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn adjacent_same_named_parent_extent_cannot_authorize_an_exclusion() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    let before = native.clone();
    let adjacent = root.child(PROPERTY.len(), 2 * PROPERTY.len()).unwrap();
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        adjacent,
    )
    .unwrap();
    assert!(fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof])
        .is_err());
    assert_eq!(native, before);
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn identical_bytes_in_another_actual_stream_do_not_match_the_failure_parent() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let foreign = ctx
        .register_named_slice(
            root,
            ByteRange {
                start: 0,
                end: u64::try_from(PROPERTY.len()).unwrap(),
            },
            "other-property",
        )
        .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    let before = native.clone();
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        foreign,
    )
    .unwrap();
    assert!(fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof])
        .is_err());
    assert_eq!(native, before);
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn foreign_session_proof_is_rejected_and_full_request_cannot_mint_one() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let other_arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (other, foreign) = DecodeContext::from_root_bytes_at(
        &bytes,
        &other_arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    // The foreign proof must be genuine and active too: otherwise an inactive
    // source refusal would prevent reaching the cross-session identity guard.
    let (_, mut foreign_fidelity, _) = produced(&other, foreign);
    let before = native.clone();
    let proof = SourceRecordExclusion::new(
        &other,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(foreign),
    )
    .unwrap();
    assert!(fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof])
        .is_err());
    assert_eq!(native, before);
    let full_arena = DecodeArena::new();
    let (full, source) =
        DecodeContext::from_root_bytes(&bytes, &full_arena, &DecodePolicy::service()).unwrap();
    assert!(SourceRecordExclusion::new(
        &full,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(source)
    )
    .is_err());
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
    foreign_fidelity
        .install_record_captures(&other, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&foreign_fidelity);
}

#[test]
fn a_later_bad_parent_proof_installs_no_earlier_staged_replacement() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    let before = native.clone();
    let first = first(root);
    let proofs = [
        SourceRecordExclusion::new(
            &ctx,
            FIRST,
            ArenaClass::Design,
            part_level("freecad.property"),
            first,
        )
        .unwrap(),
        SourceRecordExclusion::new(
            &ctx,
            SECOND,
            ArenaClass::Design,
            part_level("freecad.property"),
            first,
        )
        .unwrap(),
    ];
    assert!(fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &proofs)
        .is_err());
    assert_eq!(native, before);
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn source_exclusion_preserves_the_callers_first_fused_refusal_without_mutation() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .unwrap();
    let before = native.clone();
    assert!(ctx
        .reserve_working(
            &mut Vec::<u64>::new(),
            usize::MAX,
            "source exclusion prior refusal"
        )
        .is_err());
    let refusal = ctx.resource_refusal().unwrap();
    assert!(
        matches!(fidelity.exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof]), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert_eq!(native, before);
    // A Session refusal ends source work; extraction must preserve that exact
    // first refusal as well, rather than clear it to construct final layouts.
    assert!(matches!(
        fidelity.install_record_captures(&ctx, |_, _| Ok(())),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}

#[test]
fn an_actual_typed_source_owner_before_install_closes_only_its_failure_link_without_retained_charge(
) {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, losses) = produced(&ctx, root);
    let owner = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<Source>("properties")
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let owners = native
        .namespace_mut("fcstd")
        .arenas_mut()
        .remove("properties")
        .unwrap();
    assert_eq!(owners[0].id(), owner.id.as_str());
    assert_eq!(owner.id.as_str(), FIRST);
    let before_unknowns = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    let available = ctx.retained_bytes_available();
    let proof = SourceRecordExclusion::from_typed_source(
        &ctx,
        FIRST,
        &owner,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .unwrap();
    let before_fidelity = serde_json::to_value(&fidelity).unwrap();
    assert_eq!(ctx.retained_bytes_available(), available);
    fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof])
        .unwrap();
    assert_eq!(ctx.retained_bytes_available(), available);
    assert!(!native
        .namespace("fcstd")
        .unwrap()
        .arenas()
        .contains_key("properties"));
    assert_eq!(serde_json::to_value(&fidelity).unwrap(), before_fidelity);
    assert_eq!(losses.len(), 2);
    let after = native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    for (before, current) in before_unknowns.iter().zip(&after) {
        let mut expected = before.clone();
        expected.links.retain(|id| id.as_str() != FIRST);
        assert_eq!(*current, expected);
    }
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn no_installed_or_typed_source_owner_is_rejected_without_unknown_mutation() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (mut native, mut fidelity, _) = produced(&ctx, root);
    native
        .namespace_mut("fcstd")
        .arenas_mut()
        .remove("properties")
        .unwrap();
    let before = native.clone();
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .unwrap();
    assert!(fidelity
        .exclude_requested_native_sources(&ctx, native.namespace_mut("fcstd"), &[proof])
        .is_err());
    assert_eq!(native, before);
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn typed_source_owner_identity_mismatch_never_authorizes_a_different_record() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    // Exercise the identity mismatch in the same active, actual source phase
    // as production; an inactive-session refusal would not test this guard.
    let (_, mut fidelity, _) = produced(&ctx, root);
    let owner = Source {
        id: Identity::new(SECOND.to_owned()).unwrap(),
    };
    assert!(SourceRecordExclusion::from_typed_source(
        &ctx,
        FIRST,
        &owner,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .is_err());
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn a_warm_transient_extent_proof_keeps_the_actual_parent_layouts_without_retained_charge() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (_, mut fidelity, _) = produced(&ctx, root);
    let available = ctx.retained_bytes_available();
    let proof = SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root),
    )
    .unwrap();
    assert_eq!(ctx.retained_bytes_available(), available);
    assert_eq!(proof.source.stream(), "file");
    assert_eq!(proof.source.offset(), 0);
    assert_eq!(
        proof.source.byte_len(),
        u64::try_from(PROPERTY.len()).unwrap()
    );
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
}

#[test]
fn a_source_extent_proof_after_capture_extraction_still_rejects_the_closed_phase() {
    let bytes = [PROPERTY, PROPERTY].concat();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes_at(
        &bytes,
        &arena,
        &DecodePolicy::service(),
        DecodeDepth::Geometry,
    )
    .unwrap();
    let (_, mut fidelity, _) = produced(&ctx, root);
    fidelity
        .install_record_captures(&ctx, |_, _| Ok(()))
        .unwrap();
    assert_parent_layouts(&fidelity);
    assert!(SourceRecordExclusion::new(
        &ctx,
        FIRST,
        ArenaClass::Design,
        part_level("freecad.property"),
        first(root)
    )
    .is_err());
}
