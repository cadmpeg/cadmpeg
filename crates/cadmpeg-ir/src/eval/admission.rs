// SPDX-License-Identifier: Apache-2.0
//! Resource policy for geometry evaluation.

use std::cell::Cell;

use cadmpeg_core::decode::{
    u64_from_index, work_units, DecodeContext, DepthGuard, ResourceDimension, ResourceFailure,
    ResourceLimit, WorkBudget,
};

use super::EvaluationFailure;

const INDEPENDENT_EVALUATION_DEPTH: usize = 256;

/// Evaluation uses the caller's decode session or standard-library storage.
#[derive(Clone, Copy)]
pub enum EvaluationAdmission<'ctx, 'arena> {
    /// Charge storage, work and nesting to this live decode context.
    Decode(&'ctx DecodeContext<'arena>),
    /// Use fallible standard-library storage and an independent depth limit of 256.
    Standard,
    /// Evaluate within a work slice established by `within_work_slice`.
    WorkSlice(EvaluationWorkSlice<'ctx, 'arena>),
}

/// A work slice bound to its allocation and recursion policy.
#[derive(Clone, Copy)]
pub struct EvaluationWorkSlice<'ctx, 'arena> {
    policy: WorkSlicePolicy<'ctx, 'arena>,
}

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
    /// Model carriers share the caller's depth, cycle path and scratch policy.
    pub(super) fn within_model<T, R>(
        self,
        run: impl for<'frame> FnOnce(
            EvaluationAdmission<'frame, 'frame>,
        ) -> Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        let evaluate = |admission: EvaluationAdmission<'_, '_>| {
            let scratch = super::decode::Scratch::new(admission);
            scratch
                .unless_refused()
                .map_err(EvaluationFailure::ResourceLimit)?;
            let result = run(admission);
            let result = match admission.work_slice() {
                Some(_) => super::ModelEvaluationDepthGuard::finish_budgeted(admission, result),
                None => result,
            };
            scratch.settle(result)
        };
        if let Self::Decode(context) = self {
            let work = context.work_budget(u64_from_index(usize::MAX));
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
    pub(super) fn model_step<R>(self) -> Result<(), EvaluationFailure<R>> {
        match self.work_slice() {
            Some(work) => {
                let remaining = u64_from_index(work.remaining());
                if work.charge() {
                    return Ok(());
                }
                if let Some(context) = self.context() {
                    context
                        .charge_work_limit(0, "model evaluation work slice")
                        .map_err(EvaluationFailure::ResourceLimit)?;
                    drop(context.refuse_codec_limit("model evaluation work slice", remaining, 1));
                    context
                        .charge_work_limit(0, "model evaluation work slice")
                        .map_err(EvaluationFailure::ResourceLimit)?;
                }
                Err(EvaluationFailure::NoValue)
            }
            None => self
                .work(1, "model evaluation work step")
                .map_err(EvaluationFailure::ResourceLimit),
        }
    }

    /// Run one evaluation under a bounded slice. Session work is charged by
    /// the child as it occurs; consumption transfers to the parent once.
    /// Independent slices charge the stored representation's work cost.
    pub fn within_work_slice<T, R>(
        self,
        parent: &WorkBudget<'_>,
        run: impl for<'slice> FnOnce(
            EvaluationAdmission<'slice, 'slice>,
        ) -> Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        let _boundary = parent
            .reserve_scratch(0, "geometry work slice boundary")
            .map_err(EvaluationFailure::ResourceLimit)?;
        match self.context() {
            Some(context) => {
                context
                    .charge_work_limit(0, "geometry work slice boundary")
                    .map_err(EvaluationFailure::ResourceLimit)?;
                let child = context.work_budget(u64_from_index(parent.remaining()));
                let result = run(EvaluationAdmission::WorkSlice(EvaluationWorkSlice {
                    policy: WorkSlicePolicy::Session {
                        context,
                        work: &child,
                    },
                }));
                let remaining = u64_from_index(parent.remaining());
                if parent.consume_child(&child).is_err() {
                    drop(context.refuse_codec_limit(
                        "geometry work slice transfer",
                        remaining,
                        u64_from_index(work_units(child.consumed())),
                    ));
                }
                context
                    .charge_work_limit(0, "geometry work slice boundary")
                    .map_err(EvaluationFailure::ResourceLimit)?;
                result
            }
            None => {
                let result = run(EvaluationAdmission::WorkSlice(EvaluationWorkSlice {
                    policy: WorkSlicePolicy::Independent(parent),
                }));
                let _boundary = parent
                    .reserve_scratch(0, "geometry work slice boundary")
                    .map_err(EvaluationFailure::ResourceLimit)?;
                result
            }
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
                .ok_or(EvaluationFailure::NoValue),
            _ => Ok(()),
        }
    }

    pub(super) fn work(self, count: u64, operation: &'static str) -> Result<(), ResourceLimit> {
        match self {
            Self::Decode(context) => context.charge_work_limit(count, operation),
            Self::Standard => Ok(()),
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Independent(_),
            }) => Ok(()),
            Self::WorkSlice(EvaluationWorkSlice {
                policy: WorkSlicePolicy::Session { context, work },
            }) => {
                context.charge_work_limit(0, operation)?;
                let remaining = u64_from_index(work.remaining());
                if !usize::try_from(count).is_ok_and(|count| work.charge_by(count)) {
                    context.charge_work_limit(0, operation)?;
                    drop(context.refuse_codec_limit(
                        "geometry evaluation work slice",
                        remaining,
                        count,
                    ));
                    return context.charge_work_limit(0, operation);
                }
                Ok(())
            }
        }
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

