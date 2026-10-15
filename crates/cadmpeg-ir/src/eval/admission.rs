// SPDX-License-Identifier: Apache-2.0
//! Recursion depth and local work slices for geometry evaluation.

use std::cell::Cell;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, DepthGuard, ResourceDimension, ResourceFailure, ResourceLimit,
    WorkBudget,
};

use super::EvaluationFailure;

const INDEPENDENT_EVALUATION_DEPTH: usize = 256;

/// Why an evaluation work charge was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationRefusal {
    /// The evaluation's local work slice ran out. The decode session continues.
    WorkExhausted,
    /// The decode session refused. The session keeps this refusal.
    ResourceLimit(ResourceLimit),
}

impl From<ResourceLimit> for EvaluationRefusal {
    fn from(limit: ResourceLimit) -> Self {
        Self::ResourceLimit(limit)
    }
}

impl EvaluationRefusal {
    /// No value for an exhausted work slice; the session refusal otherwise.
    pub(super) fn absent<T>(self) -> Result<Option<T>, ResourceLimit> {
        match self {
            Self::WorkExhausted => Ok(None),
            Self::ResourceLimit(limit) => Err(limit),
        }
    }
}

#[cfg(test)]
impl EvaluationRefusal {
    /// The session refusal of an evaluation that runs without a work slice.
    pub(super) fn into_limit(self) -> ResourceLimit {
        match self {
            Self::ResourceLimit(limit) => limit,
            Self::WorkExhausted => panic!("an evaluation without a work slice exhausted one"),
        }
    }
}

/// Evaluation nests under the caller's decode session or an independent depth limit.
#[derive(Clone, Copy)]
pub enum EvaluationAdmission<'ctx, 'arena> {
    /// Count nesting against this live decode context.
    Decode(&'ctx DecodeContext<'arena>),
    /// Use an independent depth limit of 256.
    Standard,
    /// Evaluate within a work slice established by `within_work_slice`.
    WorkSlice(EvaluationWorkSlice<'ctx, 'arena>),
}

/// A local work slice bound to its recursion policy.
#[derive(Clone, Copy)]
pub struct EvaluationWorkSlice<'ctx, 'arena> {
    policy: WorkSlicePolicy<'ctx, 'arena>,
}

/// A session slice counts each evaluation step; an independent slice counts
/// the stored representation's evaluation cost.
#[derive(Clone, Copy)]
enum WorkSlicePolicy<'ctx, 'arena> {
    Independent(&'ctx WorkBudget<'ctx>),
    Session {
        context: &'ctx DecodeContext<'arena>,
        work: &'ctx WorkBudget<'ctx>,
    },
}

impl<'ctx, 'arena> From<&'ctx DecodeContext<'arena>> for EvaluationAdmission<'ctx, 'arena> {
    fn from(context: &'ctx DecodeContext<'arena>) -> Self {
        Self::Decode(context)
    }
}

