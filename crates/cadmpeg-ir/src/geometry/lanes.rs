// SPDX-License-Identifier: Apache-2.0
//! Admission and raw views of the construction records a procedural store
//! holds.
//!
//! A source states each record raw: `f64` scalars, `Vector3` vectors and
//! `Point3` points. A store admits the record once, into the instantiation
//! whose scalars are `FiniteReal`, whose vectors are `FiniteVector3` and whose
//! points are `FinitePoint3`, and holds that instantiation. `admit` is the one
//! walk over a raw record; it is absent when a stored value is not finite.
//! `to_raw` hands the raw values back to a reader that writes or edits them.

use super::{
    CacheContract, CacheFirstCurveForm, CacheFirstCurveParameterization, ClassicLoftProfileData,
    CompoundComponent, CompoundLoftConstruction, CompoundLoftDirection, CompoundLoftScale,
    CompoundLoftScaleMember, CompoundLoftScales, CompoundLoftTail, CurveOffsetDistanceLaw,
    CurveOffsetRange, DeformableCurveData, DeformableSurfaceConstruction, DeformableSurfaceData,
    DeformableSurfaceFrame, DeformableVectorFrame, ExactSpline, G2BlendConstruction,
    G2BlendFirstShape, G2BlendSide, LawExpression, LawFormula, LawSurfaceConstruction,
    LawSurfaceTail, LoftBridgeToken, LoftMemberForm, LoftPath, LoftPathCurve, LoftProfileMember,
    LoftRevisionForm, LoftSection, LoftSectionEntry, LoftSubdata, LoftSubdataRow, LoftSubdataTable,
    NetSurfaceConstruction, OffsetExtension, OffsetSide, ProjectionTail, RevisionCacheForm,
    RevisionSurfaceForm, RevisionSurfaceParameterization, RollingBallConstruction,
    RollingBallJetDerivative, RollingBallJetSite, RollingBallJetStation, RollingBallRadiusSelector,
    RollingBallSide, RollingBallSupportCurve, RollingBallSupportSurface, RollingBallThirdSide,
    ScaledCompoundLoftBranch, ScaledCompoundLoftConstruction, ScaledCompoundLoftShape,
    SkinSurfaceConstruction, SkinSurfaceLayout, SkinSurfaceProfile, SplineSurfaceParameters,
    SpringLayout, SpringPcurve, SpringSupport, SweepRevisionForm, SweepSurfaceConstruction,
    SweepSurfaceLayout, TaperSurfaceKind, VariableBlendCache, VariableBlendConstruction,
    VariableBlendCrossSection, VariableBlendInterpolationPoint, VariableBlendRadii,
    VariableBlendTerminal, VariableBlendValue, VariableBlendValuePayload, VertexBlendBoundary,
    VertexBlendBoundaryGeometry, VertexBlendConstruction, VertexBlendTwists,
    LAW_EXPRESSION_DEPTH_LIMIT,
};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::topology::ParameterInterval;

/// Admit every present pair of an optional array, or none of them.
fn optional_pair(values: Option<[f64; 2]>) -> Option<Option<[FiniteReal; 2]>> {
    values
        .map(FiniteReal::array)
        .map_or(Some(None), |pair| pair.map(Some))
}

/// Admit an optional point and vector frame, or refuse it.
fn optional_frame(
    frame: Option<(Point3, Vector3)>,
) -> Option<Option<(FinitePoint3, FiniteVector3)>> {
    match frame {
        None => Some(None),
        Some((point, vector)) => Some(Some((
            FinitePoint3::new(point)?,
            FiniteVector3::new(vector)?,
        ))),
    }
}

/// Admit an optional vector, or refuse it.
fn optional_vector(vector: Option<Vector3>) -> Option<Option<FiniteVector3>> {
    vector.map_or(Some(None), |vector| FiniteVector3::new(vector).map(Some))
}

/// The raw form of an optional point and vector frame.
fn raw_frame(frame: Option<(FinitePoint3, FiniteVector3)>) -> Option<(Point3, Vector3)> {
    frame.map(|(point, vector)| (point.get(), vector.get()))
}

impl RevisionSurfaceParameterization {
    /// The parameterization with admitted interval bounds.
    pub(super) fn admit(self) -> Option<RevisionSurfaceParameterization<FiniteReal>> {
        Some(RevisionSurfaceParameterization {
            u_interval: FiniteReal::optional(self.u_interval)?,
            v_interval: FiniteReal::optional(self.v_interval)?,
            u_closure: self.u_closure,
            v_closure: self.v_closure,
            u_singularity: self.u_singularity,
            v_singularity: self.v_singularity,
        })
    }
}

impl RevisionSurfaceParameterization<FiniteReal> {
    /// The parameterization with raw interval bounds.
    #[must_use]
    pub fn to_raw(&self) -> RevisionSurfaceParameterization {
        RevisionSurfaceParameterization {
            u_interval: FiniteReal::raw_optional(self.u_interval),
            v_interval: FiniteReal::raw_optional(self.v_interval),
            u_closure: self.u_closure,
            v_closure: self.v_closure,
            u_singularity: self.u_singularity,
            v_singularity: self.v_singularity,
        }
    }
}

impl CacheFirstCurveParameterization {
    /// The parameterization with admitted interval bounds.
    pub(super) fn admit(self) -> Option<CacheFirstCurveParameterization<FiniteReal>> {
        Some(CacheFirstCurveParameterization {
            interval: FiniteReal::optional(self.interval)?,
            closed_form: self.closed_form,
        })
    }
}

impl CacheFirstCurveParameterization<FiniteReal> {
    /// The parameterization with raw interval bounds.
    #[must_use]
    pub fn to_raw(&self) -> CacheFirstCurveParameterization {
        CacheFirstCurveParameterization {
            interval: FiniteReal::raw_optional(self.interval),
            closed_form: self.closed_form,
        }
    }
}

impl<P> RevisionCacheForm<P> {
    /// The form with its parameterization mapped by `map`, absent when `map`
    /// refuses it. A solved cache states its tolerance as a `FitTolerance`,
    /// which the map keeps.
    fn map_parameterization<Q>(
        self,
        map: impl FnOnce(P) -> Option<Q>,
    ) -> Option<RevisionCacheForm<Q>> {
        Some(match self {
            Self::SolvedCache { fit_tolerance } => RevisionCacheForm::SolvedCache { fit_tolerance },
            Self::Parameterization(parameterization) => {
                RevisionCacheForm::Parameterization(map(parameterization)?)
            }
        })
    }
}

impl RevisionCacheForm {
    /// The form with an admitted parameterization.
    pub(super) fn admit(
        self,
    ) -> Option<RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>> {
        self.map_parameterization(RevisionSurfaceParameterization::admit)
    }
}

impl RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>> {
    /// The form with a raw parameterization.
    #[must_use]
    pub fn to_raw(&self) -> RevisionCacheForm {
        match self {
            Self::SolvedCache { fit_tolerance } => RevisionCacheForm::SolvedCache {
                fit_tolerance: *fit_tolerance,
            },
            Self::Parameterization(parameterization) => {
                RevisionCacheForm::Parameterization(parameterization.to_raw())
            }
        }
    }
}

impl RevisionCacheForm<CacheFirstCurveParameterization> {
    /// The form with an admitted parameterization.
    pub(super) fn admit(
        self,
    ) -> Option<RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>> {
        self.map_parameterization(CacheFirstCurveParameterization::admit)
    }
}

impl RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>> {
    /// The form with a raw parameterization.
    #[must_use]
    pub fn to_raw(&self) -> RevisionCacheForm<CacheFirstCurveParameterization> {
        match self {
            Self::SolvedCache { fit_tolerance } => RevisionCacheForm::SolvedCache {
                fit_tolerance: *fit_tolerance,
            },
            Self::Parameterization(parameterization) => {
                RevisionCacheForm::Parameterization(parameterization.to_raw())
            }
        }
    }
}

impl<F: Default> RevisionSurfaceForm<F> {
    /// The form with admitted bounds, endpoints, cache and discontinuities.
    pub(super) fn admit(self) -> Option<RevisionSurfaceForm<F, FiniteReal>> {
        Some(RevisionSurfaceForm {
            revision: self.revision,
            support_bounds: FiniteReal::optional(self.support_bounds)?,
            reference_endpoints: FiniteReal::optional(self.reference_endpoints)?,
            second_endpoints: FiniteReal::optional(self.second_endpoints)?,
            flags: self.flags,
            cache: self.cache.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            tail_flag: self.tail_flag,
            trailing_flags: self.trailing_flags,
        })
    }
}

impl<F: Default + Clone> RevisionSurfaceForm<F, FiniteReal> {
    /// The form with raw bounds, endpoints, cache and discontinuities.
    #[must_use]
    pub fn to_raw(&self) -> RevisionSurfaceForm<F> {
        RevisionSurfaceForm {
            revision: self.revision,
            support_bounds: FiniteReal::raw_optional(self.support_bounds),
            reference_endpoints: FiniteReal::raw_optional(self.reference_endpoints),
            second_endpoints: FiniteReal::raw_optional(self.second_endpoints),
            flags: self.flags.clone(),
            cache: self.cache.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            tail_flag: self.tail_flag,
            trailing_flags: self.trailing_flags.clone(),
        }
    }
}

impl VariableBlendCache {
    /// The cache with an admitted parameterization.
    pub(super) fn admit(self) -> Option<VariableBlendCache<FiniteReal>> {
        Some(match self {
            Self::Current {
                shape_prefix,
                fit_tolerance,
            } => VariableBlendCache::Current {
                shape_prefix,
                fit_tolerance,
            },
            Self::Stale {} => VariableBlendCache::Stale {},
            Self::Parameterization {
                shape_prefix,
                parameterization,
            } => VariableBlendCache::Parameterization {
                shape_prefix,
                parameterization: parameterization.admit()?,
            },
        })
    }
}

impl VariableBlendCache<FiniteReal> {
    /// The cache with a raw parameterization.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendCache {
        match self {
            Self::Current {
                shape_prefix,
                fit_tolerance,
            } => VariableBlendCache::Current {
                shape_prefix: *shape_prefix,
                fit_tolerance: *fit_tolerance,
            },
            Self::Stale {} => VariableBlendCache::Stale {},
            Self::Parameterization {
                shape_prefix,
                parameterization,
            } => VariableBlendCache::Parameterization {
                shape_prefix: *shape_prefix,
                parameterization: parameterization.to_raw(),
            },
        }
    }
}

impl OffsetExtension {
    /// The extension with an admitted revision-gated form.
    pub(super) fn admit(self) -> Option<OffsetExtension<FiniteReal>> {
        Some(match self {
            Self::Legacy { flags, cache } => OffsetExtension::Legacy { flags, cache },
            Self::Revision { form } => OffsetExtension::Revision {
                form: form.admit()?,
            },
        })
    }
}

impl OffsetExtension<FiniteReal> {
    /// The extension with a raw revision-gated form.
    #[must_use]
    pub fn to_raw(&self) -> OffsetExtension {
        match self {
            Self::Legacy { flags, cache } => OffsetExtension::Legacy {
                flags: *flags,
                cache: *cache,
            },
            Self::Revision { form } => OffsetExtension::Revision {
                form: form.to_raw(),
            },
        }
    }
}

impl SplineSurfaceParameters {
    /// The parameters with admitted intervals.
    pub(super) fn admit(self) -> Option<SplineSurfaceParameters<FiniteReal>> {
        Some(match self {
            Self::OrderedRanges { ranges } => SplineSurfaceParameters::OrderedRanges {
                ranges: FiniteReal::grid(ranges)?,
            },
            Self::RevisionRanges { intervals } => SplineSurfaceParameters::RevisionRanges {
                intervals: FiniteReal::optional_grid(intervals)?,
            },
        })
    }
}

impl SplineSurfaceParameters<FiniteReal> {
    /// The parameters with raw intervals.
    #[must_use]
    pub fn to_raw(&self) -> SplineSurfaceParameters {
        match self {
            Self::OrderedRanges { ranges } => SplineSurfaceParameters::OrderedRanges {
                ranges: FiniteReal::raw_grid(*ranges),
            },
            Self::RevisionRanges { intervals } => SplineSurfaceParameters::RevisionRanges {
                intervals: FiniteReal::raw_optional_grid(*intervals),
            },
        }
    }
}

impl ExactSpline {
    /// The spline with admitted ranges, intervals and form.
    pub(super) fn admit(self) -> Option<ExactSpline<FiniteReal>> {
        Some(match self {
            Self::Legacy {
                ranges,
                extension,
                cache,
            } => ExactSpline::Legacy {
                ranges: FiniteReal::grid(ranges)?,
                extension,
                cache,
            },
            Self::Revision {
                intervals,
                extension,
                form,
            } => ExactSpline::Revision {
                intervals: FiniteReal::optional_grid(intervals)?,
                extension,
                form: form.admit()?,
            },
        })
    }
}

impl ExactSpline<FiniteReal> {
    /// The spline with raw ranges, intervals and form.
    #[must_use]
    pub fn to_raw(&self) -> ExactSpline {
        match self {
            Self::Legacy {
                ranges,
                extension,
                cache,
            } => ExactSpline::Legacy {
                ranges: FiniteReal::raw_grid(*ranges),
                extension: *extension,
                cache: *cache,
            },
            Self::Revision {
                intervals,
                extension,
                form,
            } => ExactSpline::Revision {
                intervals: FiniteReal::raw_optional_grid(*intervals),
                extension: *extension,
                form: form.to_raw(),
            },
        }
    }
}

impl<T> CompoundComponent<T> {
    /// The component with an admitted parameter.
    pub(super) fn admit(self) -> Option<CompoundComponent<T, FiniteReal>> {
        Some(CompoundComponent {
            parameter: FiniteReal::new(self.parameter)?,
            component: self.component,
        })
    }
}

impl<T: Clone> CompoundComponent<T, FiniteReal> {
    /// The component with a raw parameter.
    #[must_use]
    pub fn to_raw(&self) -> CompoundComponent<T> {
        CompoundComponent {
            parameter: self.parameter.get(),
            component: self.component.clone(),
        }
    }
}

