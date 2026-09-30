// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted copies of feature profiles and termination operands.

use super::{
    copy_feature_selection_text, DistinctMembers, FeatureId, GeneratedCurveRef, GeneratedVertexRef,
    LinearTermination, NativeSelections, NonEmptyMembers, ParameterValue, PlanarProfileRef,
    SelectionMembers, SelectionReference, SketchProfileBoundaryUse, SketchProfileLoops,
    SketchProfileRegion, SketchProfileRegions, VertexSelection,
};
use crate::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId, HistoricalVertexId, IdentityError};
use crate::sketches::{SketchEntityId, SketchId};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

fn copy_identity<T>(
    ctx: &DecodeContext<'_>, text: &str, operation: &'static str,
    mint: impl FnOnce(String) -> Result<T, IdentityError>,
) -> Result<T, CodecError> {
    mint(copy_feature_selection_text(ctx, text, operation)?).map_err(CodecError::malformed)
}

fn copy_sequence<T, U>(
    ctx: &DecodeContext<'_>, values: &[T], operation: &'static str,
    mut copy: impl FnMut(&T) -> Result<U, CodecError>,
) -> Result<Vec<U>, CodecError> {
    let work = std::mem::size_of::<T>().checked_add(std::mem::size_of::<U>())
        .and_then(|size| size.checked_add(1)).and_then(|size| size.checked_mul(values.len()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(u64_from_index(work), operation)?;
    let mut copied = Vec::new();
    ctx.reserve_collection_vec(&mut copied, values.len(), operation)?;
    for value in values { copied.push(copy(value)?); }
    Ok(copied)
}

fn copy_native_selections(
    ctx: &DecodeContext<'_>, selections: &NativeSelections, operation: &'static str,
) -> Result<NativeSelections, CodecError> {
    Ok(NativeSelections(copy_sequence(ctx, selections.as_slice(), operation,
        |text| copy_feature_selection_text(ctx, text, operation))?))
}

fn copy_profile_region(
    ctx: &DecodeContext<'_>, region: &SketchProfileRegion, operation: &'static str,
) -> Result<SketchProfileRegion, CodecError> {
    match region {
        SketchProfileRegion::Loops { loops } => Ok(SketchProfileRegion::Loops {
            loops: SketchProfileLoops {
                outer: loops.outer,
                holes: DistinctMembers(copy_sequence(ctx, loops.holes.as_slice(), operation, |value| Ok(*value))?),
            },
        }),
        SketchProfileRegion::Trimmed { outer_boundary, hole_boundaries } => {
            let mut boundary = |value: &SketchProfileBoundaryUse| Ok(SketchProfileBoundaryUse {
                entity: copy_identity(ctx, value.entity.as_str(), operation, SketchEntityId::mint)?,
                parameter_range: value.parameter_range, reversed: value.reversed,
            });
            let outer_boundary = NonEmptyMembers(copy_sequence(ctx, outer_boundary.as_slice(), operation, &mut boundary)?);
            let hole_boundaries = copy_sequence(ctx, hole_boundaries, operation, |ring| {
                Ok(NonEmptyMembers(copy_sequence(ctx, ring.as_slice(), operation, &mut boundary)?))
            })?;
            Ok(SketchProfileRegion::Trimmed { outer_boundary, hole_boundaries })
        }
    }
}

impl PlanarProfileRef {
    /// Copy each profile identity and nested selection collection through the caller context.
    pub fn try_clone_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), operation)?;
        match self {
            Self::Unresolved(text) => Ok(Self::Unresolved(copy_feature_selection_text(ctx, text, operation)?)),
            Self::Native(text) => Ok(Self::Native(copy_feature_selection_text(ctx, text, operation)?)),
            Self::Sketch(sketch) => Ok(Self::Sketch(copy_identity(ctx, sketch.as_str(), operation, SketchId::mint)?)),
            Self::SketchProfiles { sketch, profiles } => Ok(Self::SketchProfiles {
                sketch: copy_identity(ctx, sketch.as_str(), operation, SketchId::mint)?,
                profiles: SelectionMembers(copy_sequence(ctx, profiles.as_slice(), operation, |value| Ok(*value))?),
            }),
            Self::SketchRegions { sketch, regions } => Ok(Self::SketchRegions {
                sketch: copy_identity(ctx, sketch.as_str(), operation, SketchId::mint)?,
                regions: SketchProfileRegions(copy_sequence(ctx, regions.as_slice(), operation,
                    |region| copy_profile_region(ctx, region, operation))?),
            }),
            Self::SketchEntities { sketch, entities } => Ok(Self::SketchEntities {
                sketch: copy_identity(ctx, sketch.as_str(), operation, SketchId::mint)?,
                entities: SelectionMembers(copy_sequence(ctx, entities.as_slice(), operation,
                    |id| copy_identity(ctx, id.as_str(), operation, SketchEntityId::mint))?),
            }),
            Self::SketchSelection { sketch, selections } => Ok(Self::SketchSelection {
                sketch: copy_identity(ctx, sketch.as_str(), operation, SketchId::mint)?,
                selections: copy_native_selections(ctx, selections, operation)?,
            }),
            Self::HistoricalFaces { state, faces, native } => Ok(Self::HistoricalFaces {
                state: copy_identity(ctx, state.as_str(), operation, FeatureInputTopologyId::mint)?,
                faces: SelectionMembers(copy_sequence(ctx, faces.as_slice(), operation,
                    |id| copy_identity(ctx, id.as_str(), operation, HistoricalFaceId::mint))?),
                native: copy_native_selections(ctx, native, operation)?,
            }),
            Self::Feature(feature) => Ok(Self::Feature(copy_identity(ctx, feature.as_str(), operation, FeatureId::mint)?)),
            Self::Generated { curves, native } => Ok(Self::Generated {
                curves: NonEmptyMembers(copy_sequence(ctx, curves.as_slice(), operation, |curve| Ok(GeneratedCurveRef {
                    feature: copy_identity(ctx, curve.feature.as_str(), operation, FeatureId::mint)?,
                    local_id: SelectionReference(copy_feature_selection_text(ctx, curve.local_id.as_str(), operation)?),
                }))?),
                native: SelectionReference(copy_feature_selection_text(ctx, native.as_str(), operation)?),
            }),
            Self::Faces(faces) => Ok(Self::Faces(copy_sequence(ctx, faces, operation,
                |id| copy_identity(ctx, id.as_str(), operation, FaceId::mint))?)),
        }
    }
}

