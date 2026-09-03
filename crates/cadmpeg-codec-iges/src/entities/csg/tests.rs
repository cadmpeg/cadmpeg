// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::{Curve, CurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

const EPS_PROFILE_CLOSURE: f64 = 1.0e-9;

#[test]
fn profile_closure_rejects_conflicting_edge_occurrences() {
    let curve_id = CurveId("iges:model:curve#D1".into());
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: curve_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: PointId("closed-point".into()),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: PointId("open-point".into()),
            position: Point3::new(1.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: VertexId("closed-start".into()),
            point: PointId("closed-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("closed-end".into()),
            point: PointId("closed-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("open-start".into()),
            point: PointId("closed-point".into()),
            tolerance: None,
        },
        Vertex {
            id: VertexId("open-end".into()),
            point: PointId("open-point".into()),
            tolerance: None,
        },
    ]);
    ir.model.edges.extend([
        Edge {
            id: EdgeId("closed-occurrence".into()),
            curve: Some(curve_id.clone()),
            start: VertexId("closed-start".into()),
            end: VertexId("closed-end".into()),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        },
        Edge {
            id: EdgeId("open-occurrence".into()),
            curve: Some(curve_id),
            start: VertexId("open-start".into()),
            end: VertexId("open-end".into()),
            param_range: Some([0.0, 1.0]),
            tolerance: None,
        },
    ]);

    assert_eq!(super::profile_closed(&ir, 1, EPS_PROFILE_CLOSURE), None);
}

#[test]
fn decode_types_all_csg_primitive_solids_and_defaults() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(primitive_solids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let solids = &result.ir().native.namespace("iges").unwrap().arenas["primitive_solids"];
    assert_eq!(solids.len(), 8);
    let block = solids
        .iter()
        .find(|solid| solid.id() == "iges:solid:primitive#D1")
        .unwrap();
    assert_eq!(block.fields()["kind"], "block");
    assert_eq!(block.fields()["dimensions"]["x_length"], 2.0);
    assert_eq!(block.fields()["origin"][0], 1.0);
    let default_block = solids
        .iter()
        .find(|solid| solid.id() == "iges:solid:primitive#D3")
        .unwrap();
    assert!(default_block.fields()["origin"][0].is_null());
    assert_eq!(
        solids
            .iter()
            .map(|solid| solid.fields()["kind"].as_str().unwrap().to_owned())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(
            [
                "block",
                "ellipsoid",
                "right_angular_wedge",
                "right_circular_cone_frustum",
                "right_circular_cylinder",
                "sphere",
                "torus",
            ]
            .map(str::to_owned)
        )
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_rejects_invalid_csg_primitive_dimensions_semantically() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 160,
        form: 0,
        label: "TORUS".into(),
        status: "00000000",
        parameters: "160,1,2,0,0,0,0,0,1;".into(),
    }]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["primitive_solids"].len(),
        1
    );
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("primitive dimension invariant is violated")));
    assert!(!result.report().geometry_transferred());
}

#[test]
fn decode_applies_declared_real_significance_to_primitive_axes() {
    for (axes, decoded) in [
        (".8,.6,0,-.60000116,.8,0", true),
        (".8,.6,0,-.60000118,.8,0", false),
        (".8D0,.6D0,0,-.6000001D0,.8D0,0", false),
    ] {
        let bytes = owned_test_file(&[OwnedTestEntity {
            entity_type: 168,
            form: 0,
            label: "ELLIPSO".into(),
            status: "00000000",
            parameters: format!("168,3,2,1,0,0,0,{axes};"),
        }]);
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        assert_eq!(result.report().losses.is_empty(), decoded, "{axes}");
        if !decoded {
            assert!(result.report().losses[0]
                .message
                .contains("primitive axes are not orthonormal"));
        }
    }
}