impl DeformableVectorFrame {
    /// The frame with admitted vectors and scalar.
    fn admit(self) -> Option<DeformableVectorFrame<FiniteReal, FiniteVector3>> {
        Some(DeformableVectorFrame {
            vectors: FiniteVector3::array(self.vectors)?,
            parameter: FiniteReal::new(self.parameter)?,
            flags: self.flags,
        })
    }
}

impl DeformableVectorFrame<FiniteReal, FiniteVector3> {
    /// The frame with raw vectors and scalar.
    #[must_use]
    pub fn to_raw(&self) -> DeformableVectorFrame {
        DeformableVectorFrame {
            vectors: FiniteVector3::raw_array(self.vectors),
            parameter: self.parameter.get(),
            flags: self.flags,
        }
    }
}

impl DeformableSurfaceFrame {
    /// The frame with admitted vectors, scalars and point.
    fn admit(self) -> Option<DeformableSurfaceFrame<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(DeformableSurfaceFrame {
            leading_vectors: FiniteVector3::array(self.leading_vectors)?,
            leading_parameter: FiniteReal::new(self.leading_parameter)?,
            leading_flags: self.leading_flags,
            secondary_vectors: FiniteVector3::array(self.secondary_vectors)?,
            secondary_parameter: FiniteReal::new(self.secondary_parameter)?,
            secondary_flags: self.secondary_flags,
            point: FinitePoint3::new(self.point)?,
            trailing_flags: self.trailing_flags,
        })
    }
}

impl DeformableSurfaceFrame<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The frame with raw vectors, scalars and point.
    #[must_use]
    pub fn to_raw(&self) -> DeformableSurfaceFrame {
        DeformableSurfaceFrame {
            leading_vectors: FiniteVector3::raw_array(self.leading_vectors),
            leading_parameter: self.leading_parameter.get(),
            leading_flags: self.leading_flags,
            secondary_vectors: FiniteVector3::raw_array(self.secondary_vectors),
            secondary_parameter: self.secondary_parameter.get(),
            secondary_flags: self.secondary_flags,
            point: self.point.get(),
            trailing_flags: self.trailing_flags,
        }
    }
}

impl DeformableSurfaceData {
    /// The payload with admitted vectors, scalars and points.
    fn admit(self) -> Option<DeformableSurfaceData<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::Full {
                leading_vectors,
                leading_parameter,
                leading_flags,
                selector,
                surface,
                native_id,
                flag,
                first_parameter,
                version_value,
                second_parameter,
                curve,
                frames,
                trailing_value,
            } => {
                let [first, second] = *frames;
                DeformableSurfaceData::Full {
                    leading_vectors: FiniteVector3::array(leading_vectors)?,
                    leading_parameter: FiniteReal::new(leading_parameter)?,
                    leading_flags,
                    selector,
                    surface,
                    native_id,
                    flag,
                    first_parameter: FiniteReal::new(first_parameter)?,
                    version_value,
                    second_parameter: FiniteReal::new(second_parameter)?,
                    curve,
                    frames: Box::new([first.admit()?, second.admit()?]),
                    trailing_value,
                }
            }
            Self::SurfaceCurve {
                surface,
                native_id,
                flag,
                first_parameter,
                selector,
                second_parameter,
                curve,
                vectors,
                frame_parameter,
                flags,
                parameter_triples,
            } => DeformableSurfaceData::SurfaceCurve {
                surface,
                native_id,
                flag,
                first_parameter: FiniteReal::new(first_parameter)?,
                selector,
                second_parameter: FiniteReal::new(second_parameter)?,
                curve,
                vectors: FiniteVector3::array(vectors)?,
                frame_parameter: FiniteReal::new(frame_parameter)?,
                flags,
                parameter_triples: FiniteReal::rows(parameter_triples)?,
            },
            Self::Plain {
                frame,
                parameter_triples,
            } => DeformableSurfaceData::Plain {
                frame: Box::new(frame.admit()?),
                parameter_triples: FiniteReal::rows(parameter_triples)?,
            },
            Self::Guided {
                frame,
                selector,
                guide_parameter,
            } => DeformableSurfaceData::Guided {
                frame: Box::new(frame.admit()?),
                selector,
                guide_parameter: FiniteReal::new(guide_parameter)?,
            },
            Self::Minimal { vectors, selector } => DeformableSurfaceData::Minimal {
                vectors: FiniteVector3::array(vectors)?,
                selector,
            },
            Self::RevisionMode3 {
                leading_vectors,
                leading_parameter,
                leading_flags,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                frame_flags,
                parameters,
                trailing_flags,
                trailing_parameter,
                trailing_value,
            } => DeformableSurfaceData::RevisionMode3 {
                leading_vectors: FiniteVector3::array(leading_vectors)?,
                leading_parameter: FiniteReal::new(leading_parameter)?,
                leading_flags,
                trailing_point: FinitePoint3::new(trailing_point)?,
                trailing_vectors: FiniteVector3::array(trailing_vectors)?,
                frame_parameter: FiniteReal::new(frame_parameter)?,
                frame_flags,
                parameters: FiniteReal::array(parameters)?,
                trailing_flags,
                trailing_parameter: FiniteReal::new(trailing_parameter)?,
                trailing_value,
            },
        })
    }
}

impl DeformableSurfaceData<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The payload with raw vectors, scalars and points.
    #[must_use]
    pub fn to_raw(&self) -> DeformableSurfaceData {
        match self {
            Self::Full {
                leading_vectors,
                leading_parameter,
                leading_flags,
                selector,
                surface,
                native_id,
                flag,
                first_parameter,
                version_value,
                second_parameter,
                curve,
                frames,
                trailing_value,
            } => DeformableSurfaceData::Full {
                leading_vectors: FiniteVector3::raw_array(*leading_vectors),
                leading_parameter: leading_parameter.get(),
                leading_flags: *leading_flags,
                selector: *selector,
                surface: surface.clone(),
                native_id: *native_id,
                flag: *flag,
                first_parameter: first_parameter.get(),
                version_value: *version_value,
                second_parameter: second_parameter.get(),
                curve: curve.clone(),
                frames: Box::new([frames[0].to_raw(), frames[1].to_raw()]),
                trailing_value: *trailing_value,
            },
            Self::SurfaceCurve {
                surface,
                native_id,
                flag,
                first_parameter,
                selector,
                second_parameter,
                curve,
                vectors,
                frame_parameter,
                flags,
                parameter_triples,
            } => DeformableSurfaceData::SurfaceCurve {
                surface: surface.clone(),
                native_id: *native_id,
                flag: *flag,
                first_parameter: first_parameter.get(),
                selector: *selector,
                second_parameter: second_parameter.get(),
                curve: curve.clone(),
                vectors: FiniteVector3::raw_array(*vectors),
                frame_parameter: frame_parameter.get(),
                flags: *flags,
                parameter_triples: FiniteReal::raw_rows(parameter_triples),
            },
            Self::Plain {
                frame,
                parameter_triples,
            } => DeformableSurfaceData::Plain {
                frame: Box::new(frame.to_raw()),
                parameter_triples: FiniteReal::raw_rows(parameter_triples),
            },
            Self::Guided {
                frame,
                selector,
                guide_parameter,
            } => DeformableSurfaceData::Guided {
                frame: Box::new(frame.to_raw()),
                selector: *selector,
                guide_parameter: guide_parameter.get(),
            },
            Self::Minimal { vectors, selector } => DeformableSurfaceData::Minimal {
                vectors: FiniteVector3::raw_array(*vectors),
                selector: *selector,
            },
            Self::RevisionMode3 {
                leading_vectors,
                leading_parameter,
                leading_flags,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                frame_flags,
                parameters,
                trailing_flags,
                trailing_parameter,
                trailing_value,
            } => DeformableSurfaceData::RevisionMode3 {
                leading_vectors: FiniteVector3::raw_array(*leading_vectors),
                leading_parameter: leading_parameter.get(),
                leading_flags: *leading_flags,
                trailing_point: trailing_point.get(),
                trailing_vectors: FiniteVector3::raw_array(*trailing_vectors),
                frame_parameter: frame_parameter.get(),
                frame_flags: *frame_flags,
                parameters: FiniteReal::raw_array(*parameters),
                trailing_flags: *trailing_flags,
                trailing_parameter: trailing_parameter.get(),
                trailing_value: *trailing_value,
            },
        }
    }
}

impl DeformableSurfaceConstruction {
    /// The construction with an admitted payload, cache and discontinuities.
    pub(super) fn admit(
        self,
    ) -> Option<DeformableSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(DeformableSurfaceConstruction {
            support: self.support,
            data: self.data.admit()?,
            cache: self.cache.admit_form(RevisionSurfaceForm::admit)?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            discontinuity_flag: self.discontinuity_flag,
        })
    }
}

impl DeformableSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with a raw payload, cache and discontinuities.
    #[must_use]
    pub fn to_raw(&self) -> DeformableSurfaceConstruction {
        DeformableSurfaceConstruction {
            support: self.support.clone(),
            data: self.data.to_raw(),
            cache: self.cache.view_form(RevisionSurfaceForm::to_raw),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            discontinuity_flag: self.discontinuity_flag,
        }
    }
}

impl<F> CacheContract<F> {
    /// The contract with its revision-gated form admitted by `admit`, absent
    /// when `admit` refuses the form. A legacy layout states its tolerance as
    /// a `FitTolerance`, which the admission keeps.
    pub(super) fn admit_form<G>(
        self,
        admit: impl FnOnce(F) -> Option<G>,
    ) -> Option<CacheContract<G>> {
        Some(match self {
            Self::Legacy { cache } => CacheContract::Legacy { cache },
            Self::Revision { form } => CacheContract::Revision { form: admit(form)? },
        })
    }

    /// The contract with its revision-gated form viewed through `view`.
    pub(crate) fn view_form<G>(&self, view: impl FnOnce(&F) -> G) -> CacheContract<G> {
        match self {
            Self::Legacy { cache } => CacheContract::Legacy { cache: *cache },
            Self::Revision { form } => CacheContract::Revision { form: view(form) },
        }
    }
}

impl RollingBallJetDerivative {
    /// The derivative row with admitted vectors and angle.
    fn admit(self) -> Option<RollingBallJetDerivative<FiniteReal, FiniteVector3>> {
        let [first_limit, second_limit, center] =
            FiniteVector3::array([self.first_limit, self.second_limit, self.center])?;
        Some(RollingBallJetDerivative {
            first_limit,
            second_limit,
            center,
            angle: FiniteReal::new(self.angle)?,
        })
    }
}

impl RollingBallJetDerivative<FiniteReal, FiniteVector3> {
    /// The derivative row with raw vectors and angle.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallJetDerivative {
        RollingBallJetDerivative {
            first_limit: self.first_limit.get(),
            second_limit: self.second_limit.get(),
            center: self.center.get(),
            angle: self.angle.get(),
        }
    }
}

impl RollingBallJetSite {
    /// The site with admitted points, angle and derivatives. The values are
    /// refused before the first derivative, and each derivative in order.
    pub(super) fn admit(
        self,
    ) -> Result<RollingBallJetSite<FiniteReal, FiniteVector3, FinitePoint3>, &'static str> {
        const VALUES: &str = "rolling-ball jet site coordinates and angle must be finite";
        const DERIVATIVES: &str = "rolling-ball jet site derivatives must be finite";
        let [first_limit, second_limit, center] =
            FinitePoint3::array([self.first_limit, self.second_limit, self.center])
                .ok_or(VALUES)?;
        let angle = FiniteReal::new(self.angle).ok_or(VALUES)?;
        Ok(RollingBallJetSite {
            first_limit,
            second_limit,
            center,
            angle,
            first_derivative: self.first_derivative.admit().ok_or(DERIVATIVES)?,
            second_derivative: self.second_derivative.admit().ok_or(DERIVATIVES)?,
        })
    }
}

impl RollingBallJetSite<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The site with raw points, angle and derivatives.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallJetSite {
        RollingBallJetSite {
            first_limit: self.first_limit.get(),
            second_limit: self.second_limit.get(),
            center: self.center.get(),
            angle: self.angle.get(),
            first_derivative: self.first_derivative.to_raw(),
            second_derivative: self.second_derivative.to_raw(),
        }
    }
}

impl RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The station with a raw knot and site.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallJetStation {
        RollingBallJetStation {
            knot: self.knot.get(),
            multiplicity: self.multiplicity,
            site: self.site.to_raw(),
        }
    }
}

impl TaperSurfaceKind {
    /// The tail with an admitted draft vector and scalars.
    pub(super) fn admit(self) -> Option<TaperSurfaceKind<FiniteReal, FiniteVector3>> {
        Some(match self {
            Self::Standard {} => TaperSurfaceKind::Standard {},
            Self::Orthogonal { sense } => TaperSurfaceKind::Orthogonal { sense },
            Self::Edge { draft } => TaperSurfaceKind::Edge {
                draft: FiniteVector3::new(draft)?,
            },
            Self::Shadow {
                draft,
                sine,
                cosine,
            } => TaperSurfaceKind::Shadow {
                draft: FiniteVector3::new(draft)?,
                sine: FiniteReal::new(sine)?,
                cosine: FiniteReal::new(cosine)?,
            },
            Self::Ruled {
                draft,
                sine,
                cosine,
                factor,
            } => TaperSurfaceKind::Ruled {
                draft: FiniteVector3::new(draft)?,
                sine: FiniteReal::new(sine)?,
                cosine: FiniteReal::new(cosine)?,
                factor: FiniteReal::new(factor)?,
            },
            Self::Swept {
                draft,
                sine,
                cosine,
            } => TaperSurfaceKind::Swept {
                draft: FiniteVector3::new(draft)?,
                sine: FiniteReal::new(sine)?,
                cosine: FiniteReal::new(cosine)?,
            },
        })
    }
}