impl<'ctx, 'arena> EvaluationAdmission<'ctx, 'arena> {
    /// Model carriers share the caller's depth, cycle path and work slice.
    pub(super) fn within_model<T, R>(
        self,
        run: impl for<'frame> FnOnce(
            EvaluationAdmission<'frame, 'frame>,
        ) -> Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        let evaluate = |admission: EvaluationAdmission<'_, '_>| {
            let scratch = super::decode::Scratch::new(admission);
            scratch.unless_refused()?;
            let result = run(admission);
            let result = match admission.work_slice() {
                Some(_) => super::ModelEvaluationDepthGuard::finish_budgeted(admission, result),
                None => result,
            };
            scratch.settle(result)
        };
        if let Self::Decode(_) = self {
            let work = WorkBudget::new(usize::MAX);
            self.within_work_slice(&work, evaluate)
        } else {
            evaluate(self)
        }
    }

    pub(super) fn work_slice(self) -> Option<&'ctx WorkBudget<'ctx>> {
        match self {
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Independent(work) | WorkSlicePolicy::Session { work, .. },
            }) => Some(work),
            Self::Decode(_) | Self::Standard => None,
        }
    }

    /// Each model carrier consumes one work step before reading its payload.
    /// An exhausted slice is [`EvaluationFailure::WorkExhausted`] unless the
    /// session refused behind it.
    pub(super) fn model_step<R>(self) -> Result<(), EvaluationFailure<R>> {
        match self.work_slice() {
            Some(work) if !work.charge() => Err(self.exhausted().into()),
            _ => Ok(()),
        }
    }

    /// Run one evaluation under a bounded slice of `parent`. The child's
    /// consumption transfers to the parent once.
    ///
    /// A slice that runs out leaves the parent exhausted and the decode
    /// session unrefused; the evaluation reports
    /// [`EvaluationFailure::WorkExhausted`]. A session refusal before or
    /// after the evaluation is its result.
    pub fn within_work_slice<T, R>(
        self,
        parent: &WorkBudget<'_>,
        run: impl for<'slice> FnOnce(
            EvaluationAdmission<'slice, 'slice>,
        ) -> Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        match self.context() {
            Some(context) => {
                session_refusal(context)?;
                let child = WorkBudget::new(parent.remaining());
                let result = run(EvaluationAdmission::WorkSlice(EvaluationWorkSlice {
                    policy: WorkSlicePolicy::Session {
                        context,
                        work: &child,
                    },
                }));
                // A transfer that does not fit exhausts the parent; a child
                // that ran out leaves the parent exhausted as well.
                if parent.consume_child(&child).is_ok() && child.exhausted() {
                    parent.exhaust();
                }
                session_refusal(context)?;
                result
            }
            None => run(EvaluationAdmission::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Independent(parent),
            })),
        }
    }

    pub(super) fn context(self) -> Option<&'ctx DecodeContext<'arena>> {
        match self {
            Self::Decode(context) => Some(context),
            Self::Standard => None,
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Session { context, .. },
            }) => Some(context),
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Independent(_),
            }) => None,
        }
    }

    pub(super) fn independent_cost<R>(
        self,
        cost: Option<usize>,
    ) -> Result<(), EvaluationFailure<R>> {
        match self {
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Independent(work),
            }) => cost
                .is_some_and(|cost| work.charge_by(cost))
                .then_some(())
                .ok_or(EvaluationFailure::WorkExhausted),
            _ => Ok(()),
        }
    }

    /// Count evaluation steps against a session slice. A slice that cannot
    /// cover them is [`EvaluationRefusal::WorkExhausted`] unless the session
    /// refused behind it; the session stays unrefused.
    pub(super) fn work(self, count: usize) -> Result<(), EvaluationRefusal> {
        match self {
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Session { work, .. },
            }) if !work.charge_by(count) => Err(self.exhausted()),
            _ => Ok(()),
        }
    }

    /// The refusal this admission already holds: the session's refusal, or
    /// an exhausted session slice.
    pub(super) fn refusal(self) -> Option<EvaluationRefusal> {
        let refusal = self
            .context()
            .and_then(DecodeContext::resource_refusal)
            .map(EvaluationRefusal::ResourceLimit);
        match self {
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Session { work, .. },
            }) if work.exhausted() => refusal.or(Some(EvaluationRefusal::WorkExhausted)),
            _ => refusal,
        }
    }

    /// An exhausted slice, or the session refusal behind it.
    fn exhausted(self) -> EvaluationRefusal {
        self.context()
            .and_then(DecodeContext::resource_refusal)
            .map_or(
                EvaluationRefusal::WorkExhausted,
                EvaluationRefusal::ResourceLimit,
            )
    }

    pub(super) fn enter<'scratch>(
        self,
        depth: &'scratch Cell<usize>,
    ) -> Result<EvaluationDepthGuard<'scratch>, ResourceLimit>
    where
        'ctx: 'scratch,
    {
        match self.context() {
            Some(context) => context
                .enter_nested_limit("geometry evaluation nesting")
                .map(|guard| EvaluationDepthGuard::Decode { _guard: guard }),
            None => {
                if depth.get() >= INDEPENDENT_EVALUATION_DEPTH {
                    return Err(ResourceLimit {
                        dimension: ResourceDimension::RecursionDepth,
                        reason: ResourceFailure::BudgetExceeded,
                        limit: u64_from_index(INDEPENDENT_EVALUATION_DEPTH),
                        used: u64_from_index(depth.get()),
                        additional: 1,
                        operation: "independent geometry evaluation nesting",
                    });
                }
                depth.set(depth.get() + 1);
                Ok(EvaluationDepthGuard::Standard { depth })
            }
        }
    }
}

