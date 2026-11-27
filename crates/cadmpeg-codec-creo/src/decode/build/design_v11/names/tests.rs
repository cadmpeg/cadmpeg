// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::serde_erase::json;

fn feature(key: u32) -> FeatureId {
    FeatureId::mint(format!("{FEATURE}{key}")).expect("feature id")
}

fn scan() -> ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let surface = |id, feature_id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    scan.surfaces.rows.extend([
        surface(7, 10),
        surface(8, 99),
        surface(6, 10),
        surface(6, 11),
    ]);
    scan.curves
        .topology_rows
        .push(crate::curve::CurveTopologyRow {
            id: 9,
            type_byte: 8,
            feature_id: 10,
            directions: [1, 0xf6],
            faces: [std::num::NonZeroU32::new(7), std::num::NonZeroU32::new(8)],
            next_edges: [9, 9],
            offset: 0,
        });
    scan
}

fn geometry() -> CadIr {
    let mut ir = CadIr::empty();
    ir.geometry.faces = ["6", "7", "8"]
        .into_iter()
        .map(|key| {
            json::from_value(cadmpeg_core::json!({
                "id": format!("{FACE}{key}"),
                "shell": "creo:visibgeom:shell#1",
                "surface": format!("{SURFACE}{key}"),
                "sense": "forward",
                "loops": {"kind": "unspecified", "loops": []},
            }))
            .expect("face")
        })
        .collect();
    ir.geometry.edges = vec![json::from_value(cadmpeg_core::json!({
        "id": format!("{EDGE}9"),
        "carrier": {},
    }))
    .expect("edge")];
    ir
}

#[test]
fn an_entity_is_named_by_the_feature_its_geometry_id_records() {
    let scan = scan();
    let ir = geometry();
    let order = [feature(10), feature(11)];
    let names = Names::new(&scan, order.iter());
    let table = crate::decode::with_service_context(|ctx| names.table(ctx, &ir)).expect("table");
    let entry = |entity: EntityRef| table.entries().get(&entity).cloned().expect("entry");
    let face = entry(EntityRef::Face(ir.geometry.faces[1].id.clone()));
    assert_eq!(
        face.name,
        Some(TopoName::Generated {
            kind: NameKind::Face,
            feature: feature(10),
            role: None,
            generator: None,
            index: None,
        })
    );
    assert_eq!(face.native, Some(native(7)));
    let edge = entry(EntityRef::Edge(ir.geometry.edges[0].id.clone()));
    assert!(matches!(
        edge.name,
        Some(TopoName::Generated { kind: NameKind::Edge, ref feature, .. }) if *feature == self::feature(10)
    ));
}

#[test]
fn an_entity_whose_producer_the_design_lacks_or_two_rows_claim_keeps_only_its_native_name() {
    let scan = scan();
    let ir = geometry();
    let order = [feature(10), feature(11)];
    let names = Names::new(&scan, order.iter());
    let table = crate::decode::with_service_context(|ctx| names.table(ctx, &ir)).expect("table");
    for (face, id) in [(0, 6), (2, 8)] {
        let entry = table
            .entries()
            .get(&EntityRef::Face(ir.geometry.faces[face].id.clone()))
            .expect("entry");
        assert_eq!(entry.name, None);
        assert_eq!(entry.native, Some(native(id)));
    }
}

#[test]
fn a_pick_names_only_a_producer_before_the_picking_feature() {
    let scan = scan();
    let order = [feature(10), feature(11)];
    let names = Names::new(&scan, order.iter());
    assert!(matches!(
        names.pick(NameKind::Face, 7, &feature(11)).target,
        PickTarget::Entity(_)
    ));
    assert_eq!(
        names.pick(NameKind::Face, 7, &feature(10)).target,
        PickTarget::Native { within: None }
    );
    assert_eq!(
        names.pick(NameKind::Face, 7, &feature(10)).native,
        Some(native(7))
    );
}