impl TaperSurfaceKind<FiniteReal, FiniteVector3> {
    /// The tail with a raw draft vector and scalars.
    #[must_use]
    pub fn to_raw(&self) -> TaperSurfaceKind {
        match self {
            Self::Standard {} => TaperSurfaceKind::Standard {},
            Self::Orthogonal { sense } => TaperSurfaceKind::Orthogonal { sense: *sense },
            Self::Edge { draft } => TaperSurfaceKind::Edge { draft: draft.get() },
            Self::Shadow {
                draft,
                sine,
                cosine,
            } => TaperSurfaceKind::Shadow {
                draft: draft.get(),
                sine: sine.get(),
                cosine: cosine.get(),
            },
            Self::Ruled {
                draft,
                sine,
                cosine,
                factor,
            } => TaperSurfaceKind::Ruled {
                draft: draft.get(),
                sine: sine.get(),
                cosine: cosine.get(),
                factor: factor.get(),
            },
            Self::Swept {
                draft,
                sine,
                cosine,
            } => TaperSurfaceKind::Swept {
                draft: draft.get(),
                sine: sine.get(),
                cosine: cosine.get(),
            },
        }
    }
}

impl LoftSubdataRow {
    /// The row with admitted scalar pairs.
    fn admit(self) -> Option<LoftSubdataRow<FiniteReal>> {
        Some(LoftSubdataRow {
            parameters: FiniteReal::array(self.parameters)?,
            columns: FiniteReal::rows(self.columns)?,
            extra: optional_pair(self.extra)?,
        })
    }
}

impl LoftSubdataRow<FiniteReal> {
    /// The row with raw scalar pairs.
    #[must_use]
    pub fn to_raw(&self) -> LoftSubdataRow {
        LoftSubdataRow {
            parameters: FiniteReal::raw_array(self.parameters),
            columns: FiniteReal::raw_rows(&self.columns),
            extra: self.extra.map(FiniteReal::raw_array),
        }
    }
}

impl LoftSubdataTable {
    /// The table with admitted rows. Admission keeps every row, so the rows
    /// keep their shared column width.
    fn admit(self) -> Option<LoftSubdataTable<FiniteReal>> {
        Some(LoftSubdataTable {
            type_code: self.type_code,
            rows: self
                .rows
                .into_iter()
                .map(LoftSubdataRow::admit)
                .collect::<Option<Vec<_>>>()?,
        })
    }
}

impl LoftSubdataTable<FiniteReal> {
    /// The table with raw rows.
    #[must_use]
    pub fn to_raw(&self) -> LoftSubdataTable {
        LoftSubdataTable {
            type_code: self.type_code,
            rows: self.rows.iter().map(LoftSubdataRow::to_raw).collect(),
        }
    }
}

impl LoftSubdata {
    /// The subdata with admitted rows.
    pub(super) fn admit(self) -> Option<LoftSubdata<FiniteReal>> {
        Some(match self {
            Self::Type211 { dimensions, row } => LoftSubdata::Type211 {
                dimensions,
                row: FiniteReal::array(row)?,
            },
            Self::Table(table) => LoftSubdata::Table(table.admit()?),
        })
    }
}

impl LoftSubdata<FiniteReal> {
    /// The subdata with raw rows.
    #[must_use]
    pub fn to_raw(&self) -> LoftSubdata {
        match self {
            Self::Type211 { dimensions, row } => LoftSubdata::Type211 {
                dimensions: *dimensions,
                row: FiniteReal::raw_array(*row),
            },
            Self::Table(table) => LoftSubdata::Table(table.to_raw()),
        }
    }
}

impl ClassicLoftProfileData {
    /// The profile data with admitted subdata and direction.
    pub(super) fn admit(self) -> Option<ClassicLoftProfileData<FiniteReal, FiniteVector3>> {
        Some(ClassicLoftProfileData {
            surface: self.surface,
            pcurve: self.pcurve,
            first_flag: self.first_flag,
            asm_extension: self.asm_extension,
            subdata: self.subdata.admit()?,
            direction: optional_vector(self.direction)?,
        })
    }
}

impl ClassicLoftProfileData<FiniteReal, FiniteVector3> {
    /// The profile data with raw subdata and direction.
    #[must_use]
    pub fn to_raw(&self) -> ClassicLoftProfileData {
        ClassicLoftProfileData {
            surface: self.surface.clone(),
            pcurve: self.pcurve.clone(),
            first_flag: self.first_flag,
            asm_extension: self.asm_extension,
            subdata: self.subdata.to_raw(),
            direction: self.direction.map(FiniteVector3::get),
        }
    }
}

impl LoftMemberForm {
    /// The form with admitted bounds, subdata and direction.
    fn admit(self) -> Option<LoftMemberForm<FiniteReal, FiniteVector3>> {
        Some(match self {
            Self::Support {
                type_code,
                surface,
                support_bounds,
                pcurve,
                first_flag,
                asm_extension,
                subdata,
                direction,
            } => LoftMemberForm::Support {
                type_code,
                surface,
                support_bounds: FiniteReal::optional(support_bounds)?,
                pcurve,
                first_flag,
                asm_extension,
                subdata: subdata.admit()?,
                direction: optional_vector(direction)?,
            },
            Self::PcurvePair {
                pcurve,
                secondary_pcurve,
                asm_extension,
                subdata,
                direction,
            } => LoftMemberForm::PcurvePair {
                pcurve,
                secondary_pcurve,
                asm_extension,
                subdata: subdata.admit()?,
                direction: optional_vector(direction)?,
            },
        })
    }
}

impl LoftMemberForm<FiniteReal, FiniteVector3> {
    /// The form with raw bounds, subdata and direction.
    #[must_use]
    pub fn to_raw(&self) -> LoftMemberForm {
        match self {
            Self::Support {
                type_code,
                surface,
                support_bounds,
                pcurve,
                first_flag,
                asm_extension,
                subdata,
                direction,
            } => LoftMemberForm::Support {
                type_code: *type_code,
                surface: surface.clone(),
                support_bounds: FiniteReal::raw_optional(*support_bounds),
                pcurve: pcurve.clone(),
                first_flag: *first_flag,
                asm_extension: *asm_extension,
                subdata: subdata.to_raw(),
                direction: direction.map(FiniteVector3::get),
            },
            Self::PcurvePair {
                pcurve,
                secondary_pcurve,
                asm_extension,
                subdata,
                direction,
            } => LoftMemberForm::PcurvePair {
                pcurve: pcurve.clone(),
                secondary_pcurve: secondary_pcurve.clone(),
                asm_extension: *asm_extension,
                subdata: subdata.to_raw(),
                direction: direction.map(FiniteVector3::get),
            },
        }
    }
}

impl LoftPathCurve {
    /// The curve with admitted endpoints.
    pub(super) fn admit(self) -> Option<LoftPathCurve<FiniteReal>> {
        Some(LoftPathCurve {
            id: self.id,
            endpoints: match self.endpoints {
                None => None,
                Some(endpoints) => Some(FiniteReal::optional(endpoints)?),
            },
        })
    }
}

impl LoftPathCurve<FiniteReal> {
    /// The curve with raw endpoints.
    #[must_use]
    pub fn to_raw(&self) -> LoftPathCurve {
        LoftPathCurve {
            id: self.id.clone(),
            endpoints: self.endpoints.map(FiniteReal::raw_optional),
        }
    }
}

impl LoftProfileMember {
    /// The member with an admitted curve and form.
    pub(super) fn admit(self) -> Option<LoftProfileMember<FiniteReal, FiniteVector3>> {
        Some(LoftProfileMember {
            profile: self.profile.admit()?,
            form: self.form.admit()?,
        })
    }
}

impl LoftProfileMember<FiniteReal, FiniteVector3> {
    /// The member with a raw curve and form.
    #[must_use]
    pub fn to_raw(&self) -> LoftProfileMember {
        LoftProfileMember {
            profile: self.profile.to_raw(),
            form: self.form.to_raw(),
        }
    }
}

impl LoftPath {
    /// The path with an admitted curve.
    pub(super) fn admit(self) -> Option<LoftPath<FiniteReal>> {
        Some(LoftPath {
            path: match self.path {
                None => None,
                Some(path) => Some(path.admit()?),
            },
            auxiliaries: self.auxiliaries,
            flag: self.flag,
        })
    }
}

impl LoftPath<FiniteReal> {
    /// The path with a raw curve.
    #[must_use]
    pub fn to_raw(&self) -> LoftPath {
        LoftPath {
            path: self.path.as_ref().map(LoftPathCurve::to_raw),
            auxiliaries: self.auxiliaries.clone(),
            flag: self.flag,
        }
    }
}

impl LoftSectionEntry {
    /// The entry with an admitted parameter, members and path.
    pub(super) fn admit(self) -> Option<LoftSectionEntry<FiniteReal, FiniteVector3>> {
        Some(LoftSectionEntry {
            parameter: FiniteReal::new(self.parameter)?,
            profile: self
                .profile
                .into_iter()
                .map(LoftProfileMember::admit)
                .collect::<Option<Vec<_>>>()?,
            path: self.path.admit()?,
        })
    }
}

impl LoftSectionEntry<FiniteReal, FiniteVector3> {
    /// The entry with a raw parameter, members and path.
    #[must_use]
    pub fn to_raw(&self) -> LoftSectionEntry {
        LoftSectionEntry {
            parameter: self.parameter.get(),
            profile: self.profile.iter().map(LoftProfileMember::to_raw).collect(),
            path: self.path.to_raw(),
        }
    }
}

impl LoftRevisionForm {
    /// The form with an admitted cache and discontinuities.
    pub(super) fn admit(self) -> Option<LoftRevisionForm<FiniteReal>> {
        Some(LoftRevisionForm {
            revision: self.revision,
            flags: self.flags,
            ints: self.ints,
            cache: self.cache.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            tail_flag: self.tail_flag,
        })
    }
}

impl LoftRevisionForm<FiniteReal> {
    /// The form with a raw cache and discontinuities.
    #[must_use]
    pub fn to_raw(&self) -> LoftRevisionForm {
        LoftRevisionForm {
            revision: self.revision,
            flags: self.flags,
            ints: self.ints,
            cache: self.cache.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            tail_flag: self.tail_flag,
        }
    }
}

impl LoftSection {
    /// The section with admitted entries.
    pub(super) fn admit(self) -> Option<LoftSection<FiniteReal, FiniteVector3>> {
        Some(LoftSection {
            entries: self
                .entries
                .into_iter()
                .map(LoftSectionEntry::admit)
                .collect::<Option<Vec<_>>>()?,
        })
    }
}

impl LoftSection<FiniteReal, FiniteVector3> {
    /// The section with raw entries.
    #[must_use]
    pub fn to_raw(&self) -> LoftSection {
        LoftSection {
            entries: self.entries.iter().map(LoftSectionEntry::to_raw).collect(),
        }
    }
}

impl LoftBridgeToken {
    /// The token with an admitted double.
    pub(super) fn admit(self) -> Option<LoftBridgeToken<FiniteReal>> {
        Some(match self {
            Self::Boolean(value) => LoftBridgeToken::Boolean(value),
            Self::Integer(value) => LoftBridgeToken::Integer(value),
            Self::Double(value) => LoftBridgeToken::Double(FiniteReal::new(value)?),
            Self::Text(value) => LoftBridgeToken::Text(value),
            Self::Enum(value) => LoftBridgeToken::Enum(value),
        })
    }
}

impl LoftBridgeToken<FiniteReal> {
    /// The token with a raw double.
    #[must_use]
    pub fn to_raw(&self) -> LoftBridgeToken {
        match self {
            Self::Boolean(value) => LoftBridgeToken::Boolean(*value),
            Self::Integer(value) => LoftBridgeToken::Integer(*value),
            Self::Double(value) => LoftBridgeToken::Double(value.get()),
            Self::Text(value) => LoftBridgeToken::Text(value.clone()),
            Self::Enum(value) => LoftBridgeToken::Enum(*value),
        }
    }
}

impl G2BlendSide {
    /// The side with an admitted direction.
    fn admit(self) -> Option<G2BlendSide<FiniteVector3>> {
        Some(G2BlendSide {
            label: self.label,
            surface: self.surface,
            curve: self.curve,
            pcurves: self.pcurves,
            direction: FiniteVector3::new(self.direction)?,
        })
    }
}

impl G2BlendSide<FiniteVector3> {
    /// The side with a raw direction.
    #[must_use]
    pub fn to_raw(&self) -> G2BlendSide {
        G2BlendSide {
            label: self.label.clone(),
            surface: self.surface.clone(),
            curve: self.curve.clone(),
            pcurves: self.pcurves.clone(),
            direction: self.direction.get(),
        }
    }
}

impl G2BlendFirstShape {
    /// The shape with admitted coefficients and token.
    fn admit(self) -> Option<G2BlendFirstShape<FiniteReal>> {
        Some(match self {
            Self::Full { support } => G2BlendFirstShape::Full { support },
            Self::None {
                coefficients,
                tolerance,
                extension,
                pcurve,
            } => G2BlendFirstShape::None {
                coefficients: FiniteReal::array(coefficients)?,
                tolerance,
                extension: match extension {
                    None => None,
                    Some(token) => Some(token.admit()?),
                },
                pcurve,
            },
        })
    }
}

impl G2BlendFirstShape<FiniteReal> {
    /// The shape with raw coefficients and token.
    #[must_use]
    pub fn to_raw(&self) -> G2BlendFirstShape {
        match self {
            Self::Full { support } => G2BlendFirstShape::Full {
                support: support.clone(),
            },
            Self::None {
                coefficients,
                tolerance,
                extension,
                pcurve,
            } => G2BlendFirstShape::None {
                coefficients: FiniteReal::raw_array(*coefficients),
                tolerance: *tolerance,
                extension: extension.as_ref().map(LoftBridgeToken::to_raw),
                pcurve: pcurve.clone(),
            },
        }
    }
}