fn session_refusal<R>(context: &DecodeContext<'_>) -> Result<(), EvaluationFailure<R>> {
    context
        .resource_refusal()
        .map_or(Ok(()), |limit| Err(EvaluationFailure::ResourceLimit(limit)))
}

impl crate::ids::comparison::sealed::TextWork for EvaluationAdmission<'_, '_> {}

impl crate::ids::comparison::TextWork for EvaluationAdmission<'_, '_> {
    type Error = EvaluationRefusal;
    fn comparison_work(
        &self,
        count: u64,
        _operation: &'static str,
    ) -> Result<(), EvaluationRefusal> {
        usize::try_from(count).map_or(Err(EvaluationRefusal::WorkExhausted), |count| {
            self.work(count)
        })
    }
}

impl crate::index::sealed::IndexQuery for EvaluationAdmission<'_, '_> {}

impl crate::index::IndexQuery for EvaluationAdmission<'_, '_> {
    type Error = EvaluationRefusal;
    type Output<T> = Result<T, EvaluationRefusal>;
    fn finish<T>(&self, result: Result<T, EvaluationRefusal>) -> Self::Output<T> {
        result
    }
    fn lazy(&self) -> bool {
        self.context().is_none()
    }
    fn work(&self, count: usize, _operation: &'static str) -> Result<(), EvaluationRefusal> {
        EvaluationAdmission::work(*self, count)
    }
    fn equal(
        &self,
        first: &str,
        second: &str,
        operation: &'static str,
    ) -> Result<bool, EvaluationRefusal> {
        crate::ids::comparison::equal(self, first, second, operation)
    }
}

