// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};

use super::*;

/// A document holding one free circle decoded from the object `circle`.
fn free_circle() -> CadIr {
    let mut ir = CadIr::empty();
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: CurveId::mint("rhino:object:curve#circle").expect("identity grammar"),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("circle"),
        )),
        source_object: Some(SourceObjectAssociation {
            format: cadmpeg_ir::codec_id!("rhino"),
            object_id: cadmpeg_core::nonblank_literal!("circle"),
            geometry_role: None,
            instance_path: Vec::new(),
        }),
        native_ref: None,
    });
    ir
}

/// The losses and bindings stated for the free circle decoded with `display`.
fn state_with(display: ObjectDisplay) -> (Vec<LossNote>, Vec<AppearanceBinding>) {
    let mut ir = free_circle();
    let mut objects = ObjectPresentations::new();
    record(
        &mut objects,
        ("circle".to_owned(), Vec::new()),
        ObjectPresentation {
            display,
            layer_index: None,
        },
    );
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let losses = state(&ctx, &mut ir, &[], &objects).expect("state");
    (losses, ir.model.appearance_bindings)
}

#[test]
fn a_hidden_free_carrier_without_a_colour_charges_its_visibility() {
    let (losses, bindings) = state_with(ObjectDisplay {
        name: None,
        color: None,
        visible: Some(false),
    });
    assert!(bindings.is_empty(), "{bindings:#?}");
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [RhinoLossCode::FreeGeometryVisibilityNotStated.kind()]
    );
}

#[test]
fn a_hidden_free_carrier_with_a_colour_is_hidden_by_its_binding() {
    let (losses, bindings) = state_with(ObjectDisplay {
        name: None,
        color: Some(Color::from_rgba8(255, 0, 0, 255)),
        visible: Some(false),
    });
    assert!(losses.is_empty(), "{losses:#?}");
    let [binding] = bindings.as_slice() else {
        panic!("{bindings:#?}");
    };
    assert_eq!(binding.visible, Some(false));
}

#[test]
fn a_cage_body_is_placed_by_its_cage_object() {
    let mut ir = CadIr::empty();
    let body = BodyId::mint("rhino:object:body#cage").expect("identity grammar");
    ir.geometry.push_cage_object(
        cadmpeg_ir::SubdSurface {
            id: cadmpeg_ir::ids::SubdId::mint("rhino:object:subd#cage").expect("identity grammar"),
            scheme: cadmpeg_ir::SubdScheme::CatmullClark,
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::codec_id!("rhino"),
                object_id: cadmpeg_core::nonblank_literal!("cage"),
                geometry_role: None,
                instance_path: Vec::new(),
            }),
            cage: cadmpeg_ir::subd::SubdCage::default(),
            native_ref: None,
        },
        body.clone(),
        cadmpeg_ir::ids::RegionId::mint("rhino:object:region#cage").expect("identity grammar"),
        cadmpeg_ir::ids::ShellId::mint("rhino:object:shell#cage").expect("identity grammar"),
    );
    let presentation = ObjectPresentation {
        display: ObjectDisplay {
            name: None,
            color: None,
            visible: Some(true),
        },
        layer_index: Some(3),
    };
    let lookup = |association: Option<&SourceObjectAssociation>| {
        association
            .filter(|association| association.object_id.as_str() == "cage")
            .map(|_| &presentation)
    };
    assert_eq!(body_objects(&ir, &lookup), [(body, &presentation)]);
}