impl G2BlendConstruction {
    /// The construction with admitted sides, shape, scalars and intervals.
    pub(super) fn admit(self) -> Option<G2BlendConstruction<FiniteReal, FiniteVector3>> {
        Some(G2BlendConstruction {
            first: self.first.admit()?,
            singularity: self.singularity,
            first_shape: self.first_shape.admit()?,
            second: self.second.admit()?,
            second_exact_surface: self.second_exact_surface,
            center_curve: self.center_curve,
            center_parameters: FiniteReal::array(self.center_parameters)?,
            center_flag: self.center_flag,
            parameter_ranges: FiniteReal::grid(self.parameter_ranges)?,
            trailing_parameters: FiniteReal::array(self.trailing_parameters)?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
        })
    }
}

impl G2BlendConstruction<FiniteReal, FiniteVector3> {
    /// The construction with raw sides, shape, scalars and intervals.
    #[must_use]
    pub fn to_raw(&self) -> G2BlendConstruction {
        G2BlendConstruction {
            first: self.first.to_raw(),
            singularity: self.singularity,
            first_shape: self.first_shape.to_raw(),
            second: self.second.to_raw(),
            second_exact_surface: self.second_exact_surface.clone(),
            center_curve: self.center_curve.clone(),
            center_parameters: FiniteReal::raw_array(self.center_parameters),
            center_flag: self.center_flag,
            parameter_ranges: FiniteReal::raw_grid(self.parameter_ranges),
            trailing_parameters: FiniteReal::raw_array(self.trailing_parameters),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
        }
    }
}

impl<S> RollingBallSupportSurface<S> {
    /// The support with admitted parameter endpoints.
    fn admit(self) -> Option<RollingBallSupportSurface<S, FiniteReal>> {
        Some(RollingBallSupportSurface {
            surface: self.surface,
            parameter_ranges: FiniteReal::optional_grid(self.parameter_ranges)?,
        })
    }
}

impl<S: Clone> RollingBallSupportSurface<S, FiniteReal> {
    /// The support with raw parameter endpoints.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallSupportSurface<S> {
        RollingBallSupportSurface {
            surface: self.surface.clone(),
            parameter_ranges: FiniteReal::raw_optional_grid(self.parameter_ranges),
        }
    }
}

impl<C> RollingBallSupportCurve<C> {
    /// The curve with admitted parameter endpoints.
    pub(super) fn admit(self) -> Option<RollingBallSupportCurve<C, FiniteReal>> {
        Some(RollingBallSupportCurve {
            curve: self.curve,
            parameter_range: FiniteReal::optional(self.parameter_range)?,
        })
    }
}

impl<C: Clone> RollingBallSupportCurve<C, FiniteReal> {
    /// The curve with raw parameter endpoints.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallSupportCurve<C> {
        RollingBallSupportCurve {
            curve: self.curve.clone(),
            parameter_range: FiniteReal::raw_optional(self.parameter_range),
        }
    }
}

impl<S, C, P> RollingBallSide<S, C, P> {
    /// The side with admitted support endpoints and location. The pcurve
    /// fields carry their own admission.
    pub(super) fn admit(self) -> Option<RollingBallSide<S, C, P, FiniteReal, FinitePoint3>> {
        Some(RollingBallSide {
            support_kind: self.support_kind,
            surface: match self.surface {
                None => None,
                Some(surface) => Some(surface.admit()?),
            },
            curve: match self.curve {
                None => None,
                Some(curve) => Some(curve.admit()?),
            },
            pcurve: self.pcurve,
            location: FinitePoint3::new(self.location)?,
            secondary_pcurve: self.secondary_pcurve,
            extension: self.extension,
        })
    }
}

impl<S: Clone, C: Clone, P: Clone> RollingBallSide<S, C, P, FiniteReal, FinitePoint3> {
    /// The side with raw support endpoints and location.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallSide<S, C, P> {
        RollingBallSide {
            support_kind: self.support_kind,
            surface: self.surface.as_ref().map(RollingBallSupportSurface::to_raw),
            curve: self.curve.as_ref().map(RollingBallSupportCurve::to_raw),
            pcurve: self.pcurve.clone(),
            location: self.location.get(),
            secondary_pcurve: self.secondary_pcurve.clone(),
            extension: self.extension.clone(),
        }
    }
}

impl RollingBallThirdSide {
    /// The third side with an admitted direction.
    fn admit(self) -> Option<RollingBallThirdSide<FiniteVector3>> {
        Some(RollingBallThirdSide {
            label: self.label,
            surface: self.surface,
            curve: self.curve,
            pcurve: self.pcurve,
            direction: FiniteVector3::new(self.direction)?,
            secondary_pcurve: self.secondary_pcurve,
            extension: self.extension,
            tertiary_pcurve: self.tertiary_pcurve,
            flag: self.flag,
        })
    }
}

impl RollingBallThirdSide<FiniteVector3> {
    /// The third side with a raw direction.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallThirdSide {
        RollingBallThirdSide {
            label: self.label.clone(),
            surface: self.surface.clone(),
            curve: self.curve.clone(),
            pcurve: self.pcurve.clone(),
            direction: self.direction.get(),
            secondary_pcurve: self.secondary_pcurve.clone(),
            extension: self.extension,
            tertiary_pcurve: self.tertiary_pcurve.clone(),
            flag: self.flag,
        }
    }
}

impl RollingBallRadiusSelector {
    /// The selector with an admitted value.
    fn admit(self) -> Option<RollingBallRadiusSelector<FiniteReal>> {
        Some(match self {
            Self::None {} => RollingBallRadiusSelector::None {},
            Self::Value { value } => RollingBallRadiusSelector::Value {
                value: FiniteReal::new(value)?,
            },
        })
    }
}

impl RollingBallRadiusSelector<FiniteReal> {
    /// The selector with a raw value.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallRadiusSelector {
        match self {
            Self::None {} => RollingBallRadiusSelector::None {},
            Self::Value { value } => RollingBallRadiusSelector::Value { value: value.get() },
        }
    }
}

impl RollingBallConstruction {
    /// The construction with admitted sides, scalars, cache and third side.
    pub(super) fn admit(
        self,
    ) -> Option<RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        let [first, second] = *self.sides;
        Some(RollingBallConstruction {
            revision: self.revision,
            sides: Box::new([first.admit()?, second.admit()?]),
            slice: self.slice,
            slice_range: FiniteReal::optional(self.slice_range)?,
            offsets: FiniteReal::array(self.offsets)?,
            radius_selector: self.radius_selector.admit()?,
            u_range: FiniteReal::optional(self.u_range)?,
            v_range: FiniteReal::optional(self.v_range)?,
            shape_prefix: self.shape_prefix,
            parameters: FiniteReal::array(self.parameters)?,
            tail: self.tail,
            cache: self.cache.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            tail_flag: self.tail_flag,
            third: match self.third {
                None => None,
                Some(third) => Some(Box::new(third.admit()?)),
            },
            tail_extensions: self.tail_extensions,
        })
    }
}

impl RollingBallConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw sides, scalars, cache and third side.
    #[must_use]
    pub fn to_raw(&self) -> RollingBallConstruction {
        RollingBallConstruction {
            revision: self.revision,
            sides: Box::new([self.sides[0].to_raw(), self.sides[1].to_raw()]),
            slice: self.slice.clone(),
            slice_range: FiniteReal::raw_optional(self.slice_range),
            offsets: FiniteReal::raw_array(self.offsets),
            radius_selector: self.radius_selector.to_raw(),
            u_range: FiniteReal::raw_optional(self.u_range),
            v_range: FiniteReal::raw_optional(self.v_range),
            shape_prefix: self.shape_prefix,
            parameters: FiniteReal::raw_array(self.parameters),
            tail: self.tail,
            cache: self.cache.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            tail_flag: self.tail_flag,
            third: self.third.as_ref().map(|third| Box::new(third.to_raw())),
            tail_extensions: self.tail_extensions,
        }
    }
}

impl VariableBlendInterpolationPoint {
    /// The control with admitted scalars, location and normal.
    fn admit(
        self,
    ) -> Option<VariableBlendInterpolationPoint<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(VariableBlendInterpolationPoint {
            parameter: FiniteReal::new(self.parameter)?,
            radius: FiniteReal::new(self.radius)?,
            tangents: FiniteReal::optional(self.tangents)?,
            location: FinitePoint3::new(self.location)?,
            normal: FiniteVector3::new(self.normal)?,
        })
    }
}

impl VariableBlendInterpolationPoint<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The control with raw scalars, location and normal.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendInterpolationPoint {
        VariableBlendInterpolationPoint {
            parameter: self.parameter.get(),
            radius: self.radius.get(),
            tangents: FiniteReal::raw_optional(self.tangents),
            location: self.location.get(),
            normal: self.normal.get(),
        }
    }
}

impl VariableBlendTerminal {
    /// The terminal with an admitted double.
    fn admit(self) -> Option<VariableBlendTerminal<FiniteReal>> {
        Some(match self {
            Self::Double(value) => VariableBlendTerminal::Double(FiniteReal::new(value)?),
            Self::Text(value) => VariableBlendTerminal::Text(value),
        })
    }
}

impl VariableBlendTerminal<FiniteReal> {
    /// The terminal with a raw double.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendTerminal {
        match self {
            Self::Double(value) => VariableBlendTerminal::Double(value.get()),
            Self::Text(value) => VariableBlendTerminal::Text(value.clone()),
        }
    }
}

impl VariableBlendValue {
    /// The value with an admitted payload, recursively.
    pub(crate) fn admit(
        self,
    ) -> Option<VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(VariableBlendValue {
            modern_flag: self.modern_flag,
            calibrated: self.calibrated,
            payload: self.payload.admit()?,
        })
    }
}

impl VariableBlendValue<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The value with a raw payload, recursively.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendValue {
        VariableBlendValue {
            modern_flag: self.modern_flag,
            calibrated: self.calibrated,
            payload: self.payload.to_raw(),
        }
    }
}

impl VariableBlendValuePayload {
    /// The payload with admitted scalars and controls, recursively.
    fn admit(self) -> Option<VariableBlendValuePayload<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::TwoEnds {
                discriminator,
                parameters,
                radii,
            } => VariableBlendValuePayload::TwoEnds {
                discriminator,
                parameters: FiniteReal::array(parameters)?,
                radii: FiniteReal::array(radii)?,
            },
            Self::FixedWidth {
                discriminator,
                parameters,
                width,
            } => VariableBlendValuePayload::FixedWidth {
                discriminator,
                parameters: FiniteReal::array(parameters)?,
                width: FiniteReal::new(width)?,
            },
            Self::EdgeOffset {
                discriminator,
                scalars,
                lengths,
            } => VariableBlendValuePayload::EdgeOffset {
                discriminator,
                scalars: FiniteReal::array(scalars)?,
                lengths: FiniteReal::array(lengths)?,
            },
            Self::Functional {
                discriminator,
                parameter,
                radius,
                function,
                terminal,
            } => VariableBlendValuePayload::Functional {
                discriminator,
                parameter: FiniteReal::new(parameter)?,
                radius: FiniteReal::new(radius)?,
                function,
                terminal: terminal.admit()?,
            },
            Self::Constant {
                discriminator,
                parameters,
                radius,
                variable_chamfer,
                chamfer_type,
                nested,
            } => VariableBlendValuePayload::Constant {
                discriminator,
                parameters: FiniteReal::array(parameters)?,
                radius: FiniteReal::new(radius)?,
                variable_chamfer,
                chamfer_type,
                nested: Box::new(nested.admit()?),
            },
            Self::Interpolated {
                discriminator,
                parameter,
                radius,
                function,
                enum_count,
                enum_tagged,
                points,
            } => VariableBlendValuePayload::Interpolated {
                discriminator,
                parameter: FiniteReal::new(parameter)?,
                radius: FiniteReal::new(radius)?,
                function,
                enum_count,
                enum_tagged,
                points: points
                    .into_iter()
                    .map(VariableBlendInterpolationPoint::admit)
                    .collect::<Option<Vec<_>>>()?,
            },
        })
    }
}

impl VariableBlendValuePayload<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The payload with raw scalars and controls, recursively.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendValuePayload {
        match self {
            Self::TwoEnds {
                discriminator,
                parameters,
                radii,
            } => VariableBlendValuePayload::TwoEnds {
                discriminator: *discriminator,
                parameters: FiniteReal::raw_array(*parameters),
                radii: FiniteReal::raw_array(*radii),
            },
            Self::FixedWidth {
                discriminator,
                parameters,
                width,
            } => VariableBlendValuePayload::FixedWidth {
                discriminator: *discriminator,
                parameters: FiniteReal::raw_array(*parameters),
                width: width.get(),
            },
            Self::EdgeOffset {
                discriminator,
                scalars,
                lengths,
            } => VariableBlendValuePayload::EdgeOffset {
                discriminator: *discriminator,
                scalars: FiniteReal::raw_array(*scalars),
                lengths: FiniteReal::raw_array(*lengths),
            },
            Self::Functional {
                discriminator,
                parameter,
                radius,
                function,
                terminal,
            } => VariableBlendValuePayload::Functional {
                discriminator: *discriminator,
                parameter: parameter.get(),
                radius: radius.get(),
                function: function.clone(),
                terminal: terminal.to_raw(),
            },
            Self::Constant {
                discriminator,
                parameters,
                radius,
                variable_chamfer,
                chamfer_type,
                nested,
            } => VariableBlendValuePayload::Constant {
                discriminator: *discriminator,
                parameters: FiniteReal::raw_array(*parameters),
                radius: radius.get(),
                variable_chamfer: *variable_chamfer,
                chamfer_type: *chamfer_type,
                nested: Box::new(nested.to_raw()),
            },
            Self::Interpolated {
                discriminator,
                parameter,
                radius,
                function,
                enum_count,
                enum_tagged,
                points,
            } => VariableBlendValuePayload::Interpolated {
                discriminator: *discriminator,
                parameter: parameter.get(),
                radius: radius.get(),
                function: function.clone(),
                enum_count: *enum_count,
                enum_tagged: *enum_tagged,
                points: points
                    .iter()
                    .map(VariableBlendInterpolationPoint::to_raw)
                    .collect(),
            },
        }
    }
}