pub(super) enum EvaluationDepthGuard<'scratch> {
    Decode { _guard: DepthGuard<'scratch> },
    Standard { depth: &'scratch Cell<usize> },
}

impl Drop for EvaluationDepthGuard<'_> {
    fn drop(&mut self) {
        if let Self::Standard { depth } = self {
            depth.set(depth.get() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EvaluationAdmission;
    use crate::eval::EvaluationFailure;
    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget,
    };
    use cadmpeg_core::CodecError;

    #[test]
    fn model_text_comparison_exhausts_only_its_local_slice() {
        // equal("alpha", "alpha") takes one length step and five byte steps.
        for allowance in 0..=6_usize {
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = WorkBudget::new(allowance);
            let result: Result<bool, EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
                .within_work_slice(&parent, |admission| {
                    crate::index::IndexQuery::equal(
                        &admission,
                        "alpha",
                        "alpha",
                        "model text comparison",
                    )
                    .map_err(EvaluationFailure::from)
                });
            assert_eq!(parent.consumed(), allowance);
            if allowance < 6 {
                // The exhausted slice is local: the session keeps running.
                assert_eq!(result, Err(EvaluationFailure::WorkExhausted));
                assert!(parent.exhausted());
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                assert_eq!(result, Ok(true));
            }
            assert!(ctx.finish_session().is_ok());
        }
        assert_eq!(
            crate::ids::comparison::compare(
                &EvaluationAdmission::Standard,
                "é",
                "ê",
                "standard text comparison"
            )
            .unwrap(),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn model_entries_return_the_session_depth_refusal() {
        use crate::geometry::{
            Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
            SurfaceGeometry,
        };
        use crate::math::{Point3, Vector3};
        let mut ir = crate::CadIr::empty();
        let curve = crate::ids::CurveId::mint("test:model:curve#line").unwrap();
        let surface = crate::ids::SurfaceId::mint("test:model:surface#plane").unwrap();
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                crate::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
        for route in 0..4 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let evaluate = || match route {
                0 => crate::eval::model_curve_point_by_id(
                    EvaluationAdmission::Decode(&ctx),
                    &index,
                    &curve,
                    0.5,
                ),
                1 => crate::eval::model_surface_point_by_id(
                    EvaluationAdmission::Decode(&ctx),
                    &index,
                    &surface,
                    0.5,
                    0.5,
                ),
                2 => crate::eval::model_surface_partials_by_id(
                    EvaluationAdmission::Decode(&ctx),
                    &index,
                    &surface,
                    0.5,
                    0.5,
                )
                .map(|partials| partials.point),
                _ => crate::eval::model_surface_point(
                    EvaluationAdmission::Decode(&ctx),
                    &ir,
                    &ir.model.surfaces[0].geometry,
                    0.5,
                    0.5,
                ),
            };
            let EvaluationFailure::ResourceLimit(first) = evaluate().unwrap_err() else {
                panic!("a model depth refusal stays a resource error");
            };
            assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
            assert_eq!((first.limit, first.used), (0, 0));
            assert!(first.additional > 0);
            assert_eq!(evaluate(), Err(EvaluationFailure::ResourceLimit(first)));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }

    #[test]
    fn model_work_slice_keeps_cycles_distinct_from_resource_refusal() {
        use crate::geometry::{
            Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
        };
        let mut ir = crate::CadIr::empty();
        let id = crate::ids::CurveId::mint("test:model:curve#cycle").unwrap();
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        ir.model
            .add_procedural_curve(
                &crate::document::admission::StandardAdmission,
                &id,
                ProceduralCurve::new(
                    crate::ids::ProceduralCurveId::mint("test:model:procedural#cycle").unwrap(),
                    ProceduralCurveDefinition::Replica {
                        source: id.clone(),
                        transform: crate::transform::Transform::identity(),
                    },
                ),
            )
            .unwrap()
            .unwrap();
        let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let parent = WorkBudget::new(100_000);
        assert_eq!(
            EvaluationAdmission::Decode(&ctx).within_work_slice(&parent, |admission| {
                crate::eval::model_curve_point_by_id(admission, &index, &id, 0.5)
            }),
            Err(EvaluationFailure::NoValue)
        );
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn exhausted_work_slice_leaves_the_session_unrefused() {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let parent = WorkBudget::new(0);
        let result: Result<(), EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
            .within_work_slice(&parent, |admission| {
                admission.work(1).map_err(EvaluationFailure::from)
            });
        assert_eq!(result, Err(EvaluationFailure::WorkExhausted));
        assert!(parent.exhausted());
        assert_eq!(ctx.resource_refusal(), None);
        // The next evaluation runs; its own slice is the exhausted remainder.
        let called = std::cell::Cell::new(false);
        let repeated: Result<(), EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
            .within_work_slice(&parent, |admission| {
                called.set(true);
                admission.work(1).map_err(EvaluationFailure::from)
            });
        assert!(called.get());
        assert_eq!(repeated, Err(EvaluationFailure::WorkExhausted));
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn work_slice_preserves_active_session_depth() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let outer = ctx.enter_nested_limit("test outer evaluation").unwrap();
        let parent = WorkBudget::new(10);
        let depth = std::cell::Cell::new(0);
        let result: Result<(), EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
            .within_work_slice(&parent, |admission| {
                admission
                    .enter(&depth)
                    .map(|_guard| ())
                    .map_err(EvaluationFailure::ResourceLimit)
            });
        let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
            panic!("a child slice shares active depth");
        };
        assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
        assert_eq!((first.limit, first.used, first.additional), (1, 1, 1));
        drop(outer);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
        );
    }
}