impl VertexSelection {
    /// Copy retained vertex identities and native text through the caller context.
    pub fn try_clone_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), operation)?;
        match self {
            Self::Unresolved => Ok(Self::Unresolved),
            Self::Native(native) => Ok(Self::Native(SelectionReference(copy_feature_selection_text(ctx, native.as_str(), operation)?))),
            Self::Generated { vertex, native } => Ok(Self::Generated {
                vertex: GeneratedVertexRef {
                    feature: copy_identity(ctx, vertex.feature.as_str(), operation, FeatureId::mint)?,
                    local_id: SelectionReference(copy_feature_selection_text(ctx, vertex.local_id.as_str(), operation)?),
                },
                native: SelectionReference(copy_feature_selection_text(ctx, native.as_str(), operation)?),
            }),
            Self::Historical { state, vertex, native } => Ok(Self::Historical {
                state: copy_identity(ctx, state.as_str(), operation, FeatureInputTopologyId::mint)?,
                vertex: copy_identity(ctx, vertex.as_str(), operation, HistoricalVertexId::mint)?,
                native: NonBlankString::new(copy_feature_selection_text(ctx, native.as_str(), operation)?)
                    .ok_or_else(|| CodecError::malformed("invalid decoded historical vertex reference"))?,
            }),
        }
    }
}

impl LinearTermination {
    /// Copy terminating face and vertex selections after caller admission.
    pub fn try_clone_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), operation)?;
        match self {
            Self::Unresolved {} => Ok(Self::Unresolved {}),
            Self::Blind { length } => Ok(Self::Blind { length: *length }),
            Self::ThroughAll {} => Ok(Self::ThroughAll {}),
            Self::ThroughNext {} => Ok(Self::ThroughNext {}),
            Self::ToFirst {} => Ok(Self::ToFirst {}),
            Self::ToLast {} => Ok(Self::ToLast {}),
            Self::ToFace { face, offset } => Ok(Self::ToFace { face: face.try_clone_charged(ctx, operation)?, offset: *offset }),
            Self::ToVertex { vertex } => Ok(Self::ToVertex { vertex: vertex.try_clone_charged(ctx, operation)? }),
            Self::OffsetFromFace { face, offset } => Ok(Self::OffsetFromFace { face: face.try_clone_charged(ctx, operation)?, offset: *offset }),
            Self::ToShape { target } => Ok(Self::ToShape { target: target.try_clone_charged(ctx, operation)? }),
        }
    }
}

impl ParameterValue {
    /// Copy a scalar value, admitting retained text through the caller context.
    pub fn try_clone_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), operation)?;
        match self {
            Self::Length(value) => Ok(Self::Length(*value)),
            Self::Angle(value) => Ok(Self::Angle(*value)),
            Self::Real(value) => Ok(Self::Real(*value)),
            Self::Integer(value) => Ok(Self::Integer(*value)),
            Self::Boolean(value) => Ok(Self::Boolean(*value)),
            Self::String(value) => Ok(Self::String(copy_feature_selection_text(ctx, value, operation)?)),
        }
    }
}

/// Preserve an admitted feature value while charging every owned child.
pub(super) trait FeatureCopy: Sized {
    fn copy_feature(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError>;
}

mod values;
mod definitions;
mod operands;

impl super::Feature {
    /// Copy the complete feature after caller admission of retained text and collections.
    pub fn try_clone_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        self.copy_feature(ctx, operation)
    }

    /// Copy the feature's evaluated construction state for one configuration.
    pub fn configuration_state_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<super::ConfigurationFeatureState, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<super::ConfigurationFeatureState>()), operation)?;
        Ok(super::ConfigurationFeatureState {
            evaluation: if self.suppressed.unwrap_or(false) { super::ConfigurationEvaluation::Suppressed {} }
                else { super::ConfigurationEvaluation::Active { outputs: self.evaluation.outputs.copy_feature(ctx, operation)? } },
            dependencies: self.dependencies.copy_feature(ctx, operation)?,
            definition: self.evaluation.definition.copy_feature(ctx, operation)?,
        })
    }
}

impl super::ConfigurationFeatureState {
    /// Copy the evaluated state after caller admission of retained text and collections.
    pub fn try_clone_charged(&self, ctx: &DecodeContext<'_>, operation: &'static str) -> Result<Self, CodecError> {
        self.copy_feature(ctx, operation)
    }
}