impl VariableBlendRadii {
    /// The radius laws with admitted values.
    fn admit(self) -> Option<VariableBlendRadii<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::Single { value } => VariableBlendRadii::Single {
                value: value.admit()?,
            },
            Self::Two { first, second } => VariableBlendRadii::Two {
                first: first.admit()?,
                second: second.admit()?,
            },
        })
    }
}

impl VariableBlendRadii<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The radius laws with raw values.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendRadii {
        match self {
            Self::Single { value } => VariableBlendRadii::Single {
                value: value.to_raw(),
            },
            Self::Two { first, second } => VariableBlendRadii::Two {
                first: first.to_raw(),
                second: second.to_raw(),
            },
        }
    }
}

impl VariableBlendCrossSection {
    /// The clause with admitted parameters and radius law.
    fn admit(self) -> Option<VariableBlendCrossSection<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::Circular {} => VariableBlendCrossSection::Circular {},
            Self::Thumbweights { parameters } => VariableBlendCrossSection::Thumbweights {
                parameters: FiniteReal::array(parameters)?,
            },
            Self::RoundedChamfer { radius } => VariableBlendCrossSection::RoundedChamfer {
                radius: match radius {
                    None => None,
                    Some(radius) => Some(Box::new(radius.admit()?)),
                },
            },
            Self::G2Round { parameters } => VariableBlendCrossSection::G2Round {
                parameters: FiniteReal::array(parameters)?,
            },
            Self::UnclassifiedBare { selector } => {
                VariableBlendCrossSection::UnclassifiedBare { selector }
            }
        })
    }
}

impl VariableBlendCrossSection<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The clause with raw parameters and radius law.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendCrossSection {
        match self {
            Self::Circular {} => VariableBlendCrossSection::Circular {},
            Self::Thumbweights { parameters } => VariableBlendCrossSection::Thumbweights {
                parameters: FiniteReal::raw_array(*parameters),
            },
            Self::RoundedChamfer { radius } => VariableBlendCrossSection::RoundedChamfer {
                radius: radius.as_ref().map(|radius| Box::new(radius.to_raw())),
            },
            Self::G2Round { parameters } => VariableBlendCrossSection::G2Round {
                parameters: FiniteReal::raw_array(*parameters),
            },
            Self::UnclassifiedBare { selector } => VariableBlendCrossSection::UnclassifiedBare {
                selector: *selector,
            },
        }
    }
}

impl VariableBlendConstruction {
    /// The construction with admitted sides, laws, scalars and cache.
    pub(super) fn admit(
        self,
    ) -> Option<VariableBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        let [first, second] = *self.sides;
        Some(VariableBlendConstruction {
            subtype: self.subtype,
            revision: self.revision,
            sides: Box::new([first.admit()?, second.admit()?]),
            slice: self.slice,
            slice_range: FiniteReal::optional(self.slice_range)?,
            offsets: FiniteReal::array(self.offsets)?,
            radii: self.radii.admit()?,
            cross_section: match self.cross_section {
                None => None,
                Some(cross_section) => Some(cross_section.admit()?),
            },
            u_range: FiniteReal::array(self.u_range)?,
            v_lower: match self.v_lower {
                None => None,
                Some(lower) => Some(FiniteReal::new(lower)?),
            },
            shape_parameter: FiniteReal::new(self.shape_parameter)?,
            shape_length: FiniteReal::new(self.shape_length)?,
            shape_tail: self.shape_tail,
            cache: self.cache.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            tail_flag: self.tail_flag,
            tail_extensions: self.tail_extensions,
            secondary_curve: match self.secondary_curve {
                None => None,
                Some(curve) => Some(curve.admit()?),
            },
            convexity: self.convexity,
            render_mode: self.render_mode,
            post_range: FiniteReal::optional(self.post_range)?,
            post_curve: self.post_curve,
            post_pcurve: self.post_pcurve,
        })
    }
}

impl VariableBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw sides, laws, scalars and cache.
    #[must_use]
    pub fn to_raw(&self) -> VariableBlendConstruction {
        VariableBlendConstruction {
            subtype: self.subtype,
            revision: self.revision,
            sides: Box::new([self.sides[0].to_raw(), self.sides[1].to_raw()]),
            slice: self.slice.clone(),
            slice_range: FiniteReal::raw_optional(self.slice_range),
            offsets: FiniteReal::raw_array(self.offsets),
            radii: self.radii.to_raw(),
            cross_section: self
                .cross_section
                .as_ref()
                .map(VariableBlendCrossSection::to_raw),
            u_range: FiniteReal::raw_array(self.u_range),
            v_lower: self.v_lower.map(FiniteReal::get),
            shape_parameter: self.shape_parameter.get(),
            shape_length: self.shape_length.get(),
            shape_tail: self.shape_tail,
            cache: self.cache.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            tail_flag: self.tail_flag,
            tail_extensions: self.tail_extensions,
            secondary_curve: self
                .secondary_curve
                .as_ref()
                .map(RollingBallSupportCurve::to_raw),
            convexity: self.convexity,
            render_mode: self.render_mode,
            post_range: FiniteReal::raw_optional(self.post_range),
            post_curve: self.post_curve.clone(),
            post_pcurve: self.post_pcurve.clone(),
        }
    }
}

impl VertexBlendTwists {
    /// The twists with admitted entries.
    fn admit(self) -> Option<VertexBlendTwists<FinitePoint3>> {
        Some(match self {
            Self::None {} => VertexBlendTwists::None {},
            Self::One { twist } => VertexBlendTwists::One {
                twist: FinitePoint3::new(twist)?,
            },
            Self::Two { twists } => VertexBlendTwists::Two {
                twists: FinitePoint3::array(twists)?,
            },
        })
    }
}

impl VertexBlendTwists<FinitePoint3> {
    /// The twists with raw entries.
    #[must_use]
    pub fn to_raw(&self) -> VertexBlendTwists {
        match self {
            Self::None {} => VertexBlendTwists::None {},
            Self::One { twist } => VertexBlendTwists::One { twist: twist.get() },
            Self::Two { twists } => VertexBlendTwists::Two {
                twists: FinitePoint3::raw_array(*twists),
            },
        }
    }
}

impl VertexBlendBoundaryGeometry {
    /// The geometry with admitted scalars, points and vectors.
    fn admit(self) -> Option<VertexBlendBoundaryGeometry<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::Circle {
                curve,
                curve_endpoints,
                twists,
                parameters,
                sense,
            } => VertexBlendBoundaryGeometry::Circle {
                curve,
                curve_endpoints: FiniteReal::optional(curve_endpoints)?,
                twists: twists.admit()?,
                parameters: FiniteReal::array(parameters)?,
                sense,
            },
            Self::Degenerate { location, normals } => VertexBlendBoundaryGeometry::Degenerate {
                location: FinitePoint3::new(location)?,
                normals: FiniteVector3::array(normals)?,
            },
            Self::Pcurve {
                surface,
                support_bounds,
                pcurve,
                sense,
                fit_tolerance,
            } => VertexBlendBoundaryGeometry::Pcurve {
                surface,
                support_bounds: FiniteReal::optional(support_bounds)?,
                pcurve,
                sense,
                fit_tolerance,
            },
            Self::Plane {
                normal,
                parameters,
                curve,
                curve_endpoints,
            } => VertexBlendBoundaryGeometry::Plane {
                normal: FiniteVector3::new(normal)?,
                parameters: FiniteReal::array(parameters)?,
                curve,
                curve_endpoints: FiniteReal::optional(curve_endpoints)?,
            },
        })
    }
}

impl VertexBlendBoundaryGeometry<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The geometry with raw scalars, points and vectors.
    #[must_use]
    pub fn to_raw(&self) -> VertexBlendBoundaryGeometry {
        match self {
            Self::Circle {
                curve,
                curve_endpoints,
                twists,
                parameters,
                sense,
            } => VertexBlendBoundaryGeometry::Circle {
                curve: curve.clone(),
                curve_endpoints: FiniteReal::raw_optional(*curve_endpoints),
                twists: twists.to_raw(),
                parameters: FiniteReal::raw_array(*parameters),
                sense: *sense,
            },
            Self::Degenerate { location, normals } => VertexBlendBoundaryGeometry::Degenerate {
                location: location.get(),
                normals: FiniteVector3::raw_array(*normals),
            },
            Self::Pcurve {
                surface,
                support_bounds,
                pcurve,
                sense,
                fit_tolerance,
            } => VertexBlendBoundaryGeometry::Pcurve {
                surface: surface.clone(),
                support_bounds: FiniteReal::raw_optional(*support_bounds),
                pcurve: pcurve.clone(),
                sense: *sense,
                fit_tolerance: *fit_tolerance,
            },
            Self::Plane {
                normal,
                parameters,
                curve,
                curve_endpoints,
            } => VertexBlendBoundaryGeometry::Plane {
                normal: normal.get(),
                parameters: FiniteReal::raw_array(*parameters),
                curve: curve.clone(),
                curve_endpoints: FiniteReal::raw_optional(*curve_endpoints),
            },
        }
    }
}

impl VertexBlendBoundary {
    /// The boundary with admitted magic direction, fullness and geometry.
    fn admit(self) -> Option<VertexBlendBoundary<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(VertexBlendBoundary {
            boundary_type: self.boundary_type,
            magic: FiniteVector3::new(self.magic)?,
            u_smoothing: self.u_smoothing,
            v_smoothing: self.v_smoothing,
            fullness: FiniteReal::new(self.fullness)?,
            geometry: self.geometry.admit()?,
        })
    }
}

impl VertexBlendBoundary<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The boundary with raw magic direction, fullness and geometry.
    #[must_use]
    pub fn to_raw(&self) -> VertexBlendBoundary {
        VertexBlendBoundary {
            boundary_type: self.boundary_type,
            magic: self.magic.get(),
            u_smoothing: self.u_smoothing,
            v_smoothing: self.v_smoothing,
            fullness: self.fullness.get(),
            geometry: self.geometry.to_raw(),
        }
    }
}

impl VertexBlendConstruction {
    /// The construction with admitted boundaries.
    pub(super) fn admit(
        self,
    ) -> Option<VertexBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(VertexBlendConstruction {
            revision: self.revision,
            boundaries: self
                .boundaries
                .into_iter()
                .map(VertexBlendBoundary::admit)
                .collect::<Option<Vec<_>>>()?,
            grid_size: self.grid_size,
            fit_tolerance: self.fit_tolerance,
        })
    }
}

impl VertexBlendConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw boundaries.
    #[must_use]
    pub fn to_raw(&self) -> VertexBlendConstruction {
        VertexBlendConstruction {
            revision: self.revision,
            boundaries: self
                .boundaries
                .iter()
                .map(VertexBlendBoundary::to_raw)
                .collect(),
            grid_size: self.grid_size,
            fit_tolerance: self.fit_tolerance,
        }
    }
}

impl CompoundLoftScaleMember {
    /// The member with admitted constraint data.
    fn admit(self) -> Option<CompoundLoftScaleMember<FiniteReal, FiniteVector3>> {
        Some(CompoundLoftScaleMember {
            type_code: self.type_code,
            curve: self.curve,
            data: self.data.admit()?,
        })
    }
}

impl CompoundLoftScaleMember<FiniteReal, FiniteVector3> {
    /// The member with raw constraint data.
    #[must_use]
    pub fn to_raw(&self) -> CompoundLoftScaleMember {
        CompoundLoftScaleMember {
            type_code: self.type_code,
            curve: self.curve.clone(),
            data: self.data.to_raw(),
        }
    }
}

impl CompoundLoftScale {
    /// The scale block with admitted members.
    fn admit(self) -> Option<CompoundLoftScale<FiniteReal, FiniteVector3>> {
        Some(CompoundLoftScale {
            members: self
                .members
                .into_iter()
                .map(CompoundLoftScaleMember::admit)
                .collect::<Option<Vec<_>>>()?,
            path: self.path,
            auxiliaries: self.auxiliaries,
            tail: self.tail,
        })
    }
}

impl CompoundLoftScale<FiniteReal, FiniteVector3> {
    /// The scale block with raw members.
    #[must_use]
    pub fn to_raw(&self) -> CompoundLoftScale {
        CompoundLoftScale {
            members: self
                .members
                .iter()
                .map(CompoundLoftScaleMember::to_raw)
                .collect(),
            path: self.path.clone(),
            auxiliaries: self.auxiliaries.clone(),
            tail: self.tail,
        }
    }
}

/// Admit an optional boxed scale block, or refuse it.
fn optional_scale(
    scale: Option<Box<CompoundLoftScale>>,
) -> Option<Option<Box<CompoundLoftScale<FiniteReal, FiniteVector3>>>> {
    match scale {
        None => Some(None),
        Some(scale) => Some(Some(Box::new(scale.admit()?))),
    }
}

impl CompoundLoftDirection {
    /// The direction with an admitted vector.
    pub(super) fn admit(self) -> Option<CompoundLoftDirection<FiniteVector3>> {
        Some(match self {
            Self::Vector { value } => CompoundLoftDirection::Vector {
                value: FiniteVector3::new(value)?,
            },
            Self::Curve { curve, selector } => CompoundLoftDirection::Curve { curve, selector },
        })
    }
}

impl CompoundLoftDirection<FiniteVector3> {
    /// The direction with a raw vector.
    #[must_use]
    pub fn to_raw(&self) -> CompoundLoftDirection {
        match self {
            Self::Vector { value } => CompoundLoftDirection::Vector { value: value.get() },
            Self::Curve { curve, selector } => CompoundLoftDirection::Curve {
                curve: curve.clone(),
                selector: *selector,
            },
        }
    }
}

