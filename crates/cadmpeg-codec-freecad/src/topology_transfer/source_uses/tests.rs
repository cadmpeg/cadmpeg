// SPDX-License-Identifier: Apache-2.0

use super::super::locations;
use super::{working_text, Identities, Inventory, Located, Locations, Plan};
use crate::brep::location_identity::LocationIdentityError;
use crate::brep::{
    Tables, TextLocation, TextOrientation, TextShapeUse, TextTShape, TextTShapeGeometry,
    TextTShapes,
};
use crate::native::source_use::{SourceUse, SourceUseSelector};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::native::NativeNamespace;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::transform::Transform;
use std::collections::HashSet;

const OWNER: &str = "fcstd:native:shape-payload#Source";

fn context<T>(f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    f(&ctx)
}

fn vertex() -> TextTShape {
    TextTShape {
        geometry: TextTShapeGeometry::Vertex {
            tolerance: FiniteReal::ZERO,
            point: FinitePoint3::ZERO,
            representations: Vec::new(),
        },
        flags: [false; 7],
        children: Vec::new(),
    }
}

fn use_shape(shape: usize, location: usize) -> TextShapeUse {
    TextShapeUse {
        shape,
        location: location.into(),
        orientation: TextOrientation::Forward,
    }
}

fn tables<'a>(
    tshapes: &'a TextTShapes,
    roots: &'a [TextShapeUse],
    locations: &'a [TextLocation],
) -> Tables<'a> {
    Tables {
        tshapes,
        roots,
        locations,
        curve2ds: &[],
        curves: &[],
        surfaces: &[],
        polygons3d: &[],
        polygons_on_triangulations: &[],
        triangulations: &[],
    }
}

fn alias_payload() -> crate::brep::ShapePayloadRecord {
    let shapes = TextTShapes::from(vec![
        vertex(),
        TextTShape {
            geometry: TextTShapeGeometry::Compound,
            flags: [false; 7],
            children: vec![use_shape(1, 0), use_shape(1, 0)],
        },
    ]);
    serde_json::from_value(serde_json::json!({
        "id": OWNER, "property": "fcstd:native:property#Source:Shape",
        "entry": "fcstd:native:entry#Source", "form": "binary", "text": null,
        "binary": { "topology_version": 1, "locations": [], "curve2ds": [], "curves": [],
            "surfaces": [], "polygons3d": [], "polygons_on_triangulations": [],
            "triangulations": [], "tshapes": shapes, "roots": [use_shape(2, 0)] }
    }))
    .unwrap()
}

fn deep_chain(count: usize) -> TextTShapes {
    let mut shapes = vec![vertex()];
    for prior in 1..count {
        shapes.push(TextTShape {
            geometry: TextTShapeGeometry::Compound,
            flags: [false; 7],
            children: vec![use_shape(prior, 0)],
        });
    }
    TextTShapes::from(shapes)
}

#[test]
fn location_admission_order_is_identical_to_the_existing_prepass() {
    context(|ctx| {
        let locations = [
            TextLocation {
                factors: Vec::new(),
                transform: Transform::identity(),
            },
            TextLocation {
                factors: Vec::new(),
                transform: Transform::identity(),
            },
        ];
        let shapes = TextTShapes::from(vec![
            vertex(),
            TextTShape {
                geometry: TextTShapeGeometry::Compound,
                flags: [false; 7],
                children: vec![use_shape(1, 1), use_shape(1, 2)],
            },
        ]);
        let roots = [use_shape(2, 1), use_shape(2, 2)];
        let tables = tables(&shapes, &roots, &locations);
        let mut old = Locations::new(ctx, tables).unwrap();
        admit_topology_locations(ctx, tables, &mut old).unwrap();
        let mut current = Locations::new(ctx, tables).unwrap();
        Inventory::admit_locations(ctx, OWNER, tables, &mut current).unwrap();
        for left in 0..=locations.len() {
            let a = old.source(tables, left).unwrap();
            let b = current.source(tables, left).unwrap();
            assert_eq!(a.identity, b.identity);
            assert_eq!(a.transform, b.transform);
            for right in 0..=locations.len() {
                let x = old.compose(a, old.source(tables, right).unwrap());
                let y = current.compose(b, current.source(tables, right).unwrap());
                match (x, y) {
                    (Ok(x), Ok(y)) => {
                        assert_eq!(x.identity, y.identity);
                        assert_eq!(x.transform, y.transform);
                    }
                    (Err(_), Err(_)) => {}
                    _ => panic!("changed location product domain"),
                }
            }
        }
    });
}

