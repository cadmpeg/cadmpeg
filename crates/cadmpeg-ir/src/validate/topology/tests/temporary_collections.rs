// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn topology_regeneration_references_preserve_temporary_storage_and_work_refusals() {
    let definition = crate::features::FeatureOperation::DerivedGeometry {
        source: crate::features::FeatureId::mint("test:model:feature#source").unwrap(),
    };
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) =
            super::super::regeneration_references(&ctx, &definition)
        else {
            panic!("temporary reference collection must refuse");
        };
        assert_eq!(original.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
    }
}

#[test]
fn topology_regeneration_references_keep_byte_order_uniqueness_and_release_storage() {
    use crate::features::patterns::{PatternKind, PatternSeed, PatternTransform};
    let first = crate::features::FeatureId::mint("test:model:feature#first").unwrap();
    let second = crate::features::FeatureId::mint("test:model:feature#second").unwrap();
    let definition = crate::features::FeatureOperation::Pattern {
        seeds: vec![
            PatternSeed::Feature(second.clone()),
            PatternSeed::Feature(first.clone()),
            PatternSeed::Feature(second.clone()),
        ],
        pattern: PatternKind::new(PatternTransform::Mirror {
            plane_origin: crate::features::FinitePoint3::new(crate::math::Point3::new(
                0.0, 0.0, 0.0,
            ))
            .unwrap(),
            plane_normal: crate::features::FeatureDirection3::new(crate::math::Vector3::new(
                1.0, 0.0, 0.0,
            ))
            .unwrap(),
        })
        .unwrap(),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65536;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let references = super::super::regeneration_references(&ctx, &definition).unwrap();
        assert_eq!(&*references, [&first, &second]);
    }
    drop(
        ctx.reserve_scoped(65536, "temporary references released")
            .unwrap(),
    );
    ctx.finish_session().unwrap();
}

#[test]
fn topology_profile_reference_borrows_the_selection_payload() {
    let profile = crate::features::ProfileRef::Planar(crate::features::PlanarProfileRef::Sketch(
        crate::sketches::SketchId::mint("test:model:sketch#profile").unwrap(),
    ));
    let crate::features::ProfileRef::Planar(original) = &profile else {
        panic!("planar fixture");
    };
    let super::super::ProfileReference::Planar(borrowed) =
        super::super::ProfileReference::from(&profile)
    else {
        panic!("planar view");
    };
    assert!(std::ptr::eq(original, borrowed));
}

#[test]
fn topology_sweep_profile_filter_admits_sections_that_produce_no_reference() {
    let definition = crate::features::FeatureOperation::Sweep {
        shape: crate::features::SweepShape::sheet_sections(
            crate::features::SweepMode::Surface {},
            crate::features::SweepSection::Unresolved(None),
            vec![crate::features::SweepSection::Unresolved(None); 8],
        ),
        path: None,
        orientation: None,
        transition: None,
        transformation: None,
        path_tangent: false,
        linearize: false,
        twist: None,
        path_extent: None,
        guide_rail: None,
        taper: None,
        scale: None,
        allow_multi_profile_faces: None,
    };
    for work in [0, 1] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) =
            super::super::definition_profiles(&ctx, &definition)
        else {
            panic!("empty profile filter must admit upstream sections");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
        );
    }
}

#[test]
fn native_reference_whitespace_admits_only_the_visited_prefix() {
    let nonblank = format!("a{}", "\u{2003}".repeat(4096));
    for (text, visits, expected) in [
        ("", 0, false),
        (nonblank.as_str(), 1, true),
        ("\u{2003}éunread", 2, true),
        (" \t\u{2003}", 3, false),
    ] {
        assert_eq!(!text.trim().is_empty(), expected);
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::super::non_blank_native_reference(&ctx, text);
            if cap == visits {
                assert_eq!(result.unwrap(), expected);
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("the next whitespace scalar must refuse");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.operation, "native reference whitespace scan");
                assert_eq!((original.used, original.additional), (cap, 1));
                assert!(matches!(super::super::non_blank_native_reference(&ctx, ""), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
            }
        }
    }
}