impl CompoundLoftTail {
    /// The tail with admitted scale blocks, directions and interval.
    fn admit(self) -> Option<CompoundLoftTail<FiniteReal, FiniteVector3>> {
        Some(match self {
            Self::Six {
                flags,
                scale,
                selector,
                direction,
                parameter_range,
                curve,
            } => CompoundLoftTail::Six {
                flags,
                scale: Box::new(scale.admit()?),
                selector,
                direction: FiniteVector3::new(direction)?,
                parameter_range: FiniteReal::array(parameter_range)?,
                curve,
            },
            Self::Seven {
                first_flag,
                first_scale,
                second_flag,
                second_scale,
                selector,
                direction,
                trailing_flags,
            } => CompoundLoftTail::Seven {
                first_flag,
                first_scale: optional_scale(first_scale)?,
                second_flag,
                second_scale: Box::new(second_scale.admit()?),
                selector,
                direction: FiniteVector3::new(direction)?,
                trailing_flags,
            },
            Self::Zero {
                flags,
                direction,
                trailing_flags,
            } => CompoundLoftTail::Zero {
                flags,
                direction: direction.admit()?,
                trailing_flags,
            },
        })
    }
}

impl CompoundLoftTail<FiniteReal, FiniteVector3> {
    /// The tail with raw scale blocks, directions and interval.
    #[must_use]
    pub fn to_raw(&self) -> CompoundLoftTail {
        match self {
            Self::Six {
                flags,
                scale,
                selector,
                direction,
                parameter_range,
                curve,
            } => CompoundLoftTail::Six {
                flags: *flags,
                scale: Box::new(scale.to_raw()),
                selector: *selector,
                direction: direction.get(),
                parameter_range: FiniteReal::raw_array(*parameter_range),
                curve: curve.clone(),
            },
            Self::Seven {
                first_flag,
                first_scale,
                second_flag,
                second_scale,
                selector,
                direction,
                trailing_flags,
            } => CompoundLoftTail::Seven {
                first_flag: *first_flag,
                first_scale: first_scale.as_ref().map(|scale| Box::new(scale.to_raw())),
                second_flag: *second_flag,
                second_scale: Box::new(second_scale.to_raw()),
                selector: *selector,
                direction: direction.get(),
                trailing_flags: *trailing_flags,
            },
            Self::Zero {
                flags,
                direction,
                trailing_flags,
            } => CompoundLoftTail::Zero {
                flags: *flags,
                direction: direction.to_raw(),
                trailing_flags: *trailing_flags,
            },
        }
    }
}

impl<const CAPACITY: usize> CompoundLoftScales<CAPACITY> {
    /// The scales with admitted members. Admission keeps every scale, so the
    /// list stays within its capacity.
    fn admit(self) -> Option<CompoundLoftScales<CAPACITY, FiniteReal, FiniteVector3>> {
        Some(CompoundLoftScales(
            self.0
                .into_iter()
                .map(CompoundLoftScale::admit)
                .collect::<Option<Vec<_>>>()?,
        ))
    }
}

impl<const CAPACITY: usize> CompoundLoftScales<CAPACITY, FiniteReal, FiniteVector3> {
    /// The scales with raw members.
    #[must_use]
    pub fn to_raw(&self) -> CompoundLoftScales<CAPACITY> {
        CompoundLoftScales(self.0.iter().map(CompoundLoftScale::to_raw).collect())
    }
}

impl CompoundLoftConstruction {
    /// The construction with admitted scales and tail.
    pub(super) fn admit(self) -> Option<CompoundLoftConstruction<FiniteReal, FiniteVector3>> {
        Some(CompoundLoftConstruction {
            scales: self.scales.admit()?,
            flags: self.flags,
            tail: self.tail.admit()?,
        })
    }
}

impl CompoundLoftConstruction<FiniteReal, FiniteVector3> {
    /// The construction with raw scales and tail.
    #[must_use]
    pub fn to_raw(&self) -> CompoundLoftConstruction {
        CompoundLoftConstruction {
            scales: self.scales.to_raw(),
            flags: self.flags,
            tail: self.tail.to_raw(),
        }
    }
}

impl ScaledCompoundLoftShape {
    /// The shape with admitted intervals and scalar arrays.
    fn admit(self) -> Option<ScaledCompoundLoftShape<FiniteReal>> {
        Some(match self {
            Self::Full {} => ScaledCompoundLoftShape::Full {},
            Self::None {
                parameter_ranges,
                parameters,
            } => ScaledCompoundLoftShape::None {
                parameter_ranges: FiniteReal::grid(parameter_ranges)?,
                parameters: FiniteReal::lanes(parameters)?,
            },
        })
    }
}

impl ScaledCompoundLoftShape<FiniteReal> {
    /// The shape with raw intervals and scalar arrays.
    #[must_use]
    pub fn to_raw(&self) -> ScaledCompoundLoftShape {
        match self {
            Self::Full {} => ScaledCompoundLoftShape::Full {},
            Self::None {
                parameter_ranges,
                parameters,
            } => ScaledCompoundLoftShape::None {
                parameter_ranges: FiniteReal::raw_grid(*parameter_ranges),
                parameters: FiniteReal::raw_lanes(parameters),
            },
        }
    }
}

impl ScaledCompoundLoftBranch {
    /// The branch with admitted scale blocks and directions.
    fn admit(self) -> Option<ScaledCompoundLoftBranch<FiniteReal, FiniteVector3>> {
        Some(match self {
            Self::ExtendedVector {
                first_scale,
                second_scale,
                selector,
                direction,
            } => ScaledCompoundLoftBranch::ExtendedVector {
                first_scale: optional_scale(first_scale)?,
                second_scale: Box::new(second_scale.admit()?),
                selector,
                direction: FiniteVector3::new(direction)?,
            },
            Self::ExtendedCurve {
                scale,
                flag,
                singularity,
                curve,
            } => ScaledCompoundLoftBranch::ExtendedCurve {
                scale: optional_scale(scale)?,
                flag,
                singularity,
                curve,
            },
            Self::Direct { flag, direction } => ScaledCompoundLoftBranch::Direct {
                flag,
                direction: direction.admit()?,
            },
        })
    }
}

impl ScaledCompoundLoftBranch<FiniteReal, FiniteVector3> {
    /// The branch with raw scale blocks and directions.
    #[must_use]
    pub fn to_raw(&self) -> ScaledCompoundLoftBranch {
        match self {
            Self::ExtendedVector {
                first_scale,
                second_scale,
                selector,
                direction,
            } => ScaledCompoundLoftBranch::ExtendedVector {
                first_scale: first_scale.as_ref().map(|scale| Box::new(scale.to_raw())),
                second_scale: Box::new(second_scale.to_raw()),
                selector: *selector,
                direction: direction.get(),
            },
            Self::ExtendedCurve {
                scale,
                flag,
                singularity,
                curve,
            } => ScaledCompoundLoftBranch::ExtendedCurve {
                scale: scale.as_ref().map(|scale| Box::new(scale.to_raw())),
                flag: *flag,
                singularity: *singularity,
                curve: curve.clone(),
            },
            Self::Direct { flag, direction } => ScaledCompoundLoftBranch::Direct {
                flag: *flag,
                direction: direction.to_raw(),
            },
        }
    }
}

impl ScaledCompoundLoftConstruction {
    /// The construction with admitted shape, scales, branch and vectors.
    pub(super) fn admit(self) -> Option<ScaledCompoundLoftConstruction<FiniteReal, FiniteVector3>> {
        Some(ScaledCompoundLoftConstruction {
            singularity: self.singularity,
            shape: self.shape.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            discontinuity_flag: self.discontinuity_flag,
            scales: self.scales.admit()?,
            flags: self.flags,
            selector: self.selector,
            branch: self.branch.admit()?,
            trailing_flags: self.trailing_flags,
            tail_kind: self.tail_kind,
            tail_directions: FiniteVector3::array(self.tail_directions)?,
            tail_singularity: self.tail_singularity,
            tail_curve: self.tail_curve,
        })
    }
}

impl ScaledCompoundLoftConstruction<FiniteReal, FiniteVector3> {
    /// The construction with raw shape, scales, branch and vectors.
    #[must_use]
    pub fn to_raw(&self) -> ScaledCompoundLoftConstruction {
        ScaledCompoundLoftConstruction {
            singularity: self.singularity,
            shape: self.shape.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            discontinuity_flag: self.discontinuity_flag,
            scales: self.scales.to_raw(),
            flags: self.flags,
            selector: self.selector,
            branch: self.branch.to_raw(),
            trailing_flags: self.trailing_flags,
            tail_kind: self.tail_kind,
            tail_directions: FiniteVector3::raw_array(self.tail_directions),
            tail_singularity: self.tail_singularity,
            tail_curve: self.tail_curve.clone(),
        }
    }
}

impl LawExpression {
    /// The expression with admitted scalars, points and vectors, refused
    /// past the law-expression depth limit.
    fn admit_at_depth(
        self,
        depth: usize,
    ) -> Option<LawExpression<FiniteReal, FiniteVector3, FinitePoint3>> {
        if depth > LAW_EXPRESSION_DEPTH_LIMIT {
            return None;
        }
        Some(match self {
            Self::Null {} => LawExpression::Null {},
            Self::Text { value } => LawExpression::Text { value },
            Self::Integer { value } => LawExpression::Integer { value },
            Self::Double { value } => LawExpression::Double {
                value: FiniteReal::new(value)?,
            },
            Self::Point { value } => LawExpression::Point {
                value: FinitePoint3::new(value)?,
            },
            Self::Vector { value } => LawExpression::Vector {
                value: FiniteVector3::new(value)?,
            },
            Self::Transform { scalars, enums } => LawExpression::Transform {
                scalars: FiniteReal::array(scalars)?,
                enums,
            },
            Self::TransformVec {
                vectors,
                scale,
                flags,
            } => LawExpression::TransformVec {
                vectors: FiniteVector3::array(vectors)?,
                scale: FiniteReal::new(scale)?,
                flags,
            },
            Self::Edge { curve, parameters } => LawExpression::Edge {
                curve: curve.admit()?,
                parameters: FiniteReal::array(parameters)?,
            },
            Self::Spline {
                native_id,
                knots,
                controls,
                point,
            } => LawExpression::Spline {
                native_id,
                knots: FiniteReal::lane(knots)?,
                controls: FiniteReal::lane(controls)?,
                point: FinitePoint3::new(point)?,
            },
            Self::Algebraic { operator, operands } => LawExpression::Algebraic {
                operator,
                operands: operands
                    .into_iter()
                    .map(|operand| operand.admit_at_depth(depth + 1))
                    .collect::<Option<Vec<_>>>()?,
            },
        })
    }

    /// The expression with admitted scalars, points and vectors.
    pub(crate) fn admit(self) -> Option<LawExpression<FiniteReal, FiniteVector3, FinitePoint3>> {
        self.admit_at_depth(0)
    }
}

impl LawExpression<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The expression with raw scalars, points and vectors.
    #[must_use]
    pub fn to_raw(&self) -> LawExpression {
        match self {
            Self::Null {} => LawExpression::Null {},
            Self::Text { value } => LawExpression::Text {
                value: value.clone(),
            },
            Self::Integer { value } => LawExpression::Integer { value: *value },
            Self::Double { value } => LawExpression::Double { value: value.get() },
            Self::Point { value } => LawExpression::Point { value: value.get() },
            Self::Vector { value } => LawExpression::Vector { value: value.get() },
            Self::Transform { scalars, enums } => LawExpression::Transform {
                scalars: FiniteReal::raw_array(*scalars),
                enums: *enums,
            },
            Self::TransformVec {
                vectors,
                scale,
                flags,
            } => LawExpression::TransformVec {
                vectors: FiniteVector3::raw_array(*vectors),
                scale: scale.get(),
                flags: *flags,
            },
            Self::Edge { curve, parameters } => LawExpression::Edge {
                curve: curve.to_raw(),
                parameters: FiniteReal::raw_array(*parameters),
            },
            Self::Spline {
                native_id,
                knots,
                controls,
                point,
            } => LawExpression::Spline {
                native_id: *native_id,
                knots: FiniteReal::raw_lane(knots),
                controls: FiniteReal::raw_lane(controls),
                point: point.get(),
            },
            Self::Algebraic { operator, operands } => LawExpression::Algebraic {
                operator: operator.clone(),
                operands: operands.iter().map(LawExpression::to_raw).collect(),
            },
        }
    }
}

impl LawFormula {
    /// The formula with admitted variables. This is the one law walk: the
    /// curve carrier `FiniteLawFormula` and the law, skin, net and sweep
    /// surface admissions all refuse through it.
    pub(crate) fn admit(self) -> Option<LawFormula<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::Null {} => LawFormula::Null {},
            Self::Named { name, variables } => LawFormula::Named {
                name,
                variables: variables
                    .into_iter()
                    .map(LawExpression::admit)
                    .collect::<Option<Vec<_>>>()?,
            },
        })
    }
}

impl LawFormula<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The formula with raw variables.
    #[must_use]
    pub fn to_raw(&self) -> LawFormula {
        match self {
            Self::Null {} => LawFormula::Null {},
            Self::Named { name, variables } => LawFormula::Named {
                name: name.clone(),
                variables: variables.iter().map(LawExpression::to_raw).collect(),
            },
        }
    }
}

impl LawSurfaceTail {
    /// The tail with admitted summaries and intervals.
    fn admit(self) -> Option<LawSurfaceTail<FiniteReal>> {
        Some(match self {
            Self::Full { cache } => LawSurfaceTail::Full { cache },
            Self::Summary {
                parameters,
                fit_tolerance,
                closures,
                singularities,
            } => LawSurfaceTail::Summary {
                parameters: FiniteReal::lanes(parameters)?,
                fit_tolerance,
                closures,
                singularities,
            },
            Self::None {
                parameter_ranges,
                closures,
                singularities,
            } => LawSurfaceTail::None {
                parameter_ranges: FiniteReal::grid(parameter_ranges)?,
                closures,
                singularities,
            },
            Self::Historical {} => LawSurfaceTail::Historical {},
            Self::Optimal {} => LawSurfaceTail::Optimal {},
        })
    }
}

