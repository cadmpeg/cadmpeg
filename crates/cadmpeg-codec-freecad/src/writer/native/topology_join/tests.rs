// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;

impl<'a> TopologyJoin<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        ir: &'a CadIr,
        emitted: &'a [(String, Vec<EmittedTopologyUse>)],
    ) -> Result<Self, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let current = crate::topology_transfer::current_bindings(ctx, ir)?;
        Self::from_current(ctx, ir, current, emitted)
    }
}

fn fixture() -> CadIr {
    let document = br#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects>
<ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape">
<Part ElementMap="1.0" file="Shape.brp"/>
<ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap>
<ElementMap2 count="4">
1 PostfixCount 0 MapCount 1
ElementMap 1 1 2
Edge ChildCount 0 NameCount 3
0
;EdgeStable.0.a 0
;DeletedEdgeStable.0.a 0
Vertex ChildCount 0 NameCount 3
0
;VertexStable1.0.a 0
;VertexStable2.0.a 0
EndMap
</ElementMap2></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 3
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +3 0 -2 0 *
+1 0 +1 0 *";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", brep),
    ]);
    crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
        .ir()
        .clone()
}

fn map(ir: &CadIr) -> ElementMapRecord {
    ir.native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ElementMapRecord>("element_maps")
        .unwrap()
        .remove(0)
}

fn emit(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    bodies: &[&cadmpeg_ir::topology::Body],
) -> (
    Vec<(String, Vec<EmittedTopologyUse>)>,
    BTreeMap<String, Vec<u8>>,
) {
    let current = crate::topology_transfer::current_bindings(ctx, ir).unwrap();
    let roots = crate::writer::brep::RootKinds::new(ctx, ir, &current).unwrap();
    let shape = crate::writer::brep::serialize_with_uses(ctx, ir, bodies, &roots)
        .unwrap()
        .unwrap();
    (
        vec![("Shape".to_owned(), shape.uses)],
        BTreeMap::from([("Shape.brp".to_owned(), shape.bytes)]),
    )
}

#[test]
fn current_inverse_survives_arena_reordering_and_preserves_native_names() {
    let mut ir = fixture();
    let map = map(&ir);
    let before = serde_json::to_value(&map).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let join = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert!(join.map_resolves(&map, "Shape"));
    assert_eq!(
        join.current
            .selected_records(&map.property, "Edge", 1)
            .1
            .count(),
        2
    );
    assert_eq!(
        join.current
            .selected_records(&map.property, "Edge", 2)
            .1
            .count(),
        0
    );
    assert_eq!(
        map.maps.root().groups[0].names[2][0].encoded,
        ";DeletedEdgeStable.0.a"
    );
    ir.model.bodies.reverse();
    ir.model.regions.reverse();
    ir.model.shells.reverse();
    ir.model.edges.reverse();
    ir.model.vertices.reverse();
    ir.model.points.reverse();
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let join = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert!(join.map_resolves(&map, "Shape"));
    assert_eq!(serde_json::to_value(map).unwrap(), before);
}