impl crate::ids::comparison::sealed::TextWork for EvaluationAdmission<'_, '_> {}

impl crate::ids::comparison::TextWork for EvaluationAdmission<'_, '_> {
    type Error = ResourceLimit;
    fn comparison_work(&self, count: u64, operation: &'static str) -> Result<(), ResourceLimit> {
        self.work(count, operation)
    }
}

impl crate::index::sealed::IndexQuery for EvaluationAdmission<'_, '_> {}

impl crate::index::IndexQuery for EvaluationAdmission<'_, '_> {
    type Error = ResourceLimit;
    type Output<T> = Result<T, ResourceLimit>;
    type Iter<S: cadmpeg_core::decode::iter_source::IterSource> = std::iter::Chain<
        std::iter::Flatten<
            std::option::IntoIter<cadmpeg_core::decode::scan::AdmittedIter<S::Iter>>,
        >,
        std::iter::Flatten<std::option::IntoIter<S::Iter>>,
    >;
    fn admit_iter<S: cadmpeg_core::decode::iter_source::IterSource>(
        &self,
        source: S,
        operation: &'static str,
    ) -> Result<Self::Iter<S>, Self::Error> {
        let bound = source.visit_bound();
        // Core admission refuses overflowing source bounds before iteration.
        let context = match self {
            Self::Decode(ctx) => Some(*ctx),
            _ if bound.is_err() => self.context(),
            _ => None,
        };
        let (decode, standard) = if let Some(ctx) = context {
            (Some(ctx.admit_iter(source, operation)?), None)
        } else {
            if let Ok(bound) = bound {
                EvaluationAdmission::work(*self, bound, operation)?;
            }
            (None, Some(source.source_iter()))
        };
        Ok(decode
            .into_iter()
            .flatten()
            .chain(standard.into_iter().flatten()))
    }
    fn finish<T>(&self, result: Result<T, ResourceLimit>) -> Self::Output<T> {
        result
    }
    fn lazy(&self) -> bool {
        self.context().is_none()
    }
    fn work(&self, count: usize, operation: &'static str) -> Result<(), ResourceLimit> {
        EvaluationAdmission::work(*self, u64_from_index(count), operation)
    }
    fn equal(
        &self,
        first: &str,
        second: &str,
        operation: &'static str,
    ) -> Result<bool, ResourceLimit> {
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
    fn index_iteration_charges_decode_and_session_slice_work_once() {
        let source = [11, 13, 17];
        // The source has three visits and needs no decode storage.
        for sliced in [false, true] {
            for cap in 0..=3 {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let parent = ctx.work_budget(3);
                let iterate =
                    |admission: EvaluationAdmission<'_, '_>| -> Result<(), EvaluationFailure<()>> {
                        let values = crate::index::IndexQuery::admit_iter(
                            &admission,
                            &source,
                            "test index source visits",
                        )
                        .map_err(EvaluationFailure::ResourceLimit)?;
                        assert!(values.copied().eq(source));
                        Ok(())
                    };
                let admission = EvaluationAdmission::Decode(&ctx);
                let result = if sliced {
                    admission.within_work_slice(&parent, iterate)
                } else {
                    iterate(admission)
                };
                if sliced {
                    assert_eq!(parent.consumed(), 3);
                }
                let original = if cap < 3 {
                    let EvaluationFailure::ResourceLimit(original) = result.unwrap_err() else {
                        panic!("source visit refusal");
                    };
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(
                        (original.limit, original.used, original.additional),
                        (cap, 0, 3)
                    );
                    assert_eq!(
                        original.operation,
                        if sliced {
                            "work_budget"
                        } else {
                            "test index source visits"
                        }
                    );
                    original
                } else {
                    assert_eq!(result, Ok(()));
                    let original = ctx
                        .charge_work_limit(1, "test next source visit")
                        .unwrap_err();
                    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(
                        (original.limit, original.used, original.additional),
                        (3, 3, 1)
                    );
                    original
                };
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
            }
        }
    }

    #[test]
    fn index_iteration_preserves_session_refusals_for_overflowing_visit_bounds() {
        for sliced in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = u64::MAX;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = ctx.work_budget(3);
            // The inclusive range has u64::MAX + 1 visits; core saturates the
            // refused request to u64::MAX without advancing its source.
            let iterate =
                |admission: EvaluationAdmission<'_, '_>| -> Result<(), EvaluationFailure<()>> {
                    crate::index::IndexQuery::admit_iter(
                        &admission,
                        0_u64..=u64::MAX,
                        "test overflowing index source",
                    )
                    .map(|_| ())
                    .map_err(EvaluationFailure::ResourceLimit)
                };
            let admission = EvaluationAdmission::Decode(&ctx);
            let result = if sliced {
                admission.within_work_slice(&parent, iterate)
            } else {
                iterate(admission)
            };
            let EvaluationFailure::ResourceLimit(original) = result.unwrap_err() else {
                panic!("source bound overflow must preserve its refusal");
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(
                original.reason,
                cadmpeg_core::decode::ResourceFailure::BudgetExceeded
            );
            assert_eq!(
                (original.limit, original.used, original.additional),
                (u64::MAX, 0, u64::MAX)
            );
            assert_eq!(original.operation, "test overflowing index source");
            assert_eq!(parent.consumed(), 0);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
            );
        }
    }