impl LawSurfaceTail<FiniteReal> {
    /// The tail with raw summaries and intervals.
    #[must_use]
    pub fn to_raw(&self) -> LawSurfaceTail {
        match self {
            Self::Full { cache } => LawSurfaceTail::Full { cache: *cache },
            Self::Summary {
                parameters,
                fit_tolerance,
                closures,
                singularities,
            } => LawSurfaceTail::Summary {
                parameters: FiniteReal::raw_lanes(parameters),
                fit_tolerance: *fit_tolerance,
                closures: *closures,
                singularities: *singularities,
            },
            Self::None {
                parameter_ranges,
                closures,
                singularities,
            } => LawSurfaceTail::None {
                parameter_ranges: FiniteReal::raw_grid(*parameter_ranges),
                closures: *closures,
                singularities: *singularities,
            },
            Self::Historical {} => LawSurfaceTail::Historical {},
            Self::Optimal {} => LawSurfaceTail::Optimal {},
        }
    }
}

impl LawSurfaceConstruction {
    /// The construction with admitted intervals, laws, tail and
    /// discontinuities.
    pub(super) fn admit(
        self,
    ) -> Option<LawSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(LawSurfaceConstruction {
            parameter_ranges: match self.parameter_ranges {
                None => None,
                Some(ranges) => Some(FiniteReal::grid(ranges)?),
            },
            primary: self.primary.admit()?,
            additional: self
                .additional
                .into_iter()
                .map(LawFormula::admit)
                .collect::<Option<Vec<_>>>()?,
            tail: self.tail.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
        })
    }
}

impl LawSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw intervals, laws, tail and discontinuities.
    #[must_use]
    pub fn to_raw(&self) -> LawSurfaceConstruction {
        LawSurfaceConstruction {
            parameter_ranges: self.parameter_ranges.map(FiniteReal::raw_grid),
            primary: self.primary.to_raw(),
            additional: self.additional.iter().map(LawFormula::to_raw).collect(),
            tail: self.tail.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
        }
    }
}

impl SkinSurfaceProfile {
    /// The profile with admitted constraint data.
    fn admit(self) -> Option<SkinSurfaceProfile<FiniteReal, FiniteVector3>> {
        Some(SkinSurfaceProfile {
            type_code: self.type_code,
            curve: self.curve,
            data: self.data.admit()?,
        })
    }
}

impl SkinSurfaceProfile<FiniteReal, FiniteVector3> {
    /// The profile with raw constraint data.
    #[must_use]
    pub fn to_raw(&self) -> SkinSurfaceProfile {
        SkinSurfaceProfile {
            type_code: self.type_code,
            curve: self.curve.clone(),
            data: self.data.to_raw(),
        }
    }
}

impl SkinSurfaceLayout {
    /// The layout with admitted profiles or subdata.
    fn admit(self) -> Option<SkinSurfaceLayout<FiniteReal, FiniteVector3>> {
        Some(match self {
            Self::Profiles {
                profiles,
                path,
                tail,
            } => SkinSurfaceLayout::Profiles {
                profiles: profiles
                    .into_iter()
                    .map(SkinSurfaceProfile::admit)
                    .collect::<Option<Vec<_>>>()?,
                path,
                tail,
            },
            Self::Compact {
                inner_count,
                curve,
                subdata,
                first_tail,
                secondary_curve,
                second_tail,
            } => SkinSurfaceLayout::Compact {
                inner_count,
                curve,
                subdata: subdata.admit()?,
                first_tail,
                secondary_curve,
                second_tail,
            },
        })
    }
}

impl SkinSurfaceLayout<FiniteReal, FiniteVector3> {
    /// The layout with raw profiles or subdata.
    #[must_use]
    pub fn to_raw(&self) -> SkinSurfaceLayout {
        match self {
            Self::Profiles {
                profiles,
                path,
                tail,
            } => SkinSurfaceLayout::Profiles {
                profiles: profiles.iter().map(SkinSurfaceProfile::to_raw).collect(),
                path: path.clone(),
                tail: *tail,
            },
            Self::Compact {
                inner_count,
                curve,
                subdata,
                first_tail,
                secondary_curve,
                second_tail,
            } => SkinSurfaceLayout::Compact {
                inner_count: *inner_count,
                curve: curve.clone(),
                subdata: subdata.to_raw(),
                first_tail: *first_tail,
                secondary_curve: secondary_curve.clone(),
                second_tail: *second_tail,
            },
        }
    }
}

impl SkinSurfaceConstruction {
    /// The construction with admitted layout, scalars, direction and law.
    pub(super) fn admit(
        self,
    ) -> Option<SkinSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(SkinSurfaceConstruction {
            surface_boolean: self.surface_boolean,
            surface_normal: self.surface_normal,
            surface_direction: self.surface_direction,
            count: self.count,
            parameter: FiniteReal::new(self.parameter)?,
            layout: self.layout.admit()?,
            direction: FiniteVector3::new(self.direction)?,
            trailing_parameter: FiniteReal::new(self.trailing_parameter)?,
            formula: self.formula.admit()?,
            parameter_curve: self.parameter_curve,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            discontinuity_flag: self.discontinuity_flag,
        })
    }
}

impl SkinSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw layout, scalars, direction and law.
    #[must_use]
    pub fn to_raw(&self) -> SkinSurfaceConstruction {
        SkinSurfaceConstruction {
            surface_boolean: self.surface_boolean,
            surface_normal: self.surface_normal,
            surface_direction: self.surface_direction,
            count: self.count,
            parameter: self.parameter.get(),
            layout: self.layout.to_raw(),
            direction: self.direction.get(),
            trailing_parameter: self.trailing_parameter.get(),
            formula: self.formula.to_raw(),
            parameter_curve: self.parameter_curve.clone(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            discontinuity_flag: self.discontinuity_flag,
        }
    }
}

impl NetSurfaceConstruction {
    /// The construction with admitted sections, frame, laws and
    /// discontinuities.
    pub(super) fn admit(
        self,
    ) -> Option<NetSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        let [first, second] = *self.sections;
        let [primary, secondary, tertiary, quaternary] = *self.formulas;
        Some(NetSurfaceConstruction {
            sections: Box::new([first.admit()?, second.admit()?]),
            frame_parameters: FiniteReal::array(self.frame_parameters)?,
            flag: self.flag,
            directions: FiniteVector3::array(self.directions)?,
            formulas: Box::new([
                primary.admit()?,
                secondary.admit()?,
                tertiary.admit()?,
                quaternary.admit()?,
            ]),
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            discontinuity_flag: self.discontinuity_flag,
        })
    }
}

impl NetSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw sections, frame, laws and discontinuities.
    #[must_use]
    pub fn to_raw(&self) -> NetSurfaceConstruction {
        NetSurfaceConstruction {
            sections: Box::new([self.sections[0].to_raw(), self.sections[1].to_raw()]),
            frame_parameters: FiniteReal::raw_array(self.frame_parameters),
            flag: self.flag,
            directions: FiniteVector3::raw_array(self.directions),
            formulas: Box::new(std::array::from_fn(|index| self.formulas[index].to_raw())),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            discontinuity_flag: self.discontinuity_flag,
        }
    }
}

impl SweepSurfaceLayout {
    /// The layout with admitted frames, intervals, scalars and laws.
    fn admit(self) -> Option<SweepSurfaceLayout<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::ProfileFirst {
                secondary_kind,
                directions,
                origin,
                parameters,
                formulas,
            } => {
                let [first, second, third] = *formulas;
                SweepSurfaceLayout::ProfileFirst {
                    secondary_kind,
                    directions: FiniteVector3::array(directions)?,
                    origin: FinitePoint3::new(origin)?,
                    parameters: FiniteReal::array(parameters)?,
                    formulas: Box::new([first.admit()?, second.admit()?, third.admit()?]),
                }
            }
            Self::ExplicitFormula {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path_range,
                path_parameter,
                formula_flag,
                formula,
                trailing_flag,
            } => SweepSurfaceLayout::ExplicitFormula {
                mode,
                profile_range: FiniteReal::array(profile_range)?,
                profile_frame: optional_frame(profile_frame)?,
                origin: FinitePoint3::new(origin)?,
                directions: FiniteVector3::array(directions)?,
                trajectory_flag,
                path_range: FiniteReal::array(path_range)?,
                path_parameter: FiniteReal::new(path_parameter)?,
                formula_flag,
                formula: formula.admit()?,
                trailing_flag,
            },
            Self::ExplicitGuide {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path_range,
                path_parameter,
                guide_flags,
                guide_curve,
                guide_range,
                guide_modes,
                guide_parameters,
                trailing_flags,
            } => SweepSurfaceLayout::ExplicitGuide {
                mode,
                profile_range: FiniteReal::array(profile_range)?,
                profile_frame: optional_frame(profile_frame)?,
                origin: FinitePoint3::new(origin)?,
                directions: FiniteVector3::array(directions)?,
                trajectory_flag,
                path_range: FiniteReal::array(path_range)?,
                path_parameter: FiniteReal::new(path_parameter)?,
                guide_flags,
                guide_curve,
                guide_range: FiniteReal::array(guide_range)?,
                guide_modes,
                guide_parameters: FiniteReal::array(guide_parameters)?,
                trailing_flags,
            },
            Self::ExplicitSurface {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path_range,
                path_parameter,
                singularity,
                support_surface,
                auxiliary_curve,
                support_flag,
                legacy_flag,
            } => SweepSurfaceLayout::ExplicitSurface {
                mode,
                profile_range: FiniteReal::array(profile_range)?,
                profile_frame: optional_frame(profile_frame)?,
                origin: FinitePoint3::new(origin)?,
                directions: FiniteVector3::array(directions)?,
                trajectory_flag,
                path_range: FiniteReal::array(path_range)?,
                path_parameter: FiniteReal::new(path_parameter)?,
                singularity,
                support_surface,
                auxiliary_curve,
                support_flag,
                legacy_flag,
            },
            Self::LawDriven {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                first_law,
                first_mode,
                first_range,
                law_direction,
                path_mode,
                path_flag,
                path_range,
                path_parameter,
                second_law_flag,
                second_law,
                formula_mode,
                formula,
                trailing_flag,
            } => SweepSurfaceLayout::LawDriven {
                mode,
                profile_range: FiniteReal::array(profile_range)?,
                profile_frame: optional_frame(profile_frame)?,
                origin: FinitePoint3::new(origin)?,
                directions: FiniteVector3::array(directions)?,
                first_law: Box::new(first_law.admit()?),
                first_mode,
                first_range: FiniteReal::array(first_range)?,
                law_direction: FiniteVector3::new(law_direction)?,
                path_mode,
                path_flag,
                path_range: FiniteReal::array(path_range)?,
                path_parameter: FiniteReal::new(path_parameter)?,
                second_law_flag,
                second_law: Box::new(second_law.admit()?),
                formula_mode,
                formula: formula.admit()?,
                trailing_flag,
            },
        })
    }
}

impl SweepSurfaceLayout<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The layout with raw frames, intervals, scalars and laws.
    #[must_use]
    pub fn to_raw(&self) -> SweepSurfaceLayout {
        match self {
            Self::ProfileFirst {
                secondary_kind,
                directions,
                origin,
                parameters,
                formulas,
            } => SweepSurfaceLayout::ProfileFirst {
                secondary_kind: *secondary_kind,
                directions: FiniteVector3::raw_array(*directions),
                origin: origin.get(),
                parameters: FiniteReal::raw_array(*parameters),
                formulas: Box::new(std::array::from_fn(|index| formulas[index].to_raw())),
            },
            Self::ExplicitFormula {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path_range,
                path_parameter,
                formula_flag,
                formula,
                trailing_flag,
            } => SweepSurfaceLayout::ExplicitFormula {
                mode: *mode,
                profile_range: FiniteReal::raw_array(*profile_range),
                profile_frame: raw_frame(*profile_frame),
                origin: origin.get(),
                directions: FiniteVector3::raw_array(*directions),
                trajectory_flag: *trajectory_flag,
                path_range: FiniteReal::raw_array(*path_range),
                path_parameter: path_parameter.get(),
                formula_flag: *formula_flag,
                formula: formula.to_raw(),
                trailing_flag: *trailing_flag,
            },
            Self::ExplicitGuide {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path_range,
                path_parameter,
                guide_flags,
                guide_curve,
                guide_range,
                guide_modes,
                guide_parameters,
                trailing_flags,
            } => SweepSurfaceLayout::ExplicitGuide {
                mode: *mode,
                profile_range: FiniteReal::raw_array(*profile_range),
                profile_frame: raw_frame(*profile_frame),
                origin: origin.get(),
                directions: FiniteVector3::raw_array(*directions),
                trajectory_flag: *trajectory_flag,
                path_range: FiniteReal::raw_array(*path_range),
                path_parameter: path_parameter.get(),
                guide_flags: *guide_flags,
                guide_curve: guide_curve.clone(),
                guide_range: FiniteReal::raw_array(*guide_range),
                guide_modes: *guide_modes,
                guide_parameters: FiniteReal::raw_array(*guide_parameters),
                trailing_flags: *trailing_flags,
            },
            Self::ExplicitSurface {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                trajectory_flag,
                path_range,
                path_parameter,
                singularity,
                support_surface,
                auxiliary_curve,
                support_flag,
                legacy_flag,
            } => SweepSurfaceLayout::ExplicitSurface {
                mode: *mode,
                profile_range: FiniteReal::raw_array(*profile_range),
                profile_frame: raw_frame(*profile_frame),
                origin: origin.get(),
                directions: FiniteVector3::raw_array(*directions),
                trajectory_flag: *trajectory_flag,
                path_range: FiniteReal::raw_array(*path_range),
                path_parameter: path_parameter.get(),
                singularity: *singularity,
                support_surface: support_surface.clone(),
                auxiliary_curve: auxiliary_curve.clone(),
                support_flag: *support_flag,
                legacy_flag: *legacy_flag,
            },
            Self::LawDriven {
                mode,
                profile_range,
                profile_frame,
                origin,
                directions,
                first_law,
                first_mode,
                first_range,
                law_direction,
                path_mode,
                path_flag,
                path_range,
                path_parameter,
                second_law_flag,
                second_law,
                formula_mode,
                formula,
                trailing_flag,
            } => SweepSurfaceLayout::LawDriven {
                mode: *mode,
                profile_range: FiniteReal::raw_array(*profile_range),
                profile_frame: raw_frame(*profile_frame),
                origin: origin.get(),
                directions: FiniteVector3::raw_array(*directions),
                first_law: Box::new(first_law.to_raw()),
                first_mode: *first_mode,
                first_range: FiniteReal::raw_array(*first_range),
                law_direction: law_direction.get(),
                path_mode: *path_mode,
                path_flag: *path_flag,
                path_range: FiniteReal::raw_array(*path_range),
                path_parameter: path_parameter.get(),
                second_law_flag: *second_law_flag,
                second_law: Box::new(second_law.to_raw()),
                formula_mode: *formula_mode,
                formula: formula.to_raw(),
                trailing_flag: *trailing_flag,
            },
        }
    }
}