#[test]
fn one_missing_actual_current_use_cannot_be_replaced_by_another_emitted_use() {
    let ir = fixture();
    assert_eq!(ir.model.bodies.len(), 2);
    assert_eq!(ir.model.edges.len(), 2);
    let map = map(&ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    // Serialize an actual subset; both current neutral source bindings still
    // exist, so a count or one emitted sibling is not enough for this map.
    let (emitted, entries) = emit(&ctx, &ir, &[&ir.model.bodies[0]]);
    let join = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert!(!join.map_resolves(&map, "Shape"));
    let mut object = Object::new("Shape".to_owned(), "Part::Feature", "Shape");
    object.set(
        "Shape",
        crate::writer::xml::property(
            "Shape",
            "Part::PropertyPartShape",
            &crate::writer::xml::element("Part", &[("file", "Shape.brp".to_owned())]),
        ),
    );
    let mut outcomes = crate::writer::dispositions::initial();
    join.record_emitted(&ctx, &[object], &entries, &mut outcomes)
        .unwrap();
    let first = ir.model.edges[0].native_ref.as_deref().unwrap();
    let second = ir.model.edges[1].native_ref.as_deref().unwrap();
    assert_ne!(first, second);
    assert_eq!(
        outcomes.record_disposition("fcstd", "topology_uses", first),
        NativeDisposition::Regenerated
    );
    assert_eq!(
        outcomes.record_disposition("fcstd", "topology_uses", second),
        NativeDisposition::Dropped
    );
}

#[test]
fn absent_inverse_bindings_fail_while_structurally_unlinked_native_names_stay_native() {
    let mut ir = fixture();
    let map = map(&ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    for edge in &mut ir.model.edges {
        edge.native_ref = None;
    }
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let join = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert!(!join.map_resolves(&map, "Shape"));
    // CADIR v8 #213 deletes historical topology_ids. This name has no actual
    // source selector, independently of its text. Existing native property,
    // object and shape gates remain outside this inverse check.
    let mut native = serde_json::to_value(&map).unwrap();
    native["maps"][0]["groups"][0]["names"][1] = serde_json::json!([]);
    native["maps"][0]["groups"][1]["names"][1] = serde_json::json!([]);
    native["maps"][0]["groups"][1]["names"][2] = serde_json::json!([]);
    let unlinked: ElementMapRecord = serde_json::from_value(native.clone()).unwrap();
    assert!(join.map_resolves(&unlinked, "Shape"));
    assert_eq!(serde_json::to_value(&unlinked).unwrap(), native);
    native["maps"][0]["groups"][0]["names"][2][0]["encoded"] =
        serde_json::json!(";AnyNativeName.0.a");
    let differently_named: ElementMapRecord = serde_json::from_value(native).unwrap();
    assert!(join.map_resolves(&differently_named, "Shape"));
}

#[test]
fn missing_selector_rows_do_not_reclassify_live_source_names_as_historical() {
    let mut ir = fixture();
    let map = map(&ir);
    for row in &mut ir.model.bodies {
        row.native_ref = None;
    }
    for row in &mut ir.model.regions {
        row.native_ref = None;
    }
    for row in &mut ir.model.shells {
        row.native_ref = None;
    }
    for row in &mut ir.model.faces {
        row.native_ref = None;
    }
    for row in &mut ir.model.loops {
        row.native_ref = None;
    }
    for row in &mut ir.model.edges {
        row.native_ref = None;
    }
    for row in &mut ir.model.vertices {
        row.native_ref = None;
    }
    ir.native
        .namespace_mut("fcstd")
        .arenas_mut()
        .remove("topology_uses");
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let join = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    let (needs_inverse, bindings) = join.current.selected_records(&map.property, "Edge", 1);
    assert!(needs_inverse);
    assert_eq!(bindings.count(), 0);
    assert!(!join.map_resolves(&map, "Shape"));
    let (needs_inverse, bindings) = join.current.selected_records(&map.property, "Edge", 2);
    assert!(!needs_inverse);
    assert_eq!(bindings.count(), 0);
}

fn face_roles_fixture(shared_wire: bool) -> CadIr {
    let kind = if shared_wire { "Wire" } else { "Shell" };
    let document = format!(
        r#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="1.0" file="Shape.brp"/><ElementMap new="1" count="1"><Element key="compat" value="compat"/></ElementMap><ElementMap2 count="1">
1 PostfixCount 0 MapCount 1
ElementMap 1 1 1
{kind} ChildCount 0 NameCount 2
0
;StableSource.0.a 0
EndMap
</ElementMap2></Property></Properties></Object></ObjectData></Document>"#
    );
    let brep = if shared_wire {
        "CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 6
Ve 0.001 0 0 0 0 0 1001000 *
Ed 0.001 1 1 1 0 1001000 +6 0 -6 0 *
Wi 1001000 +5 0 *
Fa 0 0.001 1 0 1001000 +4 0 *
Fa 0 0.001 1 0 1001000 +4 0 *
Sh 1001000 +3 0 -2 0 *
+1 0 *"
    } else {
        "CASCADE Topology V1, (c) Matra-Datavision
Locations 1
1 1 0 0 5 0 1 0 0 0 0 1 0
Curve2ds 0
Curves 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 1
1 0 0 0 0 0 1 1 0 0 0 1 0
Triangulations 0
TShapes 5
Ve 0.001 0 0 0 0 0 1001000 *
Ed 0.001 1 1 1 0 1001000 +5 0 -5 0 *
Wi 1001000 +4 0 *
Fa 0 0.001 1 0 1001000 +3 0 *
Sh 1001000 +2 0 +2 1 *
+1 0 *"
    };
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document.as_bytes()),
        ("Shape.brp", brep.as_bytes()),
    ]);
    crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
        .ir()
        .clone()
}