#[test]
fn decode_types_swept_solids_and_balanced_boolean_postfix() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(procedural_and_boolean_solids_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let procedural = &native.arenas["procedural_solids"];
    assert_eq!(procedural.len(), 3);
    let open_revolution = procedural
        .iter()
        .find(|solid| solid.id() == "iges:solid:procedural#D5")
        .unwrap();
    assert_eq!(open_revolution.fields()["kind"], "revolution");
    assert_eq!(open_revolution.fields()["form"], 0);
    assert_eq!(open_revolution.fields()["amount"], 0.5);
    let closed_revolution = procedural
        .iter()
        .find(|solid| solid.id() == "iges:solid:procedural#D7")
        .unwrap();
    assert_eq!(closed_revolution.fields()["form"], 1);
    let extrusion = procedural
        .iter()
        .find(|solid| solid.id() == "iges:solid:procedural#D9")
        .unwrap();
    assert_eq!(extrusion.fields()["kind"], "linear_extrusion");
    let trees = &native.arenas["boolean_trees"];
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0].fields()["declared_length"], 3);
    assert_eq!(trees[0].fields()["terms"].as_array().unwrap().len(), 3);
    let selected = &native.arenas["selected_components"];
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].fields()["boolean_tree"],
        "iges:solid:boolean-tree#D15"
    );
    assert_eq!(selected[0].fields()["selection_point"][0], 1.0);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_applies_declared_real_significance_to_solid_sweep_axes() {
    for (axis, decoded) in [
        ("0,0,.9999995", true),
        ("0,0,.99999949", false),
        (".5773503D0,.5773503D0,.5773503D0", false),
    ] {
        let bytes = owned_test_file(&[
            OwnedTestEntity {
                entity_type: 100,
                form: 0,
                label: "PROFILE".into(),
                status: "00010000",
                parameters: "100,0,0,0,1,0,1,0;".into(),
            },
            OwnedTestEntity {
                entity_type: 164,
                form: 0,
                label: "EXTRUDE".into(),
                status: "00000000",
                parameters: format!("164,1,5,{axis};"),
            },
        ]);
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        let sweep_loss = result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("solid sweep axis is invalid"));
        assert_eq!(!sweep_loss, decoded, "{axis}");
    }
}

#[test]
fn decode_brackets_solid_profile_closure_at_the_global_resolution() {
    for (end, decoded) in [
        ("0.99999950099937551,0.000999", true),
        ("0.99999949899937457,0.001001", false),
    ] {
        let bytes = owned_test_file(&[
            OwnedTestEntity {
                entity_type: 100,
                form: 0,
                label: "PROFILE".into(),
                status: "00010000",
                parameters: format!("100,0,0,0,1,0,{end};"),
            },
            OwnedTestEntity {
                entity_type: 164,
                form: 0,
                label: "EXTRUDE".into(),
                status: "00000000",
                parameters: "164,1,5,0,0,1;".into(),
            },
        ]);
        let result = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();

        let closure_loss = result.report().losses.iter().any(|loss| {
            loss.message
                .contains("sweep form disagrees with profile closure")
        });
        assert_eq!(!closure_loss, decoded, "{end}");
    }
}