#[test]
fn face_and_edge_support_order_matches_the_old_prepass_with_a_cancelling_root() {
    context(|ctx| {
        use crate::brep::{TextCurve, TextEdgeRepresentation, TextSurface};
        use cadmpeg_ir::features::FiniteVector3;
        use cadmpeg_ir::math::Vector3;
        let x = Transform::affine([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let y = Transform::affine([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 5.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        let native_locations = [
            TextLocation {
                factors: Vec::new(),
                transform: x,
            },
            TextLocation {
                factors: serde_json::from_value(serde_json::json!([{"location": 1, "power": -1}]))
                    .unwrap(),
                transform: x.try_inverse_affine().unwrap(),
            },
            TextLocation {
                factors: Vec::new(),
                transform: y,
            },
        ];
        let curves = [TextCurve::Line {
            origin: FinitePoint3::ZERO,
            direction: FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
        }];
        let surfaces = [TextSurface::Plane {
            origin: FinitePoint3::ZERO,
            axis: FiniteVector3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            u_axis: FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
            v_reversed: false,
        }];
        let edge = TextTShape {
            geometry: TextTShapeGeometry::Edge {
                tolerance: FiniteReal::ZERO,
                same_parameter: true,
                same_range: true,
                degenerated: false,
                representations: vec![TextEdgeRepresentation::Curve3d {
                    curve: 1,
                    location: 1,
                    parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
                }],
            },
            flags: [false; 7],
            children: vec![use_shape(1, 0), use_shape(1, 0)],
        };
        let wire = TextTShape {
            geometry: TextTShapeGeometry::Wire,
            flags: [false; 7],
            children: vec![use_shape(2, 3)],
        };
        let face = TextTShape {
            geometry: TextTShapeGeometry::Face {
                natural_restriction: false,
                tolerance: FiniteReal::ZERO,
                surface: None,
                location: 3.into(),
                triangulation: None,
            },
            flags: [false; 7],
            children: vec![use_shape(3, 2)],
        };
        let shapes = TextTShapes::from(vec![vertex(), edge, wire, face]);
        let mut native_shapes = serde_json::to_value(&shapes).unwrap();
        native_shapes[3]["geometry"]["surface"] = serde_json::json!(1);
        let shapes: TextTShapes = serde_json::from_value(native_shapes).unwrap();
        let roots = [use_shape(4, 1)];
        let mut tables = tables(&shapes, &roots, &native_locations);
        tables.curves = &curves;
        tables.surfaces = &surfaces;
        let mut old = Locations::new(ctx, tables).unwrap();
        admit_topology_locations(ctx, tables, &mut old).unwrap();
        let mut current = Locations::new(ctx, tables).unwrap();
        Inventory::admit_locations(ctx, OWNER, tables, &mut current).unwrap();
        let mut admitted = vec![(Located::identity(), Located::identity())];
        for index in 1..=native_locations.len() {
            let pair = (
                old.source(tables, index).unwrap(),
                current.source(tables, index).unwrap(),
            );
            assert_eq!(pair.0.identity, pair.1.identity);
            assert_eq!(pair.0.transform, pair.1.transform);
            admitted.push(pair);
        }
        let cancelled = old.compose(admitted[1].0, admitted[2].0).unwrap();
        assert_eq!(cancelled.identity, 0);
        assert_eq!(cancelled.transform, Transform::identity());
        // Include the child-chain and carrier products admitted by Edge, so
        // equal affine matrices cannot conceal a changed symbolic order.
        for (left, right) in [(2, 3), (1, 2)] {
            let pair = (
                old.compose(admitted[left].0, admitted[right].0).unwrap(),
                current
                    .compose(admitted[left].1, admitted[right].1)
                    .unwrap(),
            );
            assert_eq!(pair.0.identity, pair.1.identity);
            assert_eq!(pair.0.transform, pair.1.transform);
            admitted.push(pair);
        }
        for left in &admitted {
            for right in &admitted {
                match (
                    old.compose(left.0, right.0),
                    current.compose(left.1, right.1),
                ) {
                    (Ok(a), Ok(b)) => {
                        assert_eq!(a.identity, b.identity);
                        assert_eq!(a.transform, b.transform);
                    }
                    (Err(_), Err(_)) => {}
                    _ => panic!("changed support-product domain"),
                }
            }
        }
    });
}

fn namespace(
    ctx: &DecodeContext<'_>,
    payload: &crate::brep::ShapePayloadRecord,
) -> cadmpeg_ir::native::NativeNamespace {
    let tables = Tables::from_payload(payload).unwrap();
    let mut locations = Locations::new(ctx, tables).unwrap();
    let inventory = Inventory::admit_locations(ctx, &payload.id, tables, &mut locations).unwrap();
    let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
    inventory.install(&mut namespace).unwrap();
    namespace
        .set_arena(ctx, "shape_payloads", &[payload])
        .unwrap();
    namespace
}

#[test]
fn repeated_root_uses_keep_distinct_source_rows_at_the_same_placement() {
    context(|ctx| {
        let shapes = TextTShapes::from(vec![vertex()]);
        let roots = [use_shape(1, 0), use_shape(1, 0)];
        let tables = tables(&shapes, &roots, &[]);
        let mut locations = Locations::new(ctx, tables).unwrap();
        let inventory = Inventory::admit_locations(ctx, OWNER, tables, &mut locations).unwrap();
        assert_eq!(inventory.rows().len(), 2);
        assert_eq!(inventory.rows()[0].source_use, SourceUse::Root { root: 1 });
        assert_eq!(inventory.rows()[1].source_use, SourceUse::Root { root: 2 });
        assert_ne!(
            inventory.selector(1, 1, Located::identity()),
            inventory.selector(2, 1, Located::identity())
        );
    });
}

#[test]
fn equal_matrices_with_distinct_native_datums_do_not_coalesce_topology() {
    context(|ctx| {
        let datums = [
            TextLocation {
                factors: Vec::new(),
                transform: Transform::identity(),
            },
            TextLocation {
                factors: Vec::new(),
                transform: Transform::identity(),
            },
        ];
        let shapes = TextTShapes::from(vec![
            vertex(),
            TextTShape {
                geometry: TextTShapeGeometry::Compound,
                flags: [false; 7],
                children: vec![use_shape(1, 1), use_shape(1, 2)],
            },
        ]);
        let roots = [use_shape(2, 0)];
        let tables = tables(&shapes, &roots, &datums);
        let mut locations = Locations::new(ctx, tables).unwrap();
        let inventory = Inventory::admit_locations(ctx, OWNER, tables, &mut locations).unwrap();
        let left = locations.source(tables, 1_usize).unwrap();
        let right = locations.source(tables, 2_usize).unwrap();
        assert_eq!(left.transform, right.transform);
        assert_ne!(left.identity, right.identity);
        assert_eq!(inventory.rows().len(), 3);
        assert_ne!(
            inventory.selector(1, 1, left),
            inventory.selector(1, 1, right)
        );
    });
}

#[test]
fn reversed_aliases_share_the_existing_topology_key_and_first_source_route() {
    context(|ctx| {
        let mut reversed = use_shape(1, 0);
        reversed.orientation = TextOrientation::Reversed;
        let shapes = TextTShapes::from(vec![
            vertex(),
            TextTShape {
                geometry: TextTShapeGeometry::Compound,
                flags: [false; 7],
                children: vec![use_shape(1, 0), reversed],
            },
        ]);
        let roots = [use_shape(2, 0)];
        let tables = tables(&shapes, &roots, &[]);
        let mut locations = Locations::new(ctx, tables).unwrap();
        let inventory = Inventory::admit_locations(ctx, OWNER, tables, &mut locations).unwrap();
        assert_eq!(inventory.rows().len(), 2);
        assert_eq!(
            inventory.rows()[1].source_use,
            SourceUse::Child {
                parent: inventory.rows()[0].id.clone(),
                slot: 2
            }
        );
        assert_eq!(
            inventory.selector(1, 1, Located::identity()),
            Some(inventory.rows()[1].id.as_str())
        );
    });
}

#[test]
fn equivalent_compound_location_aliases_have_one_canonical_native_row() {
    context(|ctx| {
        let datums = [
            TextLocation {
                factors: Vec::new(),
                transform: Transform::identity(),
            },
            TextLocation {
                factors: serde_json::from_value(serde_json::json!([{"location":1,"power":1}]))
                    .unwrap(),
                transform: Transform::identity(),
            },
        ];
        let shapes = TextTShapes::from(vec![
            vertex(),
            TextTShape {
                geometry: TextTShapeGeometry::Compound,
                flags: [false; 7],
                children: vec![use_shape(1, 1), use_shape(1, 2)],
            },
        ]);
        let roots = [use_shape(2, 0)];
        let before = (shapes.clone(), datums.clone());
        let tables = tables(&shapes, &roots, &datums);
        let mut locations = Locations::new(ctx, tables).unwrap();
        let inventory = Inventory::admit_locations(ctx, OWNER, tables, &mut locations).unwrap();
        let first = locations.source(tables, 1_usize).unwrap();
        let second = locations.source(tables, 2_usize).unwrap();
        assert_eq!(first.identity, second.identity);
        assert_eq!(inventory.rows().len(), 2);
        assert_eq!(
            inventory.selector(1, 1, first),
            inventory.selector(1, 1, second)
        );
        assert_eq!(shapes, before.0);
        assert_eq!(datums, before.1);
    });
}

#[test]
fn selector_admission_charges_native_entities_before_materializing_a_row() {
    let shapes = deep_chain(4096);
    let roots = [use_shape(4096, 0)];
    let tables = tables(&shapes, &roots, &[]);
    for count in [0, 2] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = count;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut locations = Locations::new(&ctx, tables).unwrap();
        assert!(
            matches!(Inventory::admit_locations(&ctx, OWNER, tables, &mut locations),
            Err(crate::brep::location_identity::LocationIdentityError::Decode(CodecError::ResourceLimit(limit)))
            if limit.dimension == ResourceDimension::Entities && limit.used == count && limit.additional == 1)
        );
    }
}

#[test]
fn deep_source_chain_refuses_each_unadmitted_row_before_expanding_its_path() {
    let shapes = deep_chain(4096);
    let roots = [use_shape(4096, 0)];
    let tables = tables(&shapes, &roots, &[]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut locations = Locations::new(&ctx, tables).unwrap();
    assert!(
        matches!(Inventory::admit_locations(&ctx, OWNER, tables, &mut locations),
        Err(crate::brep::location_identity::LocationIdentityError::Decode(CodecError::ResourceLimit(limit)))
        if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 0 && limit.additional > 0)
    );
}

#[test]
fn admitted_native_text_is_not_charged_twice_when_installed() {
    context(|ctx| {
        let shapes = deep_chain(4);
        let roots = [use_shape(4, 0)];
        let tables = tables(&shapes, &roots, &[]);
        let mut locations = Locations::new(ctx, tables).unwrap();
        let inventory = Inventory::admit_locations(ctx, OWNER, tables, &mut locations).unwrap();
        let expected = inventory.rows().to_vec();
        let remaining = ctx.retained_bytes_available();
        let mut namespace = cadmpeg_ir::native::NativeNamespace::default();
        inventory.install(&mut namespace).unwrap();
        assert_eq!(ctx.retained_bytes_available(), remaining);
        let actual = namespace
            .arena_as::<SourceUseSelector>("topology_uses")
            .unwrap();
        assert_eq!(actual.len(), expected.len());
        for row in expected {
            assert!(actual.contains(&row));
        }
    });
}

#[test]
fn selector_materialization_obeys_all_three_caller_container_depths() {
    // #246 removes the path vector: exactly TWO serialized containers remain.
    let shapes = deep_chain(2);
    let roots = [use_shape(2, 0)];
    let tables = tables(&shapes, &roots, &[]);
    for allowed in 0..=2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = allowed;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut locations = Locations::new(&ctx, tables).unwrap();
        let result = Inventory::admit_locations(&ctx, OWNER, tables, &mut locations);
        if allowed < 2 {
            assert!(
                matches!(result, Err(crate::brep::location_identity::LocationIdentityError::Decode(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::RecursionDepth && limit.limit == allowed)
            );
        } else {
            let mut ns = cadmpeg_ir::native::NativeNamespace::default();
            result.ok().unwrap().install(&mut ns).unwrap();
            assert_eq!(ns.arenas()["topology_uses"].len(), 2);
        }
    }
}

#[test]
fn namespace_api_streams_actual_rows_and_rejects_dangling_alias_and_global_identity_edits() {
    context(|ctx| {
        let payload = alias_payload();
        let ns = namespace(ctx, &payload);
        let remaining = ctx.retained_bytes_available();
        let resolved = super::resolve_namespace(ctx, &ns, std::slice::from_ref(&payload)).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(ctx.retained_bytes_available(), remaining);
        let rows = ns.arena_as::<SourceUseSelector>("topology_uses").unwrap();
        let root = rows
            .iter()
            .position(|row| matches!(row.source_use, SourceUse::Root { .. }))
            .unwrap();
        let child = 1 - root;
        for edit in 0..8 {
            let mut changed = rows.clone();
            match edit {
                0 => changed[child].owner = "fcstd:native:shape-payload#Absent".to_owned(),
                1 => {
                    changed[child].source_use = SourceUse::Child {
                        parent: changed[root].id.clone(),
                        slot: 1,
                    }
                }
                2 => changed[child].id = changed[root].id.clone(),
                3 => changed[child].source_use = SourceUse::Root { root: 1 },
                4 => changed[child].id = payload.id.clone(),
                5 => {
                    changed[child].source_use = SourceUse::Child {
                        parent: changed[child].id.clone(),
                        slot: 2,
                    }
                }
                6 => {
                    changed[child].source_use = SourceUse::Child {
                        parent: "fcstd:native:use#Absent".to_owned(),
                        slot: 2,
                    }
                }
                7 => changed[root].source_use = SourceUse::Root { root: 0 },
                _ => unreachable!(),
            }
            let mut modified = ns.clone();
            modified.set_arena(ctx, "topology_uses", &changed).unwrap();
            assert!(
                super::validate_namespace(ctx, &modified, std::slice::from_ref(&payload)).is_err()
            );
        }
        let mut deleted_parent = ns.clone();
        deleted_parent
            .set_arena(ctx, "topology_uses", &[rows[child].clone()])
            .unwrap();
        assert!(super::validate_namespace(ctx, &deleted_parent, &[payload]).is_err());
    });
}

#[test]
fn edited_routes_dangling_owners_and_cycles_fail_native_reference_validation() {
    context(|ctx| {
        let payload = alias_payload();
        let mut other = payload.clone();
        other.id = "fcstd:native:shape-payload#Other".to_owned();
        let mut ns = namespace(ctx, &payload);
        ns.set_arena(ctx, "shape_payloads", &[&payload, &other])
            .unwrap();
        let mut rows = ns.arena_as::<SourceUseSelector>("topology_uses").unwrap();
        let child = rows
            .iter()
            .position(|row| matches!(row.source_use, SourceUse::Child { .. }))
            .unwrap();
        rows[child].owner = other.id.clone();
        ns.set_arena(ctx, "topology_uses", &rows).unwrap();
        assert!(super::validate_namespace(ctx, &ns, &[payload, other]).is_err());
    });
}

#[test]
fn current_source_row_identities_are_opaque_and_typed_rewrites_preserve_selectors() {
    use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
    context(|ctx| {
        let mut payload = alias_payload();
        let mut ns = namespace(ctx, &payload);
        let rows = ns.arena_as::<SourceUseSelector>("topology_uses").unwrap();
        let owner = "fcstd:native:shape-payload#Renamed";
        let mut serial = 0;
        let mut map = IdentityMap::new(|id: &str| {
            Ok(if id == OWNER {
                owner.to_owned()
            } else {
                serial += 1;
                format!("opaque:native:use#{serial}")
            })
        });
        let mut renamed = Vec::new();
        for row in rows {
            let mut wire = serde_json::to_value(&row).unwrap();
            SourceUseSelector::rewrite_native_value(ctx, &mut wire, &mut map).unwrap();
            let actual = row.rewrite_identities(ctx, &mut map).unwrap();
            assert_eq!(serde_json::to_value(&actual).unwrap(), wire);
            renamed.push(actual);
        }
        payload.id = owner.to_owned();
        ns.set_arena(ctx, "shape_payloads", &[&payload]).unwrap();
        ns.set_arena(ctx, "topology_uses", &renamed).unwrap();
        ns.arenas_mut().get_mut("topology_uses").unwrap().reverse();
        assert_eq!(
            super::resolve_namespace(ctx, &ns, &[payload])
                .unwrap()
                .len(),
            2
        );
    });
}

#[test]
fn working_canonical_validation_does_not_admit_a_second_native_population() {
    let payload = alias_payload();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let ns = namespace(&ctx, &payload);
    let remaining = ctx.retained_bytes_available();
    super::validate_namespace(&ctx, &ns, &[payload]).unwrap();
    assert_eq!(ctx.retained_bytes_available(), remaining);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn existing_native_rows_reused_by_a_temporary_projection_do_not_admit_a_second_home() {
    let payload = alias_payload();
    let ns = context(|ctx| namespace(ctx, &payload));
    let before = ns.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let actual = super::resolve_namespace(&ctx, &ns, std::slice::from_ref(&payload)).unwrap();
    let existing = actual
        .into_iter()
        .map(|row| (row.node, row.id))
        .collect::<Vec<_>>();
    let tables = Tables::from_payload(&payload).unwrap();
    let mut locations = Locations::new(&ctx, tables).unwrap();
    let plan = super::Plan::discover(&ctx, tables, &mut locations).unwrap();
    let reused = Inventory::reuse(&ctx, plan, &existing).unwrap();
    assert!(reused.selector(1, 1, Located::identity()).is_some());
    let mut ns = ns;
    reused.install(&mut ns).unwrap();
    assert_eq!(ns, before);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn a_shared_wire_source_keeps_both_distinct_face_parents_available_for_inverse_roles() {
    context(|ctx| {
        let edge = TextTShape {
            geometry: TextTShapeGeometry::Edge {
                tolerance: FiniteReal::ZERO,
                same_parameter: false,
                same_range: false,
                degenerated: true,
                representations: Vec::new(),
            },
            flags: [false; 7],
            children: vec![use_shape(1, 0), use_shape(1, 0)],
        };
        let wire = TextTShape {
            geometry: TextTShapeGeometry::Wire,
            flags: [false; 7],
            children: vec![use_shape(2, 0)],
        };
        let face = TextTShape {
            geometry: TextTShapeGeometry::Face {
                natural_restriction: false,
                tolerance: FiniteReal::ZERO,
                surface: None,
                location: 0.into(),
                triangulation: None,
            },
            flags: [false; 7],
            children: vec![use_shape(3, 0)],
        };
        let compound = TextTShape {
            geometry: TextTShapeGeometry::Compound,
            flags: [false; 7],
            children: vec![use_shape(4, 0), use_shape(5, 0)],
        };
        let shapes = TextTShapes::from(vec![vertex(), edge, wire, face.clone(), face, compound]);
        let roots = [use_shape(6, 0)];
        let tables = tables(&shapes, &roots, &[]);
        let mut locations = Locations::new(ctx, tables).unwrap();
        let inventory = Inventory::admit_locations(ctx, OWNER, tables, &mut locations).unwrap();
        assert_eq!(inventory.rows().len(), 6);
        assert_ne!(
            inventory.selector(1, 4, Located::identity()),
            inventory.selector(1, 5, Located::identity())
        );
        let wire = inventory.selector(1, 3, Located::identity()).unwrap();
        let face = inventory.selector(1, 5, Located::identity()).unwrap();
        let root = inventory.selector(1, 6, Located::identity()).unwrap();
        let row = inventory.rows().iter().find(|row| row.id == wire).unwrap();
        assert_eq!(
            row.source_use,
            SourceUse::Child {
                parent: face.to_owned(),
                slot: 1
            }
        );
        let row = inventory.rows().iter().find(|row| row.id == face).unwrap();
        assert_eq!(
            row.source_use,
            SourceUse::Child {
                parent: root.to_owned(),
                slot: 2
            }
        );
        assert_eq!(
            inventory
                .plan
                .nodes
                .iter()
                .filter(|node| node.key.shape == 3)
                .count(),
            1
        );
    });
}

#[test]
fn inverse_working_text_and_lookup_use_actual_context_without_output_readmission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let value = "opaque:native:use#名前\\\"";
    assert_eq!(super::working_text(&ctx, value).unwrap(), value);
    assert_eq!(
        super::super::SourceOccurrenceKey::from_identity(&ctx, 17, 29).unwrap(),
        super::super::SourceOccurrenceKey::new(
            17,
            Located {
                identity: 29,
                ..Located::identity()
            }
        )
    );
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert_eq!(ctx.resource_refusal(), None);
    let mut working = Vec::<u64>::new();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(&mut working, usize::MAX, "source inverse prior refusal")
        .unwrap_err()
    else {
        panic!("capacity overflow must latch caller refusal")
    };
    assert!(
        matches!(super::working_text(&ctx, value), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert!(
        matches!(super::super::SourceOccurrenceKey::from_identity(&ctx, 17, 29), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert!(working.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

fn admit_topology_locations(
    ctx: &DecodeContext<'_>,
    tables: Tables<'_>,
    locations: &mut Locations,
) -> Result<(), LocationIdentityError> {
    for root in tables.roots {
        let root_location = locations.source(tables, root.location)?;
        let mut visited = HashSet::new();
        let mut stack = vec![(root.shape, Located::identity())];
        while let Some((shape_index, local)) = stack.pop() {
            if !locations.visit(ctx, &mut visited, shape_index, local)? {
                continue;
            }
            locations.admit(ctx, tables, root_location, local)?;
            let shape = tables.tshapes.resolve(shape_index)?;
            match &shape.geometry {
                TextTShapeGeometry::Face { location, .. } => {
                    let support = locations.source(tables, *location)?;
                    locations.admit(ctx, tables, local, support)?;
                }
                TextTShapeGeometry::Edge {
                    representations, ..
                } => {
                    for representation in representations {
                        let carrier = locations.source(tables, representation.location())?;
                        locations.admit(ctx, tables, local, carrier)?;
                    }
                }
                _ => {}
            }
            for child in &shape.children {
                let child_location = locations.source(tables, child.location)?;
                let child_location = locations.admit(ctx, tables, local, child_location)?;
                stack
                    .try_reserve(1)
                    .map_err(|_| locations::allocation_failed(ctx))?;
                stack.push((child.shape, child_location));
            }
        }
    }
    Ok(())
}

impl<'ctx, 'input> Inventory<'ctx, 'input> {
    pub(super) fn admit_locations(
        ctx: &'ctx DecodeContext<'input>,
        owner: &str,
        tables: Tables<'_>,
        locations: &mut Locations,
    ) -> Result<Self, LocationIdentityError> {
        let plan = Plan::discover(ctx, tables, locations)?;
        Self::materialize(ctx, owner, plan)
    }

    pub(super) fn rows(&self) -> &[SourceUseSelector] {
        match &self.identities {
            Identities::Fresh(rows) => rows,
            Identities::Existing(_) => &[],
        }
    }

    pub(super) fn install(self, namespace: &mut NativeNamespace) -> Result<(), CodecError> {
        let ctx = self.ctx;
        if let Some(records) = self.into_records()? {
            namespace
                .arenas_mut()
                .insert(working_text(ctx, "topology_uses")?, records);
        }
        Ok(())
    }
}