#[test]
fn missing_one_disconnected_shell_component_is_not_hidden_by_its_emitted_sibling() {
    let ir = face_roles_fixture(false);
    assert_eq!(ir.model.shells.len(), 2);
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.shells[0].native_ref, ir.model.shells[1].native_ref);
    let map = map(&ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let complete = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert_eq!(
        complete
            .current
            .selected_records(&map.property, "Shell", 1)
            .1
            .count(),
        2
    );
    assert!(complete.map_resolves(&map, "Shape"));
    let mut partial_output = ir.clone();
    partial_output.model.regions[0].shells.truncate(1);
    let (emitted, _) = emit(
        &ctx,
        &partial_output,
        &partial_output.model.bodies.iter().collect::<Vec<_>>(),
    );
    let partial = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert!(!partial.map_resolves(&map, "Shape"));
}

#[test]
fn missing_one_shared_wire_face_use_is_not_hidden_by_the_other_loop() {
    let ir = face_roles_fixture(true);
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.loops.len(), 2);
    assert_eq!(ir.model.loops[0].native_ref, ir.model.loops[1].native_ref);
    assert_ne!(ir.model.loops[0].face, ir.model.loops[1].face);
    let map = map(&ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let complete = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert_eq!(
        complete
            .current
            .selected_records(&map.property, "Wire", 1)
            .1
            .count(),
        2
    );
    assert!(complete.map_resolves(&map, "Shape"));
    let mut partial_output = ir.clone();
    partial_output.model.faces[1].loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    let (emitted, _) = emit(
        &ctx,
        &partial_output,
        &partial_output.model.bodies.iter().collect::<Vec<_>>(),
    );
    let partial = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    assert!(!partial.map_resolves(&map, "Shape"));
}

#[test]
fn final_shape_property_must_reference_the_actual_emitted_owner_file() {
    let ir = fixture();
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, entries) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let join = TopologyJoin::new(&ctx, &ir, &emitted).unwrap();
    for (kind, content, accepted) in [
        (
            "Part::PropertyPartShape",
            crate::writer::xml::element("Part", &[("file", "Shape.brp".to_owned())]),
            true,
        ),
        (
            "App::PropertyString",
            crate::writer::xml::element("String", &[("value", "BOM Shape collision".to_owned())]),
            false,
        ),
        (
            "Part::PropertyPartShape",
            crate::writer::xml::element("Part", &[("file", "Other.brp".to_owned())]),
            false,
        ),
    ] {
        let mut object = Object::new("Shape".to_owned(), "Part::Feature", "Shape");
        object.set(
            "Shape",
            crate::writer::xml::property("Shape", kind, &content),
        );
        assert_eq!(
            join.shape_matches(&ctx, &object, "Shape", &entries)
                .unwrap(),
            accepted
        );
        let mut outcomes = crate::writer::dispositions::initial();
        join.record_emitted(&ctx, &[object], &entries, &mut outcomes)
            .unwrap();
        let edge = ir.model.edges[0].native_ref.as_deref().unwrap();
        assert_eq!(
            outcomes.record_disposition("fcstd", "topology_uses", edge),
            if accepted {
                NativeDisposition::Regenerated
            } else {
                NativeDisposition::Dropped
            }
        );
    }
}