impl SweepRevisionForm {
    /// The form with admitted endpoints and cache.
    fn admit(self) -> Option<SweepRevisionForm<FiniteReal>> {
        Some(SweepRevisionForm {
            revision: self.revision,
            primary_flag: self.primary_flag,
            profile_endpoints: FiniteReal::optional(self.profile_endpoints)?,
            path_endpoints: FiniteReal::optional(self.path_endpoints)?,
            cache: self.cache.admit()?,
        })
    }
}

impl SweepRevisionForm<FiniteReal> {
    /// The form with raw endpoints and cache.
    #[must_use]
    pub fn to_raw(&self) -> SweepRevisionForm {
        SweepRevisionForm {
            revision: self.revision,
            primary_flag: self.primary_flag,
            profile_endpoints: FiniteReal::raw_optional(self.profile_endpoints),
            path_endpoints: FiniteReal::raw_optional(self.path_endpoints),
            cache: self.cache.to_raw(),
        }
    }
}

impl SweepSurfaceConstruction {
    /// The construction with admitted cache, layout and discontinuities.
    pub(super) fn admit(
        self,
    ) -> Option<SweepSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(SweepSurfaceConstruction {
            primary_kind: self.primary_kind,
            cache: self.cache.admit_form(SweepRevisionForm::admit)?,
            layout: self.layout.admit()?,
            discontinuities: FiniteReal::lanes(self.discontinuities)?,
            discontinuity_flag: self.discontinuity_flag,
        })
    }
}

impl SweepSurfaceConstruction<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The construction with raw cache, layout and discontinuities.
    #[must_use]
    pub fn to_raw(&self) -> SweepSurfaceConstruction {
        SweepSurfaceConstruction {
            primary_kind: self.primary_kind,
            cache: self.cache.view_form(SweepRevisionForm::to_raw),
            layout: self.layout.to_raw(),
            discontinuities: FiniteReal::raw_lanes(&self.discontinuities),
            discontinuity_flag: self.discontinuity_flag,
        }
    }
}

impl CacheFirstCurveForm {
    /// The form with admitted cache, bounds and solved range.
    pub(super) fn admit(self) -> Option<CacheFirstCurveForm<FiniteReal>> {
        Some(CacheFirstCurveForm {
            revision: self.revision,
            cache: self.cache.admit()?,
            support_bounds: FiniteReal::optional_grid(self.support_bounds)?,
            solved_range: FiniteReal::optional(self.solved_range)?,
            extension: self.extension,
        })
    }
}

impl CacheFirstCurveForm<FiniteReal> {
    /// The form with raw cache, bounds and solved range.
    #[must_use]
    pub fn to_raw(&self) -> CacheFirstCurveForm {
        CacheFirstCurveForm {
            revision: self.revision,
            cache: self.cache.to_raw(),
            support_bounds: FiniteReal::raw_optional_grid(self.support_bounds),
            solved_range: FiniteReal::raw_optional(self.solved_range),
            extension: self.extension,
        }
    }
}

impl SpringSupport {
    /// The support slot with admitted replacement ranges.
    fn admit(self) -> Option<SpringSupport<FiniteReal>> {
        Some(match self {
            Self::Surface(surface) => SpringSupport::Surface(surface),
            Self::Ranges(ranges) => SpringSupport::Ranges(FiniteReal::grid(ranges)?),
        })
    }
}

impl SpringSupport<FiniteReal> {
    /// The support slot with raw replacement ranges.
    #[must_use]
    pub fn to_raw(&self) -> SpringSupport {
        match self {
            Self::Surface(surface) => SpringSupport::Surface(surface.clone()),
            Self::Ranges(ranges) => SpringSupport::Ranges(FiniteReal::raw_grid(*ranges)),
        }
    }
}

impl SpringPcurve {
    /// The pcurve slot with an admitted replacement range.
    fn admit(self) -> Option<SpringPcurve<FiniteReal>> {
        Some(match self {
            Self::Pcurve(pcurve) => SpringPcurve::Pcurve(pcurve),
            Self::Range(range) => SpringPcurve::Range(FiniteReal::array(range)?),
        })
    }
}

impl SpringPcurve<FiniteReal> {
    /// The pcurve slot with a raw replacement range.
    #[must_use]
    pub fn to_raw(&self) -> SpringPcurve {
        match self {
            Self::Pcurve(pcurve) => SpringPcurve::Pcurve(pcurve.clone()),
            Self::Range(range) => SpringPcurve::Range(FiniteReal::raw_array(*range)),
        }
    }
}

impl SpringLayout {
    /// The layout with admitted replacement ranges, interval and
    /// discontinuities, or admitted cache-first form.
    pub(super) fn admit(self) -> Option<SpringLayout<FiniteReal, ParameterInterval>> {
        Some(match self {
            Self::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                discontinuity_flag,
                cache,
            } => {
                let [first, second] = supports;
                SpringLayout::ContextFirst {
                    supports: [first.admit()?, second.admit()?],
                    first_pcurve: first_pcurve.admit()?,
                    second_pcurve,
                    parameter_range: ParameterInterval::new(parameter_range).ok()?,
                    discontinuities: FiniteReal::lanes(discontinuities)?,
                    discontinuity_flag,
                    cache,
                }
            }
            Self::CacheFirst { context, form } => SpringLayout::CacheFirst {
                context,
                form: form.admit()?,
            },
        })
    }
}

impl SpringLayout<FiniteReal, ParameterInterval> {
    /// The layout with raw replacement ranges, interval and discontinuities,
    /// or raw cache-first form.
    #[must_use]
    pub fn to_raw(&self) -> SpringLayout {
        match self {
            Self::ContextFirst {
                supports,
                first_pcurve,
                second_pcurve,
                parameter_range,
                discontinuities,
                discontinuity_flag,
                cache,
            } => SpringLayout::ContextFirst {
                supports: [supports[0].to_raw(), supports[1].to_raw()],
                first_pcurve: first_pcurve.to_raw(),
                second_pcurve: second_pcurve.clone(),
                parameter_range: parameter_range.endpoints(),
                discontinuities: FiniteReal::raw_lanes(discontinuities),
                discontinuity_flag: *discontinuity_flag,
                cache: *cache,
            },
            Self::CacheFirst { context, form } => SpringLayout::CacheFirst {
                context: context.clone(),
                form: form.to_raw(),
            },
        }
    }
}

impl ProjectionTail {
    /// The tail with an admitted range.
    pub(super) fn admit(self) -> Option<ProjectionTail<FiniteReal>> {
        Some(match self {
            Self::EarlyClose { flag } => ProjectionTail::EarlyClose { flag },
            Self::Ranged {
                flag,
                parameter_range,
                role,
            } => ProjectionTail::Ranged {
                flag,
                parameter_range: FiniteReal::array(parameter_range)?,
                role,
            },
        })
    }
}

impl ProjectionTail<FiniteReal> {
    /// The tail with a raw range.
    #[must_use]
    pub fn to_raw(&self) -> ProjectionTail {
        match self {
            Self::EarlyClose { flag } => ProjectionTail::EarlyClose { flag: *flag },
            Self::Ranged {
                flag,
                parameter_range,
                role,
            } => ProjectionTail::Ranged {
                flag: *flag,
                parameter_range: FiniteReal::raw_array(*parameter_range),
                role: *role,
            },
        }
    }
}

impl DeformableCurveData {
    /// The payload with admitted vectors, scalars and point.
    pub(super) fn admit(
        self,
    ) -> Option<DeformableCurveData<FiniteReal, FiniteVector3, FinitePoint3>> {
        Some(match self {
            Self::VectorField {
                vectors,
                parameter_pairs,
            } => DeformableCurveData::VectorField {
                vectors: FiniteVector3::array(vectors)?,
                parameter_pairs: FiniteReal::rows(parameter_pairs)?,
            },
            Self::Mode3 {
                leading_vectors,
                leading_parameter,
                leading_flags,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                frame_flags,
                parameters,
                trailing_flags,
                trailing_parameter,
                trailing_value,
            } => DeformableCurveData::Mode3 {
                leading_vectors: FiniteVector3::array(leading_vectors)?,
                leading_parameter: FiniteReal::new(leading_parameter)?,
                leading_flags,
                trailing_point: FinitePoint3::new(trailing_point)?,
                trailing_vectors: FiniteVector3::array(trailing_vectors)?,
                frame_parameter: FiniteReal::new(frame_parameter)?,
                frame_flags,
                parameters: FiniteReal::array(parameters)?,
                trailing_flags,
                trailing_parameter: FiniteReal::new(trailing_parameter)?,
                trailing_value,
            },
        })
    }
}

impl DeformableCurveData<FiniteReal, FiniteVector3, FinitePoint3> {
    /// The payload with raw vectors, scalars and point.
    #[must_use]
    pub fn to_raw(&self) -> DeformableCurveData {
        match self {
            Self::VectorField {
                vectors,
                parameter_pairs,
            } => DeformableCurveData::VectorField {
                vectors: FiniteVector3::raw_array(*vectors),
                parameter_pairs: FiniteReal::raw_rows(parameter_pairs),
            },
            Self::Mode3 {
                leading_vectors,
                leading_parameter,
                leading_flags,
                trailing_point,
                trailing_vectors,
                frame_parameter,
                frame_flags,
                parameters,
                trailing_flags,
                trailing_parameter,
                trailing_value,
            } => DeformableCurveData::Mode3 {
                leading_vectors: FiniteVector3::raw_array(*leading_vectors),
                leading_parameter: leading_parameter.get(),
                leading_flags: *leading_flags,
                trailing_point: trailing_point.get(),
                trailing_vectors: FiniteVector3::raw_array(*trailing_vectors),
                frame_parameter: frame_parameter.get(),
                frame_flags: *frame_flags,
                parameters: FiniteReal::raw_array(*parameters),
                trailing_flags: *trailing_flags,
                trailing_parameter: trailing_parameter.get(),
                trailing_value: *trailing_value,
            },
        }
    }
}

impl OffsetSide {
    /// The side with an admitted normal or direction.
    pub(super) fn admit(self) -> Option<OffsetSide<FiniteVector3>> {
        Some(match self {
            Self::PlaneNormal { normal } => OffsetSide::PlaneNormal {
                normal: FiniteVector3::new(normal)?,
            },
            Self::Direction { direction, support } => OffsetSide::Direction {
                direction: FiniteVector3::new(direction)?,
                support,
            },
        })
    }
}

impl OffsetSide<FiniteVector3> {
    /// The side with a raw normal or direction.
    #[must_use]
    pub fn to_raw(&self) -> OffsetSide {
        match self {
            Self::PlaneNormal { normal } => OffsetSide::PlaneNormal {
                normal: normal.get(),
            },
            Self::Direction { direction, support } => OffsetSide::Direction {
                direction: direction.get(),
                support: support.clone(),
            },
        }
    }
}

impl CurveOffsetDistanceLaw {
    /// The law with admitted distances and function-parameter map.
    fn admit(self) -> Option<CurveOffsetDistanceLaw<FiniteReal>> {
        Some(match self {
            Self::Linear {
                basis,
                distances,
                control_range,
            } => CurveOffsetDistanceLaw::Linear {
                basis,
                distances: FiniteReal::array(distances)?,
                control_range,
            },
            Self::Coordinate {
                function,
                coordinate,
                basis,
                function_parameter_offset,
                function_parameter_scale,
            } => CurveOffsetDistanceLaw::Coordinate {
                function,
                coordinate,
                basis,
                function_parameter_offset: FiniteReal::new(function_parameter_offset)?,
                function_parameter_scale: FiniteReal::new(function_parameter_scale)?,
            },
        })
    }
}

impl CurveOffsetDistanceLaw<FiniteReal> {
    /// The law with raw distances and function-parameter map.
    #[must_use]
    pub fn to_raw(&self) -> CurveOffsetDistanceLaw {
        match self {
            Self::Linear {
                basis,
                distances,
                control_range,
            } => CurveOffsetDistanceLaw::Linear {
                basis: *basis,
                distances: FiniteReal::raw_array(*distances),
                control_range: *control_range,
            },
            Self::Coordinate {
                function,
                coordinate,
                basis,
                function_parameter_offset,
                function_parameter_scale,
            } => CurveOffsetDistanceLaw::Coordinate {
                function: function.clone(),
                coordinate: *coordinate,
                basis: *basis,
                function_parameter_offset: function_parameter_offset.get(),
                function_parameter_scale: function_parameter_scale.get(),
            },
        }
    }
}

impl CurveOffsetRange {
    /// The range with an admitted distance law.
    pub(super) fn admit(self) -> Option<CurveOffsetRange<FiniteReal>> {
        Some(match self {
            Self::Uniform { parameter_range } => CurveOffsetRange::Uniform { parameter_range },
            Self::Variable {
                parameter_range,
                distance_law,
            } => CurveOffsetRange::Variable {
                parameter_range,
                distance_law: distance_law.admit()?,
            },
        })
    }
}

impl CurveOffsetRange<FiniteReal> {
    /// The range with a raw distance law.
    #[must_use]
    pub fn to_raw(&self) -> CurveOffsetRange {
        match self {
            Self::Uniform { parameter_range } => CurveOffsetRange::Uniform {
                parameter_range: *parameter_range,
            },
            Self::Variable {
                parameter_range,
                distance_law,
            } => CurveOffsetRange::Variable {
                parameter_range: *parameter_range,
                distance_law: distance_law.to_raw(),
            },
        }
    }
}
