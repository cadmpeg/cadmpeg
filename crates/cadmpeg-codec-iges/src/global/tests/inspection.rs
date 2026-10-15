// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::Codec;

use super::{point_file_with_version_flag, valid_global_fields};
use crate::global::{NumericDeclarations, ResolvedGlobal, Supplied, VersionDeclaration};
use crate::test_support::test_curves_and_surfaces::point_file_with_global;
use crate::version::VersionFlag;
use crate::IgesCodec;

fn v5_0_global_without_double_declarations() -> ResolvedGlobal {
    ResolvedGlobal {
        parameter_delimiter: b',',
        record_delimiter: b';',
        sender_product: None,
        receiver_product: None,
        native_file_name: None,
        units_name: None,
        numeric: NumericDeclarations {
            integer_bits: None,
            single_magnitude: None,
            double_magnitude: Supplied::Absent,
            single_significance: None,
            double_significance: Supplied::Absent,
        },
        minimum_resolution: cadmpeg_ir::scalar::NonNegativeReal::ZERO,
        length_factor_mm: None,
        line_weight_scale: None,
        declaration: VersionDeclaration::Exact(VersionFlag::V5_0),
    }
}

#[test]
fn global_summary_refuses_note_slot_and_text_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let (global, _) = super::resolve_global_fields(&valid_global_fields());
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "iges global summary notes",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            global.summary_notes(&ctx).map(|(notes, storage)| {
                drop(notes);
                drop(storage);
            })
        },
    );
    assert!(matches!(
        result,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.used == 0
                && limit.additional == 1
                && limit.operation == "iges global summary notes"
    ));

    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "iges global summary text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            global.summary_notes(&ctx).map(|(notes, storage)| {
                drop(notes);
                drop(storage);
            })
        },
    );
    assert!(matches!(
        result,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 0
                && limit.additional == cadmpeg_core::decode::u64_from_index(b"parameter_delimiter=,".len())
                && limit.operation == "iges global summary text"
    ));

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        global.summary_notes(&ctx).unwrap().0[0],
        "parameter_delimiter=,"
    );
}

#[test]
fn global_summary_slots_are_scoped_and_note_text_is_retained() {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };

    let (global, _) = super::resolve_global_fields(&valid_global_fields());
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let (notes, storage) = global.summary_notes(&ctx).unwrap();
    assert_eq!(notes.len(), 5);
    assert_eq!(notes.capacity(), 8);
    let string_slot_bytes = std::mem::size_of::<String>();
    let slots = u64_from_index(
        notes
            .capacity()
            .checked_mul(string_slot_bytes)
            .expect("summary vector size fits"),
    );
    // Five notes grow String slots from the four-slot minimum to eight. During
    // that reallocation, the old four-slot buffer and new eight-slot buffer
    // coexist, so construction needs space for twelve slots at its peak.
    let growth_overlap = u64_from_index(
        (notes.capacity() / 2)
            .checked_mul(string_slot_bytes)
            .expect("previous summary vector size fits"),
    );
    let materialized_peak = slots
        .checked_add(growth_overlap)
        .expect("summary reallocation peak fits");
    let text_bytes = u64_from_index(
        notes
            .iter()
            .try_fold(0_usize, |total, note| total.checked_add(note.len()))
            .expect("summary text size fits"),
    );
    assert!(!notes.is_empty());
    drop(notes);
    drop(storage);

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized_peak;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = global.summary_notes(&ctx).unwrap();
    let growth_headroom = ctx
        .reserve_scoped(growth_overlap, "test fill global summary growth headroom")
        .expect("returned note slots leave only the derived reallocation headroom");
    let error = ctx
        .reserve_scoped(1, "test after live global summary slots")
        .expect_err("the returned note slots remain live");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.used == materialized_peak
                && limit.additional == 1
                && limit.operation == "test after live global summary slots"
    ));
    drop(notes);
    drop(storage);
    drop(growth_headroom);
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual))
            if matches!(error, cadmpeg_core::CodecError::ResourceLimit(expected)
                if actual == expected)
    ));

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized_peak;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = global.summary_notes(&ctx).unwrap();
    drop(notes);
    drop(storage);
    let released = ctx
        .reserve_scoped(materialized_peak, "test after released global summary slots")
        .expect("dropping the returned vector releases its backing");
    drop(released);
    let retained = ctx
        .charge_retained(1, "test after retained global summary text")
        .expect_err("dropping the slots does not release note text");
    assert!(matches!(
        retained,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == text_bytes
                && limit.additional == 1
                && limit.operation == "test after retained global summary text"
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual))
            if matches!(retained, cadmpeg_core::CodecError::ResourceLimit(expected)
                if actual == expected)
    ));
}