#[test]
fn actual_metadata_uses_the_caller_refusal_and_preserves_legacy_shape_bytes() {
    let ir = fixture();
    let bodies = ir.model.bodies.iter().collect::<Vec<_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let legacy = crate::writer::brep::tests::serialize(&ctx, &ir, &bodies)
        .unwrap()
        .unwrap();
    let current = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
    let roots = crate::writer::brep::RootKinds::new(&ctx, &ir, &current).unwrap();
    let actual = crate::writer::brep::serialize_with_uses(&ctx, &ir, &bodies, &roots)
        .unwrap()
        .unwrap();
    assert_eq!(actual.bytes, legacy);
    assert!(!actual.uses.is_empty());
    let mut working = Vec::<u64>::new();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(
            &mut working,
            usize::MAX,
            "source-use metadata prior refusal",
        )
        .unwrap_err()
    else {
        panic!("capacity overflow must latch caller refusal")
    };
    assert!(
        matches!(crate::writer::brep::serialize_with_uses(&ctx, &ir, &bodies, &roots), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert!(
        matches!(TopologyJoin::new(&ctx, &ir, &[]), Err(CodecError::ResourceLimit(actual)) if actual == refusal)
    );
    assert!(working.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn flattened_wire_body_proves_its_single_current_shell_through_the_actual_wire_root() {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part ElementMap="1.0" file="Shape.brp"/><ElementMap new="1" count="0"/><ElementMap2 count="2">
1 PostfixCount 0 MapCount 1
ElementMap 1 1 1
Wire ChildCount 0 NameCount 2
0
;WireStable.0.a 0
EndMap
</ElementMap2></Property></Properties></Object></ObjectData></Document>"#;
    let brep = b"CASCADE Topology V1, (c) Matra-Datavision
Locations 0
Curve2ds 0
Curves 1
1 0 0 0 1 0 0
Polygon3D 0
PolygonOnTriangulations 0
Surfaces 0
Triangulations 0
TShapes 4
Ve 0.001 0 0 0 0 0 1001000 *
Ve 0.001 1 0 0 0 0 1001000 *
Ed 0.001 1 1 0 1 1 0 0 1 0 1001000 +4 0 -3 0 *
Wi 1001000 +2 0 *
+1 0 *";
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", brep),
    ]);
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap();
    let ir = decoded.ir();
    assert_eq!(ir.model.shells.len(), 1);
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, _) = emit(&ctx, ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    let join = TopologyJoin::new(&ctx, ir, &emitted).unwrap();
    let map = map(ir);
    assert_eq!(
        join.current
            .selected_records(&map.property, "Wire", 1)
            .1
            .collect::<Vec<_>>(),
        [ir.model.shells[0].id.as_str()]
    );
    assert!(join.map_resolves(&map, "Shape"));
    assert!(emitted[0]
        .1
        .iter()
        .any(|usage| usage.neutral_id == ir.model.shells[0].id.as_str()
            && usage.kind == crate::brep::TextShapeKind::Wire));
    let mut missing = ir.clone();
    missing.model.shells[0] = cadmpeg_ir::topology::Shell::with_free_vertex(
        missing.model.shells[0].id.clone(),
        missing.model.shells[0].region.clone(),
        missing.model.vertices[0].id.clone(),
    );
    let (partial, _) = emit(
        &ctx,
        &missing,
        &missing.model.bodies.iter().collect::<Vec<_>>(),
    );
    assert!(!TopologyJoin::new(&ctx, ir, &partial)
        .unwrap()
        .map_resolves(&map, "Shape"));
    let mut ambiguous = ir.clone();
    let mut other = ambiguous.model.shells[0].clone();
    other.id =
        cadmpeg_ir::ids::ShellId::mint("test:model:shell#second-current-wire".to_owned()).unwrap();
    ambiguous.model.regions[0].shells.push(other.id.clone());
    ambiguous.model.shells.push(other);
    let (emitted, _) = emit(
        &ctx,
        &ambiguous,
        &ambiguous.model.bodies.iter().collect::<Vec<_>>(),
    );
    assert!(!TopologyJoin::new(&ctx, &ambiguous, &emitted)
        .unwrap()
        .map_resolves(&map, "Shape"));
}

fn native_root_kind_fixture(root: &str) -> CadIr {
    let document = br#"<Document SchemaVersion="4" FileVersion="1"><Objects Count="1"><Object type="Part::Feature" name="Shape" id="1"/></Objects><ObjectData Count="1"><Object name="Shape"><Properties Count="1"><Property name="Shape" type="Part::PropertyPartShape"><Part file="Shape.brp"/></Property></Properties></Object></ObjectData></Document>"#;
    let count = match root {
        "Fa" => 4,
        "Co" => 5,
        "CS" => 8,
        _ => panic!("the authored root fixture has no such kind"),
    };
    let mut brep = if root == "CS" {
        // A Solid shell is closed by balanced nondegenerate edge uses, not
        // by a stored closed flag. The one line is used in both directions
        // around this authored boundary, with distinct endpoint vertices.
        "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 1\n1 0 0 0 1 0 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes 8\nVe 0.001 0 0 0 0 0 1001000 *\nVe 0.001 1 0 0 0 0 1001000 *\nEd 0.001 1 1 0 1 1 0 0 1 0 1001000 +8 0 -7 0 *\nWi 1001000 +6 0 -6 0 *\nFa 0 0.001 1 0 1001000 +5 0 *\nSh 1001000 +4 0 *\nSo 1001000 +3 0 *\nCS 1001000 +2 0 *\n".to_owned()
    } else {
        format!(
        "CASCADE Topology V1, (c) Matra-Datavision\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 0\nPolygonOnTriangulations 0\nSurfaces 1\n1 0 0 0 0 0 1 1 0 0 0 1 0\nTriangulations 0\nTShapes {count}\nVe 0.001 0 0 0 0 0 1001000 *\nEd 0.001 1 1 1 0 1001000 +{count} 0 -{count} 0 *\nWi 1001000 +{} 0 *\nFa 0 0.001 1 0 1001000 +{} 0 *\n",
        count - 1,
        count - 2,
    )
    };
    if root == "Co" {
        brep.push_str("Co 1001000 +2 0 *\n");
    }
    brep.push_str("+1 0 *");
    let bytes = crate::test_support::test_archive::archive_entries(&[
        ("Document.xml", document),
        ("Shape.brp", brep.as_bytes()),
    ]);
    crate::FcstdCodec
        .decode(
            &mut Cursor::new(bytes),
            &DecodeOptions {
                depth: DecodeDepth::Geometry,
                ..DecodeOptions::default()
            },
        )
        .unwrap()
        .ir()
        .clone()
}

#[test]
fn current_native_compound_and_compsolid_root_kinds_survive_current_serialization() {
    for (code, expected) in [
        ("Co", crate::brep::TextShapeKind::Compound),
        ("CS", crate::brep::TextShapeKind::CompSolid),
    ] {
        let ir = native_root_kind_fixture(code);
        let ctx = cadmpeg_test_support::service_decode_context();
        let current = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
        let body = &ir.model.bodies[0];
        let native = current.native_for_record(body.id.as_str()).unwrap();
        assert_eq!(current.source_kind(native), Some(expected));
        let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
        assert!(emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == body.id.as_str()
                && usage.kind == expected
                && usage.native_shape > 0));
        assert!(ir.model.faces.iter().all(|face| emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == face.id.as_str()
                && usage.kind == crate::brep::TextShapeKind::Face)));
    }
}