#[test]
fn fixed_revolution_termination_slots_borrow_original_operands_in_side_order() {
    use crate::features::{FeatureOperation, RevolveExtent};
    use super::super::TerminationRef;

    for extent in [
        None,
        Some(serde_json::json!({"kind":"one_sided", "termination":{"kind":"to_face", "face":{"kind":"native", "value":"face:é"}}})),
        Some(serde_json::json!({"kind":"symmetric", "termination":{"kind":"through_all"}})),
        Some(serde_json::json!({"kind":"two_sided", "first":{"kind":"through_next"}, "second":{"kind":"angle", "angle":1.25}})),
    ] {
        let mut construction = serde_json::json!({"state":"unresolved", "missing":"profile"});
        if let Some(extent) = extent { construction["extent"] = extent; }
        let definition: FeatureOperation = serde_json::from_value(serde_json::json!({
            "definition":"revolve", "construction":construction, "op":"new_body",
        })).unwrap();
        let FeatureOperation::Revolve { construction, .. } = &definition else {
            panic!("revolution fixture");
        };
        let expected = match construction.extent() {
            None => [None, None],
            Some(RevolveExtent::OneSided { termination } | RevolveExtent::Symmetric { termination }) => [Some(termination), None],
            Some(RevolveExtent::TwoSided { first, second }) => [Some(first), Some(second)],
        };
        for (actual, expected) in super::super::definition_terminations(&definition).into_iter().zip(expected) {
            match (actual, expected) {
                (Some(TerminationRef::Angular(actual)), Some(expected)) => assert!(std::ptr::eq(actual, expected)),
                (None, None) => {},
                _ => panic!("fixed slots must preserve original side count/order"),
            }
        }
    }
}

#[test]
fn historical_member_refusal_precedes_the_borrowed_member_callback() {
    use crate::features::{DistinctMembers, FeatureId, FeatureInputTopology};
    use crate::ids::{FeatureInputTopologyId, HistoricalVertexId};
    use crate::index::identities::BorrowedIdentities;
    use std::cell::Cell;

    let state = FeatureInputTopology {
        id: FeatureInputTopologyId::mint("test:model:feature-input#borrowed").unwrap(),
        input_of: FeatureId::mint("test:model:feature#borrowed").unwrap(),
        bodies: DistinctMembers::default(),
        faces: DistinctMembers::default(),
        edges: DistinctMembers::default(),
        vertices: DistinctMembers::try_from(
            vec![
                HistoricalVertexId::mint("test:model:historical-vertex#first").unwrap(),
                HistoricalVertexId::mint("test:model:historical-vertex#second").unwrap(),
                HistoricalVertexId::mint("test:model:historical-vertex#third").unwrap(),
            ],
            &cadmpeg_test_support::service_decode_context(),
        ).unwrap(),
        native_ref: None,
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "historical member scan",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let states = BorrowedIdentities::build(&ctx, |add| add(state.id.as_str(), &state)).unwrap();
            let callbacks = Cell::new(0);
            let mut findings = Vec::new();
            let result = super::super::check_historical_members(
                &ctx, &mut findings,
                (&state.input_of, &state.id, &state.vertices[..1]),
                HistoricalVertexId::as_str, "vertex", &states,
                |state| state.vertices.iter().map(|id| {
                    callbacks.set(callbacks.get() + 1);
                    id.as_str()
                }),
            );
            let Err(CodecError::ResourceLimit(first)) = &result else {
                panic!("historical member visit must refuse");
            };
            assert_eq!(first.operation, "historical member scan");
            assert_eq!(first.additional, 1);
            assert_eq!(callbacks.get(), 0);
            assert!(findings.is_empty());
            drop(states);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *first));
            result
        },
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let states = BorrowedIdentities::build(&ctx, |add| add(state.id.as_str(), &state)).unwrap();
    let callbacks = Cell::new(0);
    let mut findings = Vec::new();
    super::super::check_historical_members(
        &ctx, &mut findings,
        (&state.input_of, &state.id, &state.vertices[..1]),
        HistoricalVertexId::as_str, "vertex", &states,
        |state| state.vertices.iter().map(|id| {
            callbacks.set(callbacks.get() + 1);
            id.as_str()
        }),
    ).unwrap();
    assert_eq!(callbacks.get(), 3);
    assert!(findings.is_empty());
    drop(states);
    ctx.finish_session().unwrap();
}

