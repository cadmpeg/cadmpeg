// SPDX-License-Identifier: Apache-2.0

use crate::geometry::sampled::PolygonalSurface;
use crate::geometry::{
    OffsetExtension, RevisionCacheForm, RevisionSurfaceForm, SolvedSurfaceGeometry, SurfaceGeometry,
};
use crate::ids::ProceduralSurfaceId;
use crate::math::Point3;
use crate::scalar::PositiveI64;
use cadmpeg_core::decode::{cost::DecodeCost, DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

use super::budget::with_limit as limited;

#[test]
fn geometry_decode_cost_counts_surface_cache_and_polygon_children() {
    let context = cadmpeg_test_support::service_decode_context();
    let polygon = PolygonalSurface::new(
        vec![
            Point3::new(0., 0., 0.),
            Point3::new(1., 0., 0.),
            Point3::new(0., 1., 0.),
        ],
        vec![[0, 1, 2]],
        0.125,
        &context,
    )
    .unwrap()
    .unwrap();
    let solved = SolvedSurfaceGeometry::Polygonal(polygon);
    let construction = ProceduralSurfaceId::mint("test:model:procedural-surface#cost").unwrap();
    let identity_bytes = cadmpeg_core::decode::u64_from_index(construction.as_str().len());
    let geometry = SurfaceGeometry::Procedural {
        construction,
        cache: Some(solved),
    };
    // Procedural tag, identity, cache presence, solved tag, vertices, triangle indexes, deflection.
    assert_eq!(
        geometry.decode_cost(&context, "surface cost").unwrap(),
        1 + identity_bytes + 1 + 1 + 3 * 3 * 8 + 3 * 4 + 8
    );
}

fn extension() -> OffsetExtension {
    OffsetExtension::Revision {
        form: Box::new(RevisionSurfaceForm {
            revision: PositiveI64::new(1).unwrap(),
            support_bounds: [None; 4],
            reference_endpoints: [None; 2],
            second_endpoints: [None; 2],
            flags: [false; 4],
            cache: RevisionCacheForm::SolvedCache {
                fit_tolerance: crate::geometry::FitTolerance::try_new(0.).unwrap(),
            },
            discontinuities: [
                vec![0., 1., 2.],
                vec![3., 4.],
                vec![],
                vec![],
                vec![],
                vec![],
            ],
            tail_flag: false,
            trailing_flags: vec![true, false, true],
        }),
    }
}

#[test]
fn geometry_decode_cost_measures_offset_extension_children_without_array_visits() {
    let extension = extension();
    // Outer tag, revision, eight absent bounds, four flags, cache tag/tolerance,
    // five discontinuities, tail flag, three trailing flags.
    let expected = 1 + 8 + 8 + 4 + 1 + 8 + 5 * 8 + 1 + 3;
    assert_eq!(
        limited(ResourceDimension::WorkUnits, 0, |ctx| extension
            .decode_cost(ctx, "extension cost"))
        .unwrap(),
        expected
    );
    let admitted = extension.admit().unwrap();
    assert_eq!(
        limited(ResourceDimension::WorkUnits, 0, |ctx| admitted
            .decode_cost(ctx, "admitted extension cost"))
        .unwrap(),
        expected
    );
}

#[test]
fn geometry_decode_cost_checks_owned_child_arithmetic() {
    struct Overflow;
    impl DecodeCost for Overflow {
        fn decode_cost(
            &self,
            _ctx: &DecodeContext<'_>,
            _operation: &'static str,
        ) -> Result<u64, CodecError> {
            Ok(u64::MAX)
        }
    }
    let extension = OffsetExtension::<Overflow>::Revision {
        form: Box::new(RevisionSurfaceForm {
            revision: PositiveI64::new(1).unwrap(),
            support_bounds: [Some(Overflow), None, None, None],
            reference_endpoints: [None, None],
            second_endpoints: [None, None],
            flags: [false; 4],
            cache: RevisionCacheForm::SolvedCache {
                fit_tolerance: crate::geometry::FitTolerance::try_new(0.).unwrap(),
            },
            discontinuities: std::array::from_fn(|_| Vec::new()),
            tail_flag: false,
            trailing_flags: Vec::new(),
        }),
    };
    assert!(matches!(limited(ResourceDimension::WorkUnits, u64::MAX,
        |ctx| extension.decode_cost(ctx, "overflow extension cost")),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "overflow extension cost"));
}

#[test]
fn geometry_decode_cost_traverses_variable_offset_children_under_budget() {
    struct Child(Vec<u64>);
    impl DecodeCost for Child {
        fn decode_cost(
            &self,
            ctx: &DecodeContext<'_>,
            operation: &'static str,
        ) -> Result<u64, CodecError> {
            self.0.decode_cost(ctx, operation)
        }
    }
    let extension = OffsetExtension::<Child>::Revision {
        form: Box::new(RevisionSurfaceForm {
            revision: PositiveI64::new(1).unwrap(),
            support_bounds: [None, None, None, None],
            reference_endpoints: [None, None],
            second_endpoints: [None, None],
            flags: [false; 4],
            cache: RevisionCacheForm::SolvedCache {
                fit_tolerance: crate::geometry::FitTolerance::try_new(0.).unwrap(),
            },
            discontinuities: [
                vec![Child(vec![1, 2]), Child(vec![3])],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            tail_flag: false,
            trailing_flags: Vec::new(),
        }),
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "variable extension cost",
        |cap| {
            limited(ResourceDimension::WorkUnits, cap, |ctx| {
                extension.decode_cost(ctx, "variable extension cost")
            })
        },
    );
    // The six fixed discontinuity lanes visit only the two variable children, whose bytes total 24.
    assert_eq!(
        limited(ResourceDimension::WorkUnits, 2, |ctx| extension
            .decode_cost(ctx, "variable extension cost"))
        .unwrap(),
        1 + 8 + 8 + 4 + 1 + 8 + 3 * 8 + 1
    );
}

#[test]
fn geometry_decode_cost_admits_surface_grid_traversal() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    for rational in [false, true] {
        let surface = NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
            NurbsSurfaceLanes::new(
                vec![vec![Point3::new(0., 0., 0.); 2]; 2],
                rational.then(|| vec![vec![1.; 2]; 2]),
            ),
            false,
        )
        .unwrap()
        .unwrap();
        let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "surface grid cost",
            |cap| {
                limited(ResourceDimension::WorkUnits, cap, |ctx| {
                    geometry.decode_cost(ctx, "surface grid cost")
                })
            },
        );
        assert_eq!(
            limited(ResourceDimension::WorkUnits, 2, |ctx| geometry
                .decode_cost(ctx, "surface grid cost"))
            .unwrap(),
            1 + 1 + 2 * 4 + 2 * 4 * 8 + 1 + 4 * (if rational { 4 * 8 } else { 3 * 8 }) + 3
        );
    }
}