    #[test]
    fn model_text_comparison_consumes_its_local_slice_and_global_work_once() {
        for allowance in 0..=6 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 6;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = ctx.work_budget(allowance);
            let result: Result<bool, EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
                .within_work_slice(&parent, |admission| {
                    crate::index::IndexQuery::equal(
                        &admission,
                        "alpha",
                        "alpha",
                        "model text comparison",
                    )
                    .map_err(EvaluationFailure::ResourceLimit)
                });
            assert_eq!(parent.consumed(), usize::try_from(allowance).unwrap());
            if allowance < 6 {
                let EvaluationFailure::ResourceLimit(original) = result.unwrap_err() else {
                    panic!("text comparison must preserve its local refusal");
                };
                assert_eq!(
                    original.dimension,
                    ResourceDimension::Codec("geometry evaluation work slice")
                );
                assert_eq!(
                    (original.limit, original.used, original.additional),
                    (0, 0, 1)
                );
                assert_eq!(
                    crate::ids::comparison::equal(&ctx, "", "", "repeated text comparison"),
                    Err(original)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
            } else {
                assert_eq!(result, Ok(true));
                let original = ctx
                    .charge_work_limit(1, "next model text comparison")
                    .unwrap_err();
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(
                    (original.limit, original.used, original.additional),
                    (6, 6, 1)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
            }
        }
        let standard = EvaluationAdmission::Standard;
        assert_eq!(
            crate::ids::comparison::compare(&standard, "é", "ê", "standard text comparison")
                .unwrap(),
            std::cmp::Ordering::Less
        );
        assert!(crate::ids::comparison::equal(
            &crate::index::StandardIndex,
            "alpha",
            "alpha",
            "standard text equality"
        )
        .unwrap());
    }

    #[test]
    fn model_lookup_work_refusal_reaches_curve_and_surface_evaluation() {
        use crate::geometry::{
            Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
            SurfaceGeometry,
        };
        let mut ir = crate::CadIr::empty();
        let curve = crate::ids::CurveId::mint("test:model:curve#unknown").unwrap();
        let surface = crate::ids::SurfaceId::mint("test:model:surface#unknown").unwrap();
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        let index = crate::index::ModelIndex::new_model_only(&ir, crate::index::StandardIndex);
        assert!(index
            .curves(curve.as_str(), crate::index::StandardIndex)
            .is_some());
        assert!(index
            .surfaces(surface.as_str(), crate::index::StandardIndex)
            .is_some());
        let frame_work = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Option<crate::eval::ModelEvaluationIdentity>,
        >());
        for surface_route in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = frame_work.checked_add(1).unwrap();
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let evaluate = || {
                if surface_route {
                    crate::eval::model_surface_point_by_id(
                        EvaluationAdmission::Decode(&ctx),
                        &index,
                        &surface,
                        0.5,
                        0.5,
                    )
                } else {
                    crate::eval::model_curve_point_by_id(
                        EvaluationAdmission::Decode(&ctx),
                        &index,
                        &curve,
                        0.5,
                    )
                }
            };
            let EvaluationFailure::ResourceLimit(first) = evaluate().unwrap_err() else {
                panic!("lookup work refusal is an evaluation resource error");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            // Curve lookup precedes its model step; surface lookup follows it.
            let expected_used = frame_work.checked_add(u64::from(surface_route)).unwrap();
            let query = if surface_route {
                surface.as_str()
            } else {
                curve.as_str()
            };
            assert_eq!(
                (first.limit, first.used, first.additional),
                (
                    policy.limits.max_work_units,
                    expected_used,
                    cadmpeg_core::decode::u64_from_index(query.len())
                )
            );
            assert_eq!(evaluate(), Err(EvaluationFailure::ResourceLimit(first)));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }

    #[test]
    fn model_entries_use_the_live_resource_policy() {
        use crate::geometry::{
            Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
            SurfaceGeometry,
        };
        use crate::math::{Point3, Vector3};
        let mut ir = crate::CadIr::empty();
        let curve = crate::ids::CurveId::mint("test:model:curve#line").unwrap();
        let surface = crate::ids::SurfaceId::mint("test:model:surface#plane").unwrap();
        ir.model.curves.push(Curve {
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
        for trigger in 0..5 {
            for route in 0..4 {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = 4096;
                let dimension = match trigger {
                    0 => {
                        policy.limits.max_materialized_bytes = 0;
                        ResourceDimension::MaterializedBytes
                    }
                    1 => {
                        policy.limits.max_collection_items = 0;
                        ResourceDimension::CollectionItems
                    }
                    2 => {
                        policy.limits.max_work_units = 0;
                        ResourceDimension::WorkUnits
                    }
                    3 => {
                        policy.limits.max_recursion_depth = 0;
                        ResourceDimension::RecursionDepth
                    }
                    _ => {
                        policy.limits.max_retained_bytes = 0;
                        ResourceDimension::RetainedBytes
                    }
                };
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
                let result = evaluate();
                if trigger == 4 {
                    assert!(result.is_ok());
                    let storage = ctx
                        .reserve_scoped_limit(
                            policy.limits.max_materialized_bytes,
                            "test released model path",
                        )
                        .unwrap();
                    drop(storage);
                    assert!(ctx.finish_session().is_ok());
                } else {
                    let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                        panic!("a model admission refusal stays a resource error");
                    };
                    assert_eq!(first.dimension, dimension);
                    assert_eq!((first.limit, first.used), (0, 0));
                    assert!(first.additional > 0);
                    assert_eq!(evaluate(), Err(EvaluationFailure::ResourceLimit(first)));
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
                    );
                }
            }
        }
    }

    #[test]
    fn model_child_nurbs_scratch_keeps_the_live_materialization_refusal() {
        use crate::geometry::nurbs::NurbsCurve;
        use crate::geometry::{
            Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
            SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
        };
        use crate::math::{Point3, Vector3};
        let mut ir = crate::CadIr::empty();
        let curve = crate::ids::CurveId::mint("test:model:curve#directrix").unwrap();
        let surface = crate::ids::SurfaceId::mint("test:model:surface#sweep").unwrap();
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(0.5, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                    ],
                    None,
                    false,
                )
                .unwrap()
                .unwrap(),
            )),
            source_object: None,
        });
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        ir.model
            .add_procedural_surface(
                &crate::document::admission::StandardAdmission,
                &surface,
                ProceduralSurface::new(
                    crate::ids::ProceduralSurfaceId::mint("test:model:procedural#sweep").unwrap(),
                    ProceduralSurfaceDefinition::LinearSweep(
                        crate::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                            curve,
                            Vector3::new(0.0, 0.0, 1.0),
                        )
                        .unwrap(),
                    ),
                    None,
                ),
            )
            .unwrap()
            .unwrap();
        let index = crate::index::ModelIndex::build(&ir, crate::index::StandardIndex);
        assert_eq!(
            crate::eval::model_surface_point_by_id(
                EvaluationAdmission::Standard,
                &index,
                &surface,
                0.25,
                2.0
            )
            .unwrap()
            .get(),
            Point3::new(0.25, 0.0, 2.0)
        );
        let frame_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Option<super::super::ModelEvaluationIdentity>,
        >());
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 3 * frame_bytes;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let evaluate = || {
            crate::eval::model_surface_point_by_id(
                EvaluationAdmission::Decode(&ctx),
                &index,
                &surface,
                0.25,
                2.0,
            )
        };
        let EvaluationFailure::ResourceLimit(first) = evaluate().unwrap_err() else {
            panic!("child NURBS scratch must use the model session");
        };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.limit, 3 * frame_bytes);
        assert_eq!(first.used, 3 * frame_bytes);
        // The core amortized reservation starts the three-value basis at four slots.
        assert_eq!(
            first.additional,
            cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<f64>())
        );
        assert_eq!(evaluate(), Err(EvaluationFailure::ResourceLimit(first)));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
        );
    }

    #[test]
    fn model_work_slice_keeps_cycles_distinct_from_resource_refusal() {
        use crate::geometry::{
            Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
        };
        let mut ir = crate::CadIr::empty();
        let id = crate::ids::CurveId::mint("test:model:curve#cycle").unwrap();
        ir.model.curves.push(Curve {
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
        let parent = ctx.work_budget(100_000);
        assert_eq!(
            EvaluationAdmission::Decode(&ctx).within_work_slice(&parent, |admission| {
                crate::eval::model_curve_point_by_id(admission, &index, &id, 0.5)
            }),
            Err(EvaluationFailure::NoValue)
        );
        assert!(ctx.finish_session().is_ok());
    }

    #[test]
    fn work_slice_transfers_actual_session_work_once() {
        for attached in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 2;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = if attached {
                ctx.work_budget(10)
            } else {
                WorkBudget::new(10)
            };
            let result: Result<(), EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
                .within_work_slice(&parent, |admission| {
                    admission
                        .work(2, "test numerical work")
                        .map_err(EvaluationFailure::ResourceLimit)
                });
            assert_eq!(result, Ok(()));
            assert_eq!(parent.consumed(), 2);
            let first = ctx
                .charge_work_limit(1, "test next numerical work")
                .unwrap_err();
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!((first.limit, first.used, first.additional), (2, 2, 1));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }

    #[test]
    fn work_slice_preserves_global_and_local_refusals() {
        for local in [false, true] {
            let mut policy = DecodePolicy::service();
            if !local {
                policy.limits.max_work_units = 0;
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = ctx.work_budget(if local { 0 } else { 10 });
            let result: Result<(), EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
                .within_work_slice(&parent, |admission| {
                    admission
                        .work(1, "test numerical work")
                        .map_err(EvaluationFailure::ResourceLimit)
                });
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("a work refusal is a resource error");
            };
            let dimension = if local {
                ResourceDimension::Codec("geometry evaluation work slice")
            } else {
                ResourceDimension::WorkUnits
            };
            assert_eq!(first.dimension, dimension);
            assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
            let called = std::cell::Cell::new(false);
            let repeated: Result<(), EvaluationFailure<()>> = EvaluationAdmission::Decode(&ctx)
                .within_work_slice(&parent, |_| {
                    called.set(true);
                    Err(EvaluationFailure::NoValue)
                });
            assert_eq!(repeated, Err(EvaluationFailure::ResourceLimit(first)));
            assert!(!called.get());
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }

    #[test]
    fn work_slice_preserves_active_session_depth() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let outer = ctx.enter_nested_limit("test outer evaluation").unwrap();
        let parent = ctx.work_budget(10);
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

    #[test]
    fn session_work_slice_admits_stored_derivative_storage_and_work() {
        use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
        use crate::math::{Point3, Vector3};
        let fixture = cadmpeg_test_support::service_decode_context();
        let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            crate::geometry::nurbs::NurbsCurve::from_lanes(
                &fixture,
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.5, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                ],
                None,
                false,
            )
            .unwrap()
            .unwrap(),
        ));
        for trigger in 0..5 {
            let mut policy = DecodePolicy::service();
            let dimension = match trigger {
                0 => {
                    policy.limits.max_materialized_bytes = 0;
                    ResourceDimension::MaterializedBytes
                }
                1 => {
                    policy.limits.max_collection_items = 0;
                    ResourceDimension::CollectionItems
                }
                2 => {
                    policy.limits.max_work_units = 0;
                    ResourceDimension::WorkUnits
                }
                3 => {
                    policy.limits.max_recursion_depth = 0;
                    ResourceDimension::RecursionDepth
                }
                _ => {
                    policy.limits.max_retained_bytes = 0;
                    policy.limits.max_materialized_bytes = 4096;
                    ResourceDimension::RetainedBytes
                }
            };
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = ctx.work_budget(100_000);
            let result = EvaluationAdmission::Decode(&ctx)
                .within_work_slice(&parent, |admission| {
                    crate::eval::decode::curve_tangent(admission, &curve, 0.5)
                });
            if trigger == 4 {
                assert_eq!(result.unwrap().get(), Vector3::new(1.0, 0.0, 0.0));
                assert!(parent.consumed() > 0);
                let storage = ctx
                    .reserve_scoped_limit(
                        policy.limits.max_materialized_bytes,
                        "test released derivative storage",
                    )
                    .unwrap();
                drop(storage);
                assert!(ctx.finish_session().is_ok());
            } else {
                let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                    panic!("a stored derivative preserves admission refusal");
                };
                assert_eq!(first.dimension, dimension);
                assert_eq!((first.limit, first.used), (0, 0));
                assert!(first.additional > 0);
                assert_eq!(
                    EvaluationAdmission::Decode(&ctx).within_work_slice(&parent, |admission| {
                        crate::eval::decode::curve_tangent(admission, &curve, f64::NAN)
                    }),
                    Err(EvaluationFailure::ResourceLimit(first))
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
                );
            }
        }
    }

    #[test]
    fn placed_evaluator_steps_refuse_before_nested_work() {
        use crate::geometry::{
            CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
        };
        use crate::math::{Point3, Vector3};
        let transform = crate::transform::Transform::affine([
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
        ])
        .unwrap();
        let curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
            crate::geometry::PlacedCurve::try_new(
                Box::new(SolvedCurveGeometry::Line(
                    crate::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                transform,
            )
            .unwrap(),
        ));
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
            crate::geometry::PlacedSurface::try_new(
                Box::new(SolvedSurfaceGeometry::Plane(
                    crate::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                )),
                transform,
            )
            .unwrap(),
        ));
        for trigger in 0..4 {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let parent = ctx.work_budget(100);
            let result =
                EvaluationAdmission::Decode(&ctx).within_work_slice(&parent, |admission| {
                    match trigger {
                        0 => crate::eval::decode::curve_point(admission, &curve, 0.5)
                            .map(|_| ())
                            .map_err(|failure| failure.map(|_| ())),
                        1 => crate::eval::decode::curve_tangent(admission, &curve, 0.5).map(|_| ()),
                        2 => {
                            crate::eval::curve_second_derivative(admission, &curve, 0.5).map(|_| ())
                        }
                        _ => crate::eval::decode::surface_point(admission, &surface, 0.5, 0.5)
                            .map(|_| ())
                            .map_err(|failure| failure.map(|_| ())),
                    }
                });
            let EvaluationFailure::ResourceLimit(first) = result.unwrap_err() else {
                panic!("a placed step must retain its work refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
            );
        }
    }
}