#[test]
fn global_loss_slots_are_scoped_and_loss_text_is_retained() {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use crate::global::{Defect, Resolution, Value, FIELD_RECEIVER_PRODUCT, METADATA_CONSEQUENCE};
    use crate::loss::IgesLossCode;

    let code = IgesLossCode::GlobalMetadataFieldUnusable;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut resolution = Resolution {
        ctx: &ctx,
        values: [Value::Omitted; 26],
        losses: Vec::new(),
        loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
    };
    resolution
        .charge(
            code,
            FIELD_RECEIVER_PRODUCT,
            Defect::Absent,
            METADATA_CONSEQUENCE,
        )
        .unwrap();
    let slots = u64_from_index(
        resolution
            .losses
            .capacity()
            .checked_mul(std::mem::size_of::<cadmpeg_ir::report::loss::LossNote>())
            .expect("loss vector size fits"),
    );
    let text_bytes = u64_from_index(resolution.losses[0].message.len())
        .checked_add(4 + u64_from_index(code.code().len()))
        .expect("loss message storage size fits");
    let crate::global::Resolution {
        losses,
        loss_storage,
        ..
    } = resolution;
    drop(losses);
    drop(loss_storage);

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = slots;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut resolution = Resolution {
        ctx: &ctx,
        values: [Value::Omitted; 26],
        losses: Vec::new(),
        loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
    };
    resolution
        .charge(
            code,
            FIELD_RECEIVER_PRODUCT,
            Defect::Absent,
            METADATA_CONSEQUENCE,
        )
        .unwrap();
    let cadmpeg_core::CodecError::ResourceLimit(first) = ctx
        .reserve_scoped(1, "test after live Global loss slots")
        .expect_err("the loss vector slots remain live")
    else {
        panic!("expected a materialized-byte refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(first.used, slots);
    assert_eq!(first.additional, 1);
    assert_eq!(first.operation, "test after live Global loss slots");
    drop(resolution);
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == first
    ));

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = slots;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut resolution = Resolution {
        ctx: &ctx,
        values: [Value::Omitted; 26],
        losses: Vec::new(),
        loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
    };
    resolution
        .charge(
            code,
            FIELD_RECEIVER_PRODUCT,
            Defect::Absent,
            METADATA_CONSEQUENCE,
        )
        .unwrap();
    drop(resolution);
    let released = ctx
        .reserve_scoped(slots, "test after released Global loss slots")
        .expect("dropping the loss vector and guard releases its backing");
    drop(released);
    let cadmpeg_core::CodecError::ResourceLimit(first) = ctx
        .charge_retained(1, "test after retained Global loss text")
        .expect_err("dropping the loss slots does not release the message")
    else {
        panic!("expected a retained-byte refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.used, text_bytes);
    assert_eq!(first.additional, 1);
    assert_eq!(first.operation, "test after retained Global loss text");
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == first
    ));
}