#[test]
fn sole_native_face_root_uses_exact_current_wrapper_membership_and_references() {
    let mut ir = native_root_kind_fixture("Fa");
    let ctx = cadmpeg_test_support::service_decode_context();
    let body = &ir.model.bodies[0];
    let native = body.native_ref.as_deref().unwrap();
    assert_eq!(ir.model.regions[0].native_ref.as_deref(), Some(native));
    assert_eq!(ir.model.shells[0].native_ref.as_deref(), Some(native));
    assert_eq!(ir.model.faces[0].native_ref.as_deref(), Some(native));
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    for record in [
        body.id.as_str(),
        ir.model.regions[0].id.as_str(),
        ir.model.shells[0].id.as_str(),
        ir.model.faces[0].id.as_str(),
    ] {
        assert!(emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == record
                && usage.kind == crate::brep::TextShapeKind::Face));
    }
    ir.model.regions[0].native_ref = None;
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    assert!(emitted[0]
        .1
        .iter()
        .any(|usage| usage.neutral_id == ir.model.bodies[0].id.as_str()
            && usage.kind != crate::brep::TextShapeKind::Face));
    assert!(emitted[0]
        .1
        .iter()
        .any(|usage| usage.neutral_id == ir.model.faces[0].id.as_str()
            && usage.kind == crate::brep::TextShapeKind::Face));
}