#[test]
fn decode_types_form_one_boolean_tree_with_brep_operand() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(explicit_tetrahedron_solid_with_boolean_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let trees = &result.ir().native.namespace("iges").unwrap().arenas["boolean_trees"];
    let tree = trees
        .iter()
        .find(|tree| tree.id() == "iges:solid:boolean-tree#D59")
        .unwrap_or_else(|| panic!("losses={:#?}", result.report().losses));
    assert_eq!(tree.fields()["form"], 1);
    assert_eq!(
        tree.fields()["terms"][0]["entity"],
        "iges:entity:directory#55"
    );
    let assembly = result.ir().native.namespace("iges").unwrap().arenas["solid_assemblies"]
        .iter()
        .find(|assembly| assembly.id() == "iges:product:solid-assembly#D61")
        .unwrap();
    assert_eq!(assembly.fields()["form"], 1);
    assert_eq!(
        assembly.fields()["items"][0]["item"],
        "iges:entity:directory#55"
    );
    let instance = result.ir().native.namespace("iges").unwrap().arenas["solid_instances"]
        .iter()
        .find(|instance| instance.id() == "iges:product:solid-instance#D63")
        .unwrap();
    assert_eq!(instance.fields()["form"], 1);
    assert_eq!(instance.fields()["solid"], "iges:entity:directory#55");
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn decode_requires_direct_brep_operand_for_boolean_form_one() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(nested_brep_boolean_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let trees = &result.ir().native.namespace("iges").unwrap().arenas["boolean_trees"];
    assert_eq!(trees.len(), 6);
    let invalid_entities = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
        .map(|loss| {
            loss.provenance
                .as_ref()
                .and_then(|provenance| provenance.tag.as_deref())
                .unwrap()
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        invalid_entities,
        BTreeSet::from([
            "directory_entry:D1".to_owned(),
            "directory_entry:D9".to_owned(),
            "directory_entry:D15".to_owned(),
            "directory_entry:D17".to_owned(),
        ])
    );
}

#[test]
fn decode_validates_selected_component_parameter_pointer() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "BOOL".into(),
            status: "00000200",
            parameters: "180,1,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000200",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "GOOD".into(),
            status: "00000200",
            parameters: "182,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "EVEN".into(),
            status: "00000200",
            parameters: "182,2,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "WRONG".into(),
            status: "00000200",
            parameters: "182,3,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "MISSING".into(),
            status: "00000200",
            parameters: "182,99,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 182,
            form: 0,
            label: "NEGATIVE".into(),
            status: "00000200",
            parameters: "182,-1,0,0,0;".into(),
        },
    ]);
    let even_pointer_offset = bytes
        .windows(5)
        .position(|window| window == b"182,2")
        .unwrap()
        + 4;
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let selected = &native.arenas["selected_components"];
    let component = |sequence| {
        selected
            .iter()
            .find(|component| {
                component.id() == format!("iges:solid:selected-component#D{sequence}")
            })
            .unwrap()
    };
    assert_eq!(
        component(5).fields()["boolean_tree"],
        "iges:solid:boolean-tree#D1"
    );
    assert!([7, 9, 11, 13]
        .into_iter()
        .all(|sequence| component(sequence).fields()["boolean_tree"].is_null()));

    for (sequence, resolution) in [
        (5, "resolved"),
        (7, "even_sequence"),
        (9, "wrong_type"),
        (11, "dangling"),
        (13, "out_of_range"),
    ] {
        let entity = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        let reference = &entity.fields()["references"][0];
        assert_eq!(reference["kind"], "parameter");
        assert_eq!(reference["parameter_index"], 1);
        assert_eq!(reference["expected"], "type-180-form-0-or-1");
        assert_eq!(reference["resolution"], resolution);
    }

    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::PointerUnresolved.kind())
            .count(),
        4
    );
    let even_loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.code == IgesLossCode::PointerUnresolved.kind()
                && loss.message.contains("D7 Parameter pointer 2")
        })
        .unwrap();
    let provenance = even_loss.provenance.as_ref().unwrap();
    assert_eq!(provenance.offset, even_pointer_offset as u64);
    assert_eq!(provenance.tag.as_deref(), Some("D7:parameter[1]"));
}

#[test]
fn decode_rejects_cyclic_boolean_tree_references() {
    let bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 158,
            form: 0,
            label: "SPHERE".into(),
            status: "00000000",
            parameters: "158,1,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "TREE1".into(),
            status: "00000000",
            parameters: "180,3,-1,-5,1;".into(),
        },
        OwnedTestEntity {
            entity_type: 180,
            form: 0,
            label: "TREE2".into(),
            status: "00000000",
            parameters: "180,3,-1,-3,1;".into(),
        },
    ]);
    let result = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        result.ir().native.namespace("iges").unwrap().arenas["boolean_trees"].len(),
        2
    );
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss
                .message
                .contains("Boolean operands, form, or reference acyclicity is invalid"))
            .count(),
        2
    );
}