#[test]
fn conditional_global_loss_slots_are_scoped_and_loss_text_is_retained() {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };
    use crate::loss::IgesLossCode;

    let global = v5_0_global_without_double_declarations();
    let codes = [
        IgesLossCode::GlobalMetadataFieldUnusable,
        IgesLossCode::GlobalSemanticContextSubstituted,
    ];

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let (notes, storage) = global
        .conditional_double_precision_losses(true, &ctx)
        .unwrap();
    assert_eq!(notes.len(), codes.len());
    assert!(notes
        .iter()
        .zip(codes)
        .all(|(note, code)| note.code == code.kind()));
    assert_eq!(
        notes
            .iter()
            .map(|note| note.message.as_str())
            .collect::<Vec<_>>(),
        [
            "IGES Global field 10 (double-precision magnitude) is absent; its value was not transferred",
            "IGES Global field 11 (double-precision significance) is absent; the decoder substituted 17 significant decimal digits from its own specification",
        ]
    );
    let slots = u64_from_index(
        notes
            .capacity()
            .checked_mul(std::mem::size_of::<cadmpeg_ir::report::loss::LossNote>())
            .expect("conditional loss vector size fits"),
    );
    let text_bytes = notes
        .iter()
        .zip(codes)
        .try_fold(0_u64, |total, (note, code)| {
            total
                .checked_add(u64_from_index(note.message.len()))?
                .checked_add(4 + u64_from_index(code.code().len()))
        })
        .expect("conditional loss text size fits");
    drop(notes);
    drop(storage);

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = slots;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = global
        .conditional_double_precision_losses(true, &ctx)
        .unwrap();
    let error = ctx
        .reserve_scoped(1, "test after live conditional Global loss slots")
        .expect_err("the conditional loss vector slots remain live");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.used == slots
                && limit.additional == 1
                && limit.operation == "test after live conditional Global loss slots"
    ));
    drop(notes);
    drop(storage);
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual))
            if matches!(error, cadmpeg_core::CodecError::ResourceLimit(expected)
                if actual == expected)
    ));

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = slots;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = global
        .conditional_double_precision_losses(true, &ctx)
        .unwrap();
    drop(notes);
    drop(storage);
    let released = ctx
        .reserve_scoped(slots, "test after released conditional Global loss slots")
        .expect("dropping the conditional losses releases their vector backing");
    drop(released);
    let retained = ctx
        .charge_retained(1, "test after retained conditional Global loss text")
        .expect_err("dropping the slots does not release conditional loss text");
    assert!(matches!(
        retained,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == text_bytes
                && limit.additional == 1
                && limit.operation == "test after retained conditional Global loss text"
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual))
            if matches!(retained, cadmpeg_core::CodecError::ResourceLimit(expected)
                if actual == expected)
    ));
}

#[test]
fn conditional_global_loss_empty_route_preserves_a_fused_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    let global = v5_0_global_without_double_declarations();

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = global
        .conditional_double_precision_losses(false, &ctx)
        .expect("the empty route only creates a zero-sized guard");
    assert!(notes.is_empty());
    drop(notes);
    drop(storage);
    ctx.finish_session()
        .expect("the empty route uses no materialized storage");

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(first) = ctx
        .charge_work(1, "test fuse before empty conditional Global losses")
        .expect_err("the zero work-unit limit fuses this session")
    else {
        panic!("expected a work refusal");
    };
    assert!(matches!(
        global.conditional_double_precision_losses(false, &ctx),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
}

#[test]
fn inspect_reports_the_resolution_losses_it_charges_as_typed_losses() {
    let mut fields = valid_global_fields();
    fields[11] = "7Hproduct".into();
    fields[16] = String::new();
    fields[22] = "6".into();
    let mut global = fields.join(",");
    global.push(';');

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_global(global.as_bytes())),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"iges_version=4.0".into()));
    assert!(summary
        .losses
        .iter()
        .all(|loss| loss.code != crate::loss::IgesLossCode::SourceDialectUnverified.kind()));
    assert!(summary
        .losses
        .iter()
        .any(|loss| { loss.code == crate::loss::IgesLossCode::LineWeightScaleUnavailable.kind() }));
}

#[test]
fn inspect_distinguishes_an_unknown_declaration_from_its_effective_version() {
    for (flag, version) in [("12", "5.3"), ("0", "2.0")] {
        let summary = IgesCodec
            .inspect(
                &mut Cursor::new(point_file_with_version_flag(flag)),
                &cadmpeg_core::decode::InspectOptions::default(),
            )
            .unwrap();
        assert!(
            summary.notes.contains(&"iges_version=unverified".into()),
            "{flag}: {:#?}",
            summary.notes
        );
        assert!(
            summary
                .notes
                .contains(&format!("iges_declared_version_flag={flag}")),
            "{flag}: {:#?}",
            summary.notes
        );
        assert!(
            summary
                .notes
                .contains(&format!("iges_effective_version={version}")),
            "{flag}: {:#?}",
            summary.notes
        );
        assert_eq!(
            summary
                .dialects()
                .expect("inspection classifies the document")
                .primary()
                .dialect()
                .as_str(),
            "iges:unknown"
        );
    }

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(point_file_with_version_flag("11")),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert!(summary.notes.contains(&"iges_version=5.3".into()));
    assert!(
        !summary
            .notes
            .iter()
            .any(|note| note.starts_with("iges_declared_version_flag=")
                || note.starts_with("iges_effective_version=")),
        "{:#?}",
        summary.notes
    );
}