#[test]
fn compound_loft_reference_checks_borrow_all_fixed_scale_and_tail_forms() {
    use crate::geometry::surface_payloads::{CompoundLoftSurfacePayload, ScaledCompoundLoftSurfacePayload};
    use crate::geometry::{
        ClassicLoftProfileData, CompoundLoftConstruction, CompoundLoftDirection,
        CompoundLoftScale, CompoundLoftScaleMember, CompoundLoftScales, CompoundLoftTail,
        Curve, CurveGeometry, LoftSubdata, ProceduralSurface, ProceduralSurfaceDefinition,
        ScaledCompoundLoftBranch, ScaledCompoundLoftConstruction, ScaledCompoundLoftShape,
        SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
    };
    use crate::index::{ModelIndex, StandardIndex};
    use crate::math::Vector3;
    use crate::CadIr;

    let curve = crate::ids::CurveId::mint("test:model:curve#scale").unwrap();
    let surface = crate::ids::SurfaceId::mint("test:model:surface#scale").unwrap();
    let direction = Vector3::new(1.0, 0.0, 0.0);
    let scale = || CompoundLoftScale {
        path: curve.clone(),
        auxiliaries: vec![curve.clone(), curve.clone()],
        members: (0..3).map(|_| CompoundLoftScaleMember {
            type_code: 0,
            curve: curve.clone(),
            data: ClassicLoftProfileData {
                surface: surface.clone(), pcurve: None, first_flag: false,
                asm_extension: 0,
                subdata: LoftSubdata::Type211 { dimensions: [0, 0], row: [0.0, 0.0] },
                direction: None,
            },
        }).collect(),
        tail: [0, 0],
    };
    let mut definitions = Vec::new();
    for tail in [
        CompoundLoftTail::Six {
            flags: [false; 2], scale: Box::new(scale()), selector: 0,
            direction, parameter_range: [0.0, 1.0], curve: curve.clone(),
        },
        CompoundLoftTail::Seven {
            first_flag: false, first_scale: None, second_flag: false,
            second_scale: Box::new(scale()), selector: 0,
            direction, trailing_flags: [false; 2],
        },
        CompoundLoftTail::Seven {
            first_flag: true, first_scale: Some(Box::new(scale())), second_flag: false,
            second_scale: Box::new(scale()), selector: 0,
            direction, trailing_flags: [false; 2],
        },
        CompoundLoftTail::Zero {
            flags: [false; 2], direction: CompoundLoftDirection::Vector { value: direction },
            trailing_flags: [false; 2],
        },
        CompoundLoftTail::Zero {
            flags: [false; 2], direction: CompoundLoftDirection::Curve {
                curve: curve.clone(), selector: std::num::NonZeroI64::new(1).unwrap(),
            },
            trailing_flags: [false; 2],
        },
    ] {
        definitions.push(ProceduralSurfaceDefinition::CompoundLoft(
            CompoundLoftSurfacePayload::try_new(CompoundLoftConstruction {
                scales: CompoundLoftScales::try_new((0..5).map(|_| scale()).collect()).unwrap(),
                flags: [false; 2], tail,
            }, None).unwrap(),
        ));
    }
    for branch in [
        ScaledCompoundLoftBranch::ExtendedVector {
            first_scale: None, second_scale: Box::new(scale()), selector: 0, direction,
        },
        ScaledCompoundLoftBranch::ExtendedVector {
            first_scale: Some(Box::new(scale())), second_scale: Box::new(scale()), selector: 0, direction,
        },
        ScaledCompoundLoftBranch::ExtendedCurve {
            scale: None, flag: false, singularity: 0, curve: curve.clone(),
        },
        ScaledCompoundLoftBranch::ExtendedCurve {
            scale: Some(Box::new(scale())), flag: true, singularity: 0, curve: curve.clone(),
        },
        ScaledCompoundLoftBranch::Direct {
            flag: false, direction: CompoundLoftDirection::Vector { value: direction },
        },
        ScaledCompoundLoftBranch::Direct {
            flag: false, direction: CompoundLoftDirection::Curve {
                curve: curve.clone(), selector: std::num::NonZeroI64::new(-1).unwrap(),
            },
        },
    ] {
        definitions.push(ProceduralSurfaceDefinition::ScaledCompoundLoft(
            ScaledCompoundLoftSurfacePayload::try_new(Box::new(ScaledCompoundLoftConstruction {
                singularity: 0, shape: ScaledCompoundLoftShape::Full {},
                discontinuities: std::array::from_fn(|_| Vec::new()), discontinuity_flag: false,
                scales: CompoundLoftScales::try_new((0..3).map(|_| scale()).collect()).unwrap(),
                flags: [false; 2], selector: 0, branch, trailing_flags: [false; 2],
                tail_kind: 0, tail_directions: [direction; 2], tail_singularity: 0,
                tail_curve: curve.clone(),
            }), None).unwrap(),
        ));
    }
    for definition in definitions {
        let mut ir = CadIr::empty();
        ir.model.curves.push(Curve {
            id: curve.clone(), source_object: None,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
        });
        ir.model.surfaces.push(Surface {
            id: surface.clone(), source_object: None,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            "test:model:surface-construction#borrowed".try_into().unwrap(), definition, None,
        ));
        let ids = ModelIndex::build(&ir, StandardIndex);
        assert!(ids.curves(curve.as_str(), StandardIndex).is_some());
        assert!(ids.surfaces(surface.as_str(), StandardIndex).is_some());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        super::super::check_references(&ctx, &ir, &ids, &mut findings).unwrap();
        assert!(findings.is_empty());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn compound_loft_missing_references_keep_branch_scale_and_member_order() {
    use crate::geometry::surface_payloads::{CompoundLoftSurfacePayload, ScaledCompoundLoftSurfacePayload};
    use crate::geometry::{
        ClassicLoftProfileData, CompoundLoftConstruction, CompoundLoftScale,
        CompoundLoftScaleMember, CompoundLoftScales, CompoundLoftTail, LoftSubdata,
        ProceduralSurface, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftConstruction, ScaledCompoundLoftShape,
    };
    use crate::index::{ModelIndex, StandardIndex};
    use crate::math::Vector3;
    let curve = |label: &str| crate::ids::CurveId::mint(&format!("test:model:curve#{label}")).unwrap();
    let surface = |label: &str| crate::ids::SurfaceId::mint(&format!("test:model:surface#{label}")).unwrap();
    let scale = |label: &str| CompoundLoftScale {
        path: curve(&format!("{label}-path")),
        auxiliaries: (0..2).map(|i| curve(&format!("{label}-aux-{i}"))).collect(),
        members: (0..2).map(|i| CompoundLoftScaleMember {
            type_code: 0, curve: curve(&format!("{label}-member-{i}")),
            data: ClassicLoftProfileData {
                surface: surface(&format!("{label}-support-{i}")), pcurve: None,
                first_flag: false, asm_extension: 0,
                subdata: LoftSubdata::Type211 { dimensions: [0, 0], row: [0.0, 0.0] },
                direction: None,
            },
        }).collect(), tail: [0, 0],
    };
    let direction = Vector3::new(1.0, 0.0, 0.0);
    let compound = ProceduralSurfaceDefinition::CompoundLoft(
        CompoundLoftSurfacePayload::try_new(CompoundLoftConstruction {
            scales: CompoundLoftScales::try_new(vec![scale("main-0"), scale("main-1")]).unwrap(),
            flags: [false; 2], tail: CompoundLoftTail::Seven {
                first_flag: true, first_scale: Some(Box::new(scale("tail-0"))),
                second_flag: false, second_scale: Box::new(scale("tail-1")),
                selector: 0, direction, trailing_flags: [false; 2],
            },
        }, None).unwrap(),
    );
    let scaled = ProceduralSurfaceDefinition::ScaledCompoundLoft(
        ScaledCompoundLoftSurfacePayload::try_new(Box::new(ScaledCompoundLoftConstruction {
            singularity: 0, shape: ScaledCompoundLoftShape::Full {},
            discontinuities: std::array::from_fn(|_| Vec::new()), discontinuity_flag: false,
            scales: CompoundLoftScales::try_new(vec![scale("main-0"), scale("main-1")]).unwrap(),
            flags: [false; 2], selector: 0,
            branch: ScaledCompoundLoftBranch::ExtendedCurve {
                scale: Some(Box::new(scale("tail-0"))), flag: true,
                singularity: 0, curve: curve("branch"),
            },
            trailing_flags: [false; 2], tail_kind: 0, tail_directions: [direction; 2],
            tail_singularity: 0, tail_curve: curve("tail-curve"),
        }), None).unwrap(),
    );
    for (definition, prefix, scale_labels) in [
        (compound, vec![], vec!["main-0", "main-1", "tail-0", "tail-1"]),
        (scaled, vec!["branch", "tail-curve"], vec!["main-0", "main-1", "tail-0"]),
    ] {
        // The grammar checks branch curves first, then leading scales and tail
        // scales. Each scale states path, auxiliaries, then member/support pairs.
        let mut expected: Vec<String> = prefix.iter().map(|label| {
            format!("references missing curve `{}`", curve(label))
        }).collect();
        for label in scale_labels {
            for suffix in ["path", "aux-0", "aux-1", "member-0", "support-0", "member-1", "support-1"] {
                let label = format!("{label}-{suffix}");
                expected.push(if suffix.starts_with("support") {
                    format!("references missing surface `{}`", surface(&label))
                } else {
                    format!("references missing curve `{}`", curve(&label))
                });
            }
        }
        let mut ir = crate::CadIr::empty();
        let owner = "test:model:surface-construction#ordered";
        ir.model.procedural_surfaces.push(ProceduralSurface::new(owner.try_into().unwrap(), definition, None));
        let ids = ModelIndex::build(&ir, StandardIndex);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut findings = Vec::new();
        super::super::check_references(&ctx, &ir, &ids, &mut findings).unwrap();
        assert_eq!(findings.iter().map(|f| &f.message).collect::<Vec<_>>(), expected.iter().collect::<Vec<_>>());
        for finding in findings {
            assert_eq!(finding.check, crate::report::check::Check::ReferentialIntegrity);
            assert_eq!(finding.severity, crate::report::Severity::Error);
            assert_eq!(finding.entity.as_deref(), Some(owner));
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn sweep_fixed_formula_slots_borrow_all_layouts_and_keep_law_reference_order() {
    use crate::geometry::surface_payloads::SweepSurfacePayload;
    use crate::geometry::{
        CacheContract, Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
        SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
        SweepSurfaceConstruction, SweepSurfaceLayout,
    };
    use crate::index::{ModelIndex, StandardIndex};
    use crate::report::check::Check;
    use crate::report::Severity;
    let curve = |label: &str| format!("test:model:curve#{label}");
    let surface = "test:model:surface#sweep-support";
    let edge = |label: &str| serde_json::json!({
        "kind":"edge", "curve":curve(label), "parameters":[0.0,1.0],
    });
    let formula = |label: &str| serde_json::json!({
        "kind":"named", "name":"test-formula", "variables":[edge(label)],
    });
    let prefix = serde_json::json!({
        "mode":0, "profile_range":[0.0,1.0], "profile_frame":null,
        "origin":{"x":0.0,"y":0.0,"z":0.0},
        "directions":vec![serde_json::json!({"x":1.0,"y":0.0,"z":0.0}); 3],
        "trajectory_flag":false, "path_range":[0.0,1.0], "path_parameter":0.0,
    });
    let mut layouts = vec![(None, vec![])];
    layouts.push((Some(serde_json::json!({
        "kind":"profile_first", "secondary_kind":0,
        "directions":vec![serde_json::json!({"x":1.0,"y":0.0,"z":0.0}); 5],
        "origin":{"x":0.0,"y":0.0,"z":0.0}, "parameters":vec![0.0; 4],
        "formulas":[formula("formula-0"),formula("formula-1"),formula("formula-2")],
    })), vec![("curve",curve("formula-0")),("curve",curve("formula-1")),("curve",curve("formula-2"))]));
    for (extra, expected) in [
        (serde_json::json!({"kind":"explicit_formula", "formula_flag":false,
            "formula":formula("formula"), "trailing_flag":false}),
            vec![("curve",curve("formula"))]),
        (serde_json::json!({"kind":"explicit_guide", "guide_flags":[false,false],
            "guide_curve":curve("guide"), "guide_range":[0.0,1.0],
            "guide_modes":[0,0], "guide_parameters":vec![0.0; 6], "trailing_flags":vec![false; 3]}),
            vec![("curve",curve("guide"))]),
        (serde_json::json!({"kind":"explicit_surface", "singularity":0,
            "support_surface":surface, "auxiliary_curve":curve("auxiliary"),
            "support_flag":false, "legacy_flag":null}),
            vec![("surface",surface.to_owned()),("curve",curve("auxiliary"))]),
        (serde_json::json!({"kind":"law_driven", "first_law":edge("first-law"),
            "first_mode":0, "first_range":[0.0,1.0],
            "law_direction":{"x":1.0,"y":0.0,"z":0.0}, "path_mode":0,
            "path_flag":false, "second_law_flag":false, "second_law":edge("second-law"),
            "formula_mode":0, "formula":formula("formula"), "trailing_flag":false}),
            vec![("curve",curve("first-law")),("curve",curve("second-law")),("curve",curve("formula"))]),
    ] {
        let mut layout = prefix.clone();
        layout.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        if layout["kind"] == "law_driven" {
            layout.as_object_mut().unwrap().remove("trajectory_flag");
        }
        layouts.push((Some(layout), expected));
    }
    for (layout, mut expected) in layouts {
        expected.splice(0..0, [("curve",curve("profile")),("curve",curve("spine"))]);
        let native = layout.map(|layout| Box::new(SweepSurfaceConstruction {
            primary_kind:0, cache:CacheContract::from_form(None),
            layout:serde_json::from_value::<SweepSurfaceLayout>(layout).unwrap(),
            discontinuities:std::array::from_fn(|_| Vec::new()), discontinuity_flag:false,
        }));
        let definition = ProceduralSurfaceDefinition::Sweep(SweepSurfacePayload::try_new(
            curve("profile").try_into().unwrap(), curve("spine").try_into().unwrap(), native,
        ).unwrap());
        let mut missing = crate::CadIr::empty();
        missing.model.procedural_surfaces.push(ProceduralSurface::new(
            "test:model:surface-construction#sweep-borrowed".try_into().unwrap(), definition, None,
        ));
        let ctx = cadmpeg_test_support::service_decode_context();
        let ids = ModelIndex::build(&missing, StandardIndex);
        let mut findings = Vec::new();
        super::super::check_references(&ctx, &missing, &ids, &mut findings).unwrap();
        assert_eq!(findings.iter().map(|f| f.message.as_str()).collect::<Vec<_>>(),
            expected.iter().map(|(kind,id)| format!("references missing {kind} `{id}`")).collect::<Vec<_>>());
        assert!(findings.iter().all(|f| f.check == Check::ReferentialIntegrity
            && f.severity == Severity::Error
            && f.entity.as_deref() == Some("test:model:surface-construction#sweep-borrowed")));
        drop(ids);
        ctx.finish_session().unwrap();
        for (kind, id) in &expected {
            match *kind {
                "curve" => missing.model.curves.push(Curve {
                    id:id.clone().try_into().unwrap(), source_object:None,
                    geometry:CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record:None }),
                }),
                "surface" => missing.model.surfaces.push(Surface {
                    id:id.clone().try_into().unwrap(), source_object:None,
                    geometry:SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record:None }),
                }),
                _ => unreachable!("only curve and surface fixture references"),
            }
        }
        let ids = ModelIndex::build(&missing, StandardIndex);
        for (kind, id) in &expected {
            match *kind {
                "curve" => assert!(ids.curves(id, StandardIndex).is_some()),
                "surface" => assert!(ids.surfaces(id, StandardIndex).is_some()),
                _ => unreachable!("only curve and surface fixture references"),
            }
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        super::super::check_references(&ctx, &missing, &ids, &mut findings).unwrap();
        assert!(findings.is_empty());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn fixed_empty_intersection_sides_add_no_visits_beyond_the_arena_row() {
    use crate::geometry::{IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, ProceduralCurveDefinition};
    use crate::index::{ModelIndex, StandardIndex};
    let context = IntcurveSupportContext::try_new(
        std::array::from_fn(|_| IntcurveSupportSide { surface:None, pcurve:None }),
        [0.0,1.0], std::array::from_fn(|_| Vec::new()),
    ).unwrap();
    let mut ir = crate::CadIr::empty();
    ir.model.procedural_curves.push(ProceduralCurve::new(
        "test:model:curve-construction#fixed-empty-sides".try_into().unwrap(),
        ProceduralCurveDefinition::Intersection { context, discontinuity_flag:false, cache:None },
    ));
    let ids = ModelIndex::build(&ir, StandardIndex);
    for cap in 0..=1 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let result = super::super::check_references(&ctx, &ir, &ids, &mut findings);
        assert!(findings.is_empty());
        if cap == 0 {
            let Err(CodecError::ResourceLimit(first)) = result else { panic!("arena row visit must refuse"); };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "topology validation scan");
            assert_eq!((first.used,first.additional),(0,1));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
        } else {
            result.unwrap();
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn fixed_empty_law_and_net_slots_add_no_visits_beyond_the_arena_row() {
    use crate::geometry::surface_payloads::{LawSurfacePayload, NetSurfacePayload};
    use crate::geometry::{
        FiniteLawFormula, IntcurveSupportContext, IntcurveSupportSide, LawFormula,
        LawSurfaceConstruction, LawSurfaceTail, LoftSection, NetSurfaceConstruction,
        ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    };
    use crate::index::{ModelIndex, StandardIndex};
    let law = ProceduralSurfaceDefinition::Law(LawSurfacePayload::try_new(Box::new(LawSurfaceConstruction {
        parameter_ranges:None, primary:LawFormula::Null {}, additional:Vec::new(),
        tail:LawSurfaceTail::Historical {}, discontinuities:std::array::from_fn(|_| Vec::new()),
    })).unwrap());
    let net = ProceduralSurfaceDefinition::Net(NetSurfacePayload::try_new(Box::new(NetSurfaceConstruction {
        sections:Box::new(std::array::from_fn(|_| LoftSection { entries:Vec::new() })),
        frame_parameters:[0.0; 12], flag:0, directions:[crate::math::Vector3::new(1.0,0.0,0.0); 4],
        formulas:Box::new(std::array::from_fn(|_| LawFormula::Null {})),
        discontinuities:std::array::from_fn(|_| Vec::new()), discontinuity_flag:false,
    }),None).unwrap());
    let curve = ProceduralCurveDefinition::Law {
        context:IntcurveSupportContext::try_new(
            std::array::from_fn(|_| IntcurveSupportSide { surface:None,pcurve:None }),
            [0.0,1.0],std::array::from_fn(|_| Vec::new()),
        ).unwrap(), version:None, extension:0, primary:FiniteLawFormula::try_new(LawFormula::Null {}).unwrap(),
        additional:Vec::new(), cache:None,
    };
    let mut models = Vec::new();
    for definition in [law,net] {
        let mut ir = crate::CadIr::empty();
        ir.model.procedural_surfaces.push(ProceduralSurface::new(
            "test:model:surface-construction#fixed-empty-laws".try_into().unwrap(),definition,None,
        ));
        models.push(ir);
    }
    let mut ir = crate::CadIr::empty();
    ir.model.procedural_curves.push(ProceduralCurve::new(
        "test:model:curve-construction#fixed-empty-laws".try_into().unwrap(),curve,
    ));
    models.push(ir);
    for ir in models {
        let ids = ModelIndex::build(&ir, StandardIndex);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        super::super::check_references(&ctx,&ir,&ids,&mut findings).unwrap();
        assert!(findings.is_empty());
        ctx.finish_session().unwrap();
    }
}