#[test]
fn geometry_decode_cost_keeps_fixed_extension_forms_free_of_visits() {
    use crate::geometry::{
        FitTolerance, LegacyCache, LegacyExtensionFlags, RevisionSurfaceParameterization,
    };
    for (flags, cache, expected) in [
        (LegacyExtensionFlags::Absent {}, None, 3),
        (LegacyExtensionFlags::Disabled {}, None, 3),
        (
            LegacyExtensionFlags::Enabled {
                secondary: false,
                tertiary: None,
            },
            None,
            5,
        ),
        (
            LegacyExtensionFlags::Enabled {
                secondary: true,
                tertiary: Some(false),
            },
            Some(LegacyCache {
                fit_tolerance: FitTolerance::try_new(0.25).unwrap(),
            }),
            14,
        ),
    ] {
        let extension = OffsetExtension::<f64>::Legacy { flags, cache };
        assert_eq!(
            limited(ResourceDimension::WorkUnits, 0, |ctx| extension
                .decode_cost(ctx, "legacy extension cost"))
            .unwrap(),
            expected
        );
    }
    let parameterization = RevisionCacheForm::Parameterization(RevisionSurfaceParameterization {
        u_interval: [Some(0.), None],
        v_interval: [None, Some(1.)],
        u_closure: 0,
        v_closure: 1,
        u_singularity: 2,
        v_singularity: 3,
    });
    assert_eq!(
        limited(ResourceDimension::WorkUnits, 0, |ctx| parameterization
            .decode_cost(ctx, "parameterized cache cost"))
        .unwrap(),
        1 + 4 * 8 + 4 + 2 * 8
    );
}

#[test]
fn geometry_decode_cost_admits_each_surface_basis_and_nesting_frame() {
    use crate::geometry::{analytic::PlaneSurface, PlacedSurface};
    use crate::math::Vector3;
    use crate::transform::Transform;
    let plane = PlaneSurface::try_new(
        Point3::new(0., 0., 0.),
        Vector3::new(0., 0., 1.),
        Vector3::new(1., 0., 0.),
    )
    .unwrap();
    let mut solved = SolvedSurfaceGeometry::Plane(plane);
    for _ in 0..2 {
        solved = SolvedSurfaceGeometry::Transformed(
            PlacedSurface::try_new(Box::new(solved), Transform::identity()).unwrap(),
        );
    }
    let geometry = SurfaceGeometry::Solved(solved);
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, "surface basis cost", |cap| {
            limited(dimension, cap, |ctx| {
                geometry.decode_cost(ctx, "surface basis cost")
            })
        });
    }
    let depth_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>());
    assert_eq!(
        limited(ResourceDimension::WorkUnits, 2, |ctx| geometry
            .decode_cost(ctx, "surface basis cost"))
        .unwrap(),
        1 + 1
            + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<PlaneSurface>())
            + 2 * (1 + 3 * 4 * 8 + depth_bytes)
    );
}
