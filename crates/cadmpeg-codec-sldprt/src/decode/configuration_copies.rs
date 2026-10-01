// SPDX-License-Identifier: Apache-2.0
//! Charged copies used when a resolved feature updates a configuration snapshot.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::holes::{HoleConstruction, HolePlacement, HoleSpecification};
use cadmpeg_ir::features::{
    FeatureId, GeneratedVertexRef, LinearTermination, SelectionReference, VertexSelection,
};
use cadmpeg_ir::ids::{FeatureInputTopologyId, HistoricalVertexId};

const OPERATION: &str = "copy SLDPRT active configuration hole";

fn copy_text(ctx: &DecodeContext<'_>, source: &str) -> Result<String, CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(source.len())
        .checked_mul(4)
        .ok_or_else(|| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(copy_work, OPERATION)?;
    let mut text = String::new();
    ctx.try_reserve_retained_text(&mut text, source.len(), OPERATION)?;
    text.push_str(source);
    Ok(text)
}

fn copy_nonblank(
    ctx: &DecodeContext<'_>,
    source: &NonBlankString,
) -> Result<NonBlankString, CodecError> {
    NonBlankString::new(copy_text(ctx, source.as_str())?)
        .ok_or_else(|| CodecError::malformed("blank decoded hole specification"))
}

fn copy_selection(
    ctx: &DecodeContext<'_>,
    source: &SelectionReference,
) -> Result<SelectionReference, CodecError> {
    SelectionReference::try_from(copy_text(ctx, source.as_str())?).map_err(CodecError::malformed)
}

pub(super) fn placements(
    ctx: &DecodeContext<'_>,
    source: &[HolePlacement],
) -> Result<Vec<HolePlacement>, CodecError> {
    let mut copied = Vec::new();
    ctx.reserve_vec(&mut copied, source.len(), OPERATION)?;
    for placement in source {
        ctx.charge_work(1, OPERATION)?;
        copied.push(placement.clone());
    }
    Ok(copied)
}

fn specification(
    ctx: &DecodeContext<'_>,
    source: &HoleSpecification,
) -> Result<Box<HoleSpecification>, CodecError> {
    let bytes = u64::try_from(std::mem::size_of::<HoleSpecification>())
        .map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    ctx.charge_retained(bytes, OPERATION)?;
    let copied = match source {
        HoleSpecification::Clearance {
            standard,
            designation,
            fit,
            modeled,
            cosmetic,
            hand,
            depth,
            clearance,
        } => HoleSpecification::Clearance {
            standard: copy_nonblank(ctx, standard)?,
            designation: designation
                .as_ref()
                .map(|value| copy_text(ctx, value))
                .transpose()?,
            fit: fit
                .as_ref()
                .map(|value| copy_text(ctx, value))
                .transpose()?,
            modeled: *modeled,
            cosmetic: *cosmetic,
            hand: *hand,
            depth: *depth,
            clearance: *clearance,
        },
        HoleSpecification::Threaded {
            standard,
            designation,
            class,
            modeled,
            cosmetic,
            pitch,
            major_diameter,
            hand,
            depth,
            clearance,
        } => HoleSpecification::Threaded {
            standard: copy_nonblank(ctx, standard)?,
            designation: designation
                .as_ref()
                .map(|value| copy_text(ctx, value))
                .transpose()?,
            class: class
                .as_ref()
                .map(|value| copy_text(ctx, value))
                .transpose()?,
            modeled: *modeled,
            cosmetic: *cosmetic,
            pitch: *pitch,
            major_diameter: *major_diameter,
            hand: *hand,
            depth: *depth,
            clearance: *clearance,
        },
    };
    Ok(Box::new(copied))
}

pub(super) fn construction(
    ctx: &DecodeContext<'_>,
    source: &HoleConstruction,
) -> Result<HoleConstruction, CodecError> {
    match source {
        HoleConstruction::Form {
            kind,
            specification: source_specification,
        } => Ok(HoleConstruction::Form {
            kind: *kind,
            specification: source_specification
                .as_deref()
                .map(|value| specification(ctx, value))
                .transpose()?,
        }),
        HoleConstruction::NativeThread {
            major_diameter,
            thread_depth,
            pitch,
            drill_point_angle,
        } => Ok(HoleConstruction::NativeThread {
            major_diameter: *major_diameter,
            thread_depth: *thread_depth,
            pitch: *pitch,
            drill_point_angle: *drill_point_angle,
        }),
    }
}

fn vertex(
    ctx: &DecodeContext<'_>,
    source: &VertexSelection,
) -> Result<VertexSelection, CodecError> {
    match source {
        VertexSelection::Unresolved => Ok(VertexSelection::Unresolved),
        VertexSelection::Generated { vertex, native } => {
            let feature = FeatureId::mint(copy_text(ctx, vertex.feature.as_str())?)
                .map_err(CodecError::malformed)?;
            let vertex =
                GeneratedVertexRef::new(feature, copy_text(ctx, vertex.local_id.as_str())?)
                    .map_err(CodecError::malformed)?;
            Ok(VertexSelection::Generated {
                vertex,
                native: copy_selection(ctx, native)?,
            })
        }
        VertexSelection::Historical {
            state,
            vertex,
            native,
        } => {
            let state = FeatureInputTopologyId::mint(copy_text(ctx, state.as_str())?)
                .map_err(CodecError::malformed)?;
            let vertex = HistoricalVertexId::mint(copy_text(ctx, vertex.as_str())?)
                .map_err(CodecError::malformed)?;
            Ok(VertexSelection::Historical {
                state,
                vertex,
                native: copy_nonblank(ctx, native)?,
            })
        }
        VertexSelection::Native(native) => {
            Ok(VertexSelection::Native(copy_selection(ctx, native)?))
        }
    }
}

pub(super) fn termination(
    ctx: &DecodeContext<'_>,
    source: &LinearTermination,
) -> Result<LinearTermination, CodecError> {
    Ok(match source {
        LinearTermination::ToFace { face, offset } => LinearTermination::ToFace {
            face: face.try_clone_charged(ctx, OPERATION)?,
            offset: *offset,
        },
        LinearTermination::ToVertex { vertex: selected } => LinearTermination::ToVertex {
            vertex: vertex(ctx, selected)?,
        },
        LinearTermination::OffsetFromFace { face, offset } => LinearTermination::OffsetFromFace {
            face: face.try_clone_charged(ctx, OPERATION)?,
            offset: *offset,
        },
        LinearTermination::ToShape { target } => LinearTermination::ToShape {
            target: target.try_clone_charged(ctx, OPERATION)?,
        },
        _ => source.clone(),
    })
}