#[test]
fn an_additional_current_shell_member_is_written_instead_of_collapsing_to_source_face() {
    let mut ir = native_root_kind_fixture("Fa");
    let mut vertex = ir.model.vertices[0].clone();
    vertex.id = cadmpeg_ir::ids::VertexId::mint("test:model:vertex#authored-extra-member").unwrap();
    vertex.native_ref = None;
    let vertex_id = vertex.id.clone();
    ir.model.vertices.push(vertex);
    let old = &ir.model.shells[0];
    let mut shell = cadmpeg_ir::topology::Shell::new(
        old.id.clone(),
        old.region.clone(),
        old.faces().to_vec(),
        old.wire_edges().to_vec(),
        vec![vertex_id.clone()],
    )
    .unwrap();
    shell.native_ref = old.native_ref.clone();
    ir.model.shells[0] = shell;
    let ctx = cadmpeg_test_support::service_decode_context();
    let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
    assert!(emitted[0]
        .1
        .iter()
        .any(|usage| usage.neutral_id == ir.model.bodies[0].id.as_str()
            && usage.kind != crate::brep::TextShapeKind::Face));
    assert!(emitted[0]
        .1
        .iter()
        .any(|usage| usage.neutral_id == vertex_id.as_str()
            && usage.kind == crate::brep::TextShapeKind::Vertex
            && usage.native_shape > 0));
    assert!(emitted[0]
        .1
        .iter()
        .any(|usage| usage.neutral_id == ir.model.faces[0].id.as_str()
            && usage.kind == crate::brep::TextShapeKind::Face));
}

#[test]
fn a_native_compsolid_root_does_not_override_current_sheet_or_wire_membership() {
    for kind in [
        cadmpeg_ir::topology::BodyKind::Sheet,
        cadmpeg_ir::topology::BodyKind::Wire,
    ] {
        let mut ir = native_root_kind_fixture("CS");
        ir.model.bodies[0].kind = kind;
        if kind == cadmpeg_ir::topology::BodyKind::Wire {
            let old = &ir.model.shells[0];
            let mut shell = cadmpeg_ir::topology::Shell::with_wire_edge(
                old.id.clone(),
                old.region.clone(),
                ir.model.edges[0].id.clone(),
            );
            shell.native_ref = old.native_ref.clone();
            ir.model.shells[0] = shell;
            ir.model.faces.clear();
            ir.model.loops.clear();
            ir.model.coedges.clear();
            ir.model.surfaces.clear();
        }
        let ctx = cadmpeg_test_support::service_decode_context();
        let current = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
        let native = current
            .native_for_record(ir.model.bodies[0].id.as_str())
            .unwrap();
        assert_eq!(
            current.source_kind(native),
            Some(crate::brep::TextShapeKind::CompSolid)
        );
        let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
        assert!(emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == ir.model.bodies[0].id.as_str()
                && usage.kind != crate::brep::TextShapeKind::CompSolid));
        assert!(emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == ir.model.edges[0].id.as_str()
                && usage.kind == crate::brep::TextShapeKind::Edge));
    }
}

#[test]
fn a_native_face_root_does_not_override_current_general_or_wire_body_kind() {
    for kind in [
        cadmpeg_ir::topology::BodyKind::General,
        cadmpeg_ir::topology::BodyKind::Wire,
    ] {
        let mut ir = native_root_kind_fixture("Fa");
        ir.model.bodies[0].kind = kind;
        if kind == cadmpeg_ir::topology::BodyKind::Wire {
            let old = &ir.model.shells[0];
            let mut shell = cadmpeg_ir::topology::Shell::with_wire_edge(
                old.id.clone(),
                old.region.clone(),
                ir.model.edges[0].id.clone(),
            );
            shell.native_ref = old.native_ref.clone();
            ir.model.shells[0] = shell;
            ir.model.faces.clear();
            ir.model.loops.clear();
            ir.model.coedges.clear();
            ir.model.surfaces.clear();
        }
        let ctx = cadmpeg_test_support::service_decode_context();
        let current = crate::topology_transfer::current_bindings(&ctx, &ir).unwrap();
        let native = current
            .native_for_record(ir.model.bodies[0].id.as_str())
            .unwrap();
        assert_eq!(
            current.source_kind(native),
            Some(crate::brep::TextShapeKind::Face)
        );
        let (emitted, _) = emit(&ctx, &ir, &ir.model.bodies.iter().collect::<Vec<_>>());
        assert!(emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == ir.model.bodies[0].id.as_str()
                && usage.kind != crate::brep::TextShapeKind::Face));
        assert!(emitted[0]
            .1
            .iter()
            .any(|usage| usage.neutral_id == ir.model.edges[0].id.as_str()
                && usage.kind == crate::brep::TextShapeKind::Edge));
    }
}
