// SPDX-License-Identifier: Apache-2.0
//! Each shape property builds occurrence bindings only for its own map.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::{Codec, DecodeOptions};

#[test]
fn populated_a_and_empty_b_maps_build_only_a_topology_occurrences() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="2"><Object type="Part::Feature" name="A" id="1"/><Object type="Part::Feature" name="B" id="2"/></Objects><ObjectData Count="2"><Object name="A"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="1.0" file="A.brp"/><ElementMap file="A.map" count="1"/></Property></Properties></Object><Object name="B"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="1.0" file="B.brp"/><ElementMap file="B.map" count="0"/></Property></Properties></Object></ObjectData></Document>"#;
    let brep = "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 3\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *\n";
    let a = format!("{brep}+1 0 *");
    let b = format!("{brep}{}*", "+1 0 ".repeat(129));
    let input = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document), ("A.brp", a.as_bytes()), ("B.brp", b.as_bytes()),
        ("A.map", b"1 Edge1 mapped 0"), ("B.map", b"0"),
    ]);
    let decoded = crate::FcstdCodec.decode(&mut std::io::Cursor::new(&input), &DecodeOptions::default())
        .expect("mixed map archive");
    assert_eq!(decoded.ir().model.edges.len(), 130);
    let native = decoded.ir().native.namespace("fcstd").expect("native namespace");
    let payloads = native.arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads").expect("payloads");
    let properties = native.arena_as::<crate::native::PropertyRecord>("properties").expect("properties");
    let mut maps = native.arena_as::<crate::native::element_map::ElementMapRecord>("element_maps").expect("maps");
    assert_eq!(maps.len(), 2);
    let empty_map = maps[1].clone();
    assert!(empty_map.maps.root().groups.is_empty());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let (curves, surfaces) = crate::brep::transfer_text_geometry(&ctx, &payloads, &properties).expect("geometry");
    ir.model.curves = curves.curves;
    ir.model.surfaces = surfaces.surfaces;
    let occurrences = super::super::transfer(&ctx, &mut ir, &payloads, &properties, &mut Vec::new(), &maps)
        .expect("property-local occurrences");
    assert!(!occurrences.records.is_empty());
    assert!(occurrences.records.iter().all(|record| record.property == maps[0].property));
    assert_eq!(ir.model.edges.len(), 130);
    crate::element_map::bind_topology(&ctx, &mut maps, &occurrences.records).expect("binding");
    assert_eq!(maps[1], empty_map);
    assert!(!maps[0].maps.root().groups[0].names[1][0].topology_ids.is_empty());

    // Keep A's populated map, but transfer only B's payload. Any source-index
    // or occurrence-index work here has no consumer and must be absent.
    for operation in ["FreeCAD source topology roots", "FreeCAD element topology occurrence index"] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut ir = cadmpeg_ir::CadIr::empty();
        let only_b = payloads.iter().find(|payload| payload.property == empty_map.property).expect("B payload");
        let (curves, surfaces) = crate::brep::transfer_text_geometry(&ctx, std::slice::from_ref(only_b), &properties).expect("B geometry");
        ir.model.curves = curves.curves;
        ir.model.surfaces = surfaces.surfaces;
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let occurrences = super::super::transfer(&ctx, &mut ir, std::slice::from_ref(only_b), &properties, &mut Vec::new(), &maps)
            .expect("B has no binding consumer");
        assert!(occurrences.records.is_empty());
        assert_eq!(ir.model.edges.len(), 129);
        crate::element_map::bind_topology(&ctx, &mut maps, &occurrences.records).expect("no unused occurrence index");
        assert_eq!(maps[1], empty_map);
        assert_eq!(ctx.resource_refusal(), None);
        drop(probe);
    }
}
