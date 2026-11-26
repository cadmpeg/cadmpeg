// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::serde_erase::json;
use serde_json::json;
use std::rc::Rc;

fn model() -> CadIr {
    let mut partial = json!({
        "bodies": [{"id":"test:model:body#b", "kind":"solid"}],
        "regions": [{"id":"test:model:region#r", "body":"test:model:body#b", "shells":["test:model:shell#s"]}],
        "shells": [{"id":"test:model:shell#s", "region":"test:model:region#r", "members":[]}],
        "faces": (0..3).map(|i| json!({"id":format!("test:model:face#f{i}"),"shell":"test:model:shell#s", "surface":format!("test:model:surface#g{i}"), "sense":"forward", "loops":{"kind":"unspecified","loops":if i<2 {vec![format!("test:model:loop#l{i}")]} else {vec![]}}})).collect::<Vec<_>>(),
        "loops": (0..2).map(|i|json!({"id":format!("test:model:loop#l{i}"), "face":format!("test:model:face#f{i}"), "boundary":{"kind":"ring","coedges":[format!("test:model:coedge#c{i}")], "vertex_uses": []}})).collect::<Vec<_>>(),
        "coedges": (0..2).map(|i|json!({"id":format!("test:model:coedge#c{i}"),"owner_loop":format!("test:model:loop#l{i}"), "edge":format!("test:model:edge#e{i}"), "radial_next":format!("test:model:coedge#c{i}"), "sense":"forward", "pcurves": []})).collect::<Vec<_>>(),
        "edges": (0..2).map(|i|json!({"id":format!("test:model:edge#e{i}"), "carrier":{"curve":format!("test:model:curve#c{i}"),"param_range":[0.0,std::f64::consts::TAU]}, "ends":{"start":format!("test:model:vertex#v{i}"), "end":format!("test:model:vertex#v{i}")}})).collect::<Vec<_>>(),
        "curves": (0..2).map(|i|json!({"id":format!("test:model:curve#c{i}"), "geometry":{"kind": "solved", "value": {"kind":"circle", "center":{"x":0.0,"y":0.0,"z":f64::from(i)*5.0},"axis":{"x":0.0,"y":0.0,"z":1.0},"ref_direction":{"x":1.0,"y":0.0,"z":0.0},"radius":2.0}}})).collect::<Vec<_>>(),
        "surfaces": [
            {"id":"test:model:surface#g0", "geometry":{"kind": "solved", "value": {"kind":"plane","origin":{"x":0.0,"y":0.0,"z":0.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}}},
            {"id":"test:model:surface#g1", "geometry":{"kind": "solved", "value": {"kind":"plane","origin":{"x":0.0,"y":0.0,"z":5.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}}},
            {"id":"test:model:surface#g2", "geometry":{"kind": "solved", "value": {"kind":"cylinder","origin":{"x":0.0,"y":0.0,"z":0.0},"axis":{"x":0.0,"y":0.0,"z":1.0},"ref_direction":{"x":1.0,"y":0.0,"z":0.0},"radius":2.0}}}
        ],
        "attributes": (0..3).map(|i| {
            let mut values = vec![json!({"kind":"reference","value":"x"});5];
            values.push(json!({"kind":"string","value":"INV_NMX_SWEEPGENERATED_TAG"}));
            values.extend([1,44,1,17,-1].into_iter().map(|value|json!({"kind":"integer","value":value})));
            values.extend(match i {0=>vec![0,0,4],1=>vec![0,0,12],_=>vec![8,9,20]}.into_iter().chain([0;7]).map(|value|json!({"kind":"integer","value":value})));
            json!({"id":format!("test:model:attribute#a{i}"),"name":"ATTRIB_CUSTOM-attrib","target":{"entity":{"kind":"face","value":format!("test:model:face#f{i}")}},"values":values})
        }).collect::<Vec<_>>()
    });
    let partial = partial.as_object_mut().expect("partial model");
    let attributes = partial.remove("attributes").expect("attributes");
    let mut ir = CadIr::empty();
    let mut geometry = json::to_value(&ir.geometry).expect("empty topology");
    geometry
        .as_object_mut()
        .expect("topology object")
        .extend(partial.clone());
    ir.geometry = json::from_value(geometry).expect("synthetic owned solid");
    ir.model.attributes = json::from_value(attributes).expect("synthetic attributes");
    ir
}

#[test]
fn sweep_requires_its_own_caps_complete_spans_and_unchanged_radius() {
    let model = model();
    let ends = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)];
    assert!(sweep(&model, 17, 2.0, 8, &[(9, None)], ends).is_some());
    assert!(sweep(&model, 18, 2.0, 8, &[(9, None)], ends).is_none());
    assert!(sweep(&model, 17, 3.0, 8, &[(9, None)], ends).is_none());
    assert!(sweep(&cylinder_model(3.0), 17, 2.0, 8, &[(9, None)], ends).is_none());
    assert!(sweep(&model, 17, 2.0, 7, &[(9, None)], ends).is_none());
    assert!(sweep(&model, 17, 2.0, 8, &[(9, None), (10, None)], ends).is_none());
    assert!(sweep(
        &model,
        17,
        2.0,
        8,
        &[(9, None)],
        [ends[0], Point3::new(0.0, 0.0, 4.0)]
    )
    .is_none());
    assert!(loft(&model, 17, [19, 23], |section, name| [(0, 8), (1, 3)]
        .contains(&(section, name)))
    .is_none());
    let mut shifted = model.clone();
    shifted.geometry.curves[1].geometry =
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(1.0, 0.0, 5.0),
                cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("shifted circle"),
        ));
    assert!(sweep(&shifted, 17, 2.0, 8, &[(9, None)], ends).is_none());

    let mut missing = model.clone();
    missing.model.attributes.pop();
    assert!(sweep(&missing, 17, 2.0, 8, &[(9, None)], ends).is_none());
    let mut duplicate = model;
    duplicate
        .model
        .attributes
        .push(duplicate.model.attributes[0].clone());
    assert!(sweep(&duplicate, 17, 2.0, 8, &[(9, None)], ends).is_none());
}

fn cylinder_model(radius: f64) -> CadIr {
    let mut model = model();
    model.geometry.surfaces[2].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .expect("cylinder"),
    ));
    model
}

fn line_span() -> PathCurve {
    PathCurve::Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(0.0, 0.0, 5.0),
    }
}

fn torus_model(center: Point3) -> CadIr {
    torus_model_with_minor(center, 2.0)
}

fn torus_model_with_minor(center: Point3, minor: f64) -> CadIr {
    let mut model = model();
    model.geometry.surfaces[2].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            center,
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            6.0,
            minor,
        )
        .expect("torus"),
    ));
    model
}

#[test]
fn a_line_joined_span_requires_a_cylinder_on_the_line() {
    let ends = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)];
    let model = model();
    assert!(sweep(&model, 17, 2.0, 8, &[(9, Some(line_span()))], ends).is_some());
    let off_axis = Some(PathCurve::Line {
        start: Point3::new(1.0, 0.0, 0.0),
        end: Point3::new(1.0, 0.0, 5.0),
    });
    assert!(sweep(&model, 17, 2.0, 8, &[(9, off_axis)], ends).is_none());
    let skew = Some(PathCurve::Line {
        start: Point3::new(0.0, 0.0, 0.0),
        end: Point3::new(1.0, 0.0, 5.0),
    });
    assert!(sweep(&model, 17, 2.0, 8, &[(9, skew)], ends).is_none());
    let arc = Some(PathCurve::Arc {
        center: Point3::new(0.0, 0.0, 0.0),
        radius: 6.0,
    });
    assert!(sweep(&model, 17, 2.0, 8, &[(9, arc)], ends).is_none());
    let torus = torus_model(Point3::new(0.0, 0.0, 0.0));
    assert!(sweep(&torus, 17, 2.0, 8, &[(9, Some(line_span()))], ends).is_none());
}

#[test]
fn an_arc_joined_span_requires_a_torus_about_the_arc() {
    let ends = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)];
    let arc = |center: Point3, radius: f64| Some(PathCurve::Arc { center, radius });
    let torus = torus_model(Point3::new(1.0, 2.0, 3.0));
    let center = Point3::new(1.0, 2.0, 3.0);
    assert!(sweep(&torus, 17, 2.0, 8, &[(9, arc(center, 6.0))], ends).is_some());
    assert!(sweep(
        &torus,
        17,
        2.0,
        8,
        &[(9, arc(Point3::new(1.0, 2.0, 4.0), 6.0))],
        ends
    )
    .is_none());
    assert!(sweep(&torus, 17, 2.0, 8, &[(9, arc(center, 5.0))], ends).is_none());
    assert!(sweep(&model(), 17, 2.0, 8, &[(9, arc(center, 6.0))], ends).is_none());
}

#[test]
fn a_span_without_a_joined_curve_keeps_only_the_radius_check() {
    let ends = [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 5.0)];
    let unjoined = [(9, None)];
    let torus = torus_model(Point3::new(1.0, 2.0, 3.0));
    assert!(sweep(&torus, 17, 2.0, 8, &unjoined, ends).is_some());
    assert!(sweep(&model(), 17, 2.0, 8, &unjoined, ends).is_some());
    // The caps keep the section radius; only the side surface differs.
    let wide_torus = torus_model_with_minor(Point3::new(1.0, 2.0, 3.0), 2.5);
    assert!(sweep(&wide_torus, 17, 2.0, 8, &unjoined, ends).is_none());
    assert!(sweep(&cylinder_model(2.5), 17, 2.0, 8, &unjoined, ends).is_none());
    // A joined span on the same faces applies the curve checks.
    let line = [(9, Some(line_span()))];
    assert!(sweep(&torus, 17, 2.0, 8, &line, ends).is_none());
}

#[test]
fn loft_result_requires_two_positional_sections_without_support_or_guidance() {
    let base = loft_payload();
    let admitted = |value| {
        json::from_value::<cadmpeg_ir::geometry::surface_payloads::LoftSurfacePayload>(value)
            .expect("synthetic loft payload")
    };
    assert!(unconstrained_loft(&admitted(base.clone())));
    for field in ["surface", "direction", "first_flag"] {
        let mut value = base.clone();
        value["sections"][0]["entries"][0]["profile"][0]["form"][field] = match field {
            "surface" => json!("test:model:surface#support"),
            "direction" => json!({"x":0.0,"y":0.0,"z":1.0}),
            _ => json!(true),
        };
        assert!(!unconstrained_loft(&admitted(value)));
    }
    let mut closed = base.clone();
    closed["closures"] = json!([0, 1]);
    assert!(!unconstrained_loft(&admitted(closed)));
    let mut guided = base;
    guided["sections"][0]["entries"][0]["path"]["path"] = json!({"curve":"test:model:curve#guide"});
    assert!(!unconstrained_loft(&admitted(guided)));
}

fn loft_payload() -> serde_json::Value {
    let member = json!({
        "profile":{"curve":"test:model:curve#curve"},
        "form":{"kind":"support", "type_code":1, "support_bounds":[null,null,null,null], "first_flag":false,
            "subdata":{"kind":"type211", "dimensions":[1,1], "row":[0.0,1.0]}}
    });
    let entry = json!({"parameter":0.0, "profile":[member], "path":{"auxiliaries":[], "flag":-1}});
    json!({
        "sections":[{"entries":[entry,entry]}, {"entries":[]}],
        "parameters":{"kind":"ordered_ranges", "ranges":[[0.0,1.0],[0.0,1.0]]},
        "closures":[0,0],"singularities":[0,0],"mode":0,"bridge":[],"cache":{"kind":"legacy"}
    })
}

#[test]
fn loft_caps_and_side_members_must_name_the_ordered_section_sketches() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis},
        ProceduralSurface, SurfaceGeometry,
    };
    let mut model = model();
    for (i, attribute) in model.model.attributes.iter_mut().enumerate() {
        attribute.values.truncate(11);
        let (family, values) = match i {
            0 => ("INV_NMX_GEN_TAG", vec![0, 19, 260]),
            1 => ("INV_NMX_GEN_TAG", vec![0, 23, 260]),
            _ => (
                "INV_NMX_ORDERED_GEN_TAG",
                vec![2, 8, 19, 258, 0, 0, 3, 23, 258, 4, 1],
            ),
        };
        attribute.values[5] = Some(AttributeValue::String(family.into()));
        attribute.values.extend(
            values
                .into_iter()
                .map(|value| Some(AttributeValue::Integer(value))),
        );
    }
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let poles = NurbsPoleGrid::from_lanes(
        vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)],
        ],
        None,
    )
    .expect("grid");
    let nurbs = NurbsSurface::new(axis(), axis(), poles, false).expect("surface");
    let id = cadmpeg_ir::ids::ProceduralSurfaceId::mint("test:model:procedural-surface#loft")
        .expect("id");
    let payload = json::from_value(loft_payload()).expect("payload");
    model
        .geometry
        .procedural_surfaces
        .push(ProceduralSurface::new(
            id.clone(),
            ProceduralSurfaceDefinition::Loft(payload),
            None,
        ));
    model.geometry.surfaces[2].geometry = SurfaceGeometry::Procedural {
        construction: id,
        cache: Some(SolvedSurfaceGeometry::Nurbs(nurbs)),
    };
    assert!(loft(&model, 17, [19, 23], |section, name| [(0, 8), (1, 3)]
        .contains(&(section, name)))
    .is_some());
    assert!(loft(&model, 17, [23, 19], |section, name| [(0, 8), (1, 3)]
        .contains(&(section, name)))
    .is_none());
    assert!(loft(&model, 17, [19, 24], |section, name| [(0, 8), (1, 3)]
        .contains(&(section, name)))
    .is_none());
    let mut different_last_curve = model.clone();
    different_last_curve.model.attributes[2].values[17] = Some(AttributeValue::Integer(5));
    assert!(loft(&different_last_curve, 17, [19, 23], |section, name| [
        (0, 8),
        (1, 5)
    ]
    .contains(&(section, name)))
    .is_some());
    assert!(loft(&different_last_curve, 17, [19, 23], |section, name| [
        (0, 8),
        (1, 3)
    ]
    .contains(&(section, name)))
    .is_none());
    let side_values = |values: [i64; 11]| {
        let mut swapped = model.clone();
        for (slot, value) in swapped.model.attributes[2].values[11..]
            .iter_mut()
            .zip(values)
        {
            *slot = Some(AttributeValue::Integer(value));
        }
        swapped
    };
    let member_tail_first = side_values([2, 3, 23, 258, 4, 1, 8, 19, 258, 0, 0]);
    assert!(loft(&member_tail_first, 17, [23, 19], |section, name| [
        (0, 3),
        (1, 8)
    ]
    .contains(&(section, name)))
    .is_some());
    assert!(loft(&member_tail_first, 17, [19, 23], |section, name| [
        (0, 8),
        (1, 3)
    ]
    .contains(&(section, name)))
    .is_none());
    for tails in [[4, 1, 4, 1], [0, 0, 0, 0]] {
        let unpaired = side_values([
            2, 3, 23, 258, tails[0], tails[1], 8, 19, 258, tails[2], tails[3],
        ]);
        assert!(
            loft(&unpaired, 17, [23, 19], |section, name| [(0, 3), (1, 8)]
                .contains(&(section, name)))
            .is_none()
        );
    }
    let mut wrong_curve = model.clone();
    wrong_curve.model.attributes[2].values[12] = Some(AttributeValue::Integer(99));
    assert!(
        loft(&wrong_curve, 17, [19, 23], |section, name| [(0, 8), (1, 3)]
            .contains(&(section, name)))
        .is_none()
    );
    model.model.attributes[2].values[21] = Some(AttributeValue::Integer(2));
    assert!(loft(&model, 17, [19, 23], |section, name| [(0, 8), (1, 3)]
        .contains(&(section, name)))
    .is_none());
}

const CAP_CLASS: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";
const SIDE_CLASS: &str =
    "NMx_Loft_Surface_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";

/// Two caps and one planar side of generator 17, plus one planar face named
/// by a later generator 99.
fn legacy_model() -> CadIr {
    let mut model = model();
    model.geometry.surfaces[2].geometry = json::from_value(json!({"kind":"solved","value":{
        "kind":"plane","origin":{"x":0.0,"y":0.0,"z":0.0},
        "normal":{"x":1.0,"y":0.0,"z":0.0},"u_axis":{"x":0.0,"y":1.0,"z":0.0}
    }}))
    .expect("plane");
    let mut later = model.geometry.faces[2].clone();
    later.id = json::from_value(json!("test:model:face#f3")).expect("id");
    model.geometry.faces.push(later);
    model
        .model
        .attributes
        .push(model.model.attributes[2].clone());
    let face = |i: usize| {
        AttributeTarget::Face(
            json::from_value(json!(format!("test:model:face#f{i}"))).expect("face id"),
        )
    };
    let attributes = [
        (CAP_CLASS, 0, 17, vec![0, 19, 260]),
        (CAP_CLASS, 1, 17, vec![0, 23, 260]),
        (
            SIDE_CLASS,
            2,
            17,
            vec![2, 8, 19, 258, 0, 0, 3, 23, 258, 0, 0],
        ),
        (
            SIDE_CLASS,
            3,
            99,
            vec![2, 5, 19, 258, 0, 0, 5, 23, 258, 0, 0],
        ),
    ];
    for (attribute, (class, target, generator, tail)) in
        model.model.attributes.iter_mut().zip(attributes)
    {
        attribute.name = class.into();
        attribute.target = face(target).into();
        attribute.values.truncate(5);
        attribute.values.extend(
            [38, 1, generator, -1]
                .into_iter()
                .chain(tail)
                .map(|value| Some(AttributeValue::Integer(value))),
        );
    }
    model
}

fn legacy_curves(section: usize) -> Vec<i64> {
    [vec![8], vec![3]][section].clone()
}

#[test]
fn legacy_loft_requires_both_caps_and_one_side_for_each_section_curve() {
    let model = legacy_model();
    assert!(legacy_loft(&model, 17, [19, 23], legacy_curves).is_some());
    assert!(loft_result(&model, 17, [19, 23], legacy_curves).is_some());
    // A later generator owns its own faces.
    assert!(legacy_loft(&model, 99, [19, 23], |_| vec![5]).is_none());
    assert!(legacy_loft(&model, 17, [23, 19], legacy_curves).is_none());
    assert!(legacy_loft(&model, 17, [19, 19], legacy_curves).is_none());
    assert!(legacy_loft(&model, 18, [19, 23], legacy_curves).is_none());
    // A section boundary with another curve has no side face for it.
    assert!(legacy_loft(&model, 17, [19, 23], |section| {
        if section == 0 {
            vec![8, 9]
        } else {
            vec![3]
        }
    })
    .is_none());
    assert!(legacy_loft(&model, 17, [19, 23], |_| Vec::new()).is_none());
    assert!(
        legacy_loft(&model, 17, [19, 23], |section| [vec![8], vec![4]][section]
            .clone())
        .is_none()
    );
}

#[test]
fn legacy_loft_refuses_a_cap_side_or_name_that_is_not_the_positional_form() {
    let base = legacy_model();
    let check = |model: &CadIr| legacy_loft(model, 17, [19, 23], legacy_curves);
    let mut missing_cap = base.clone();
    missing_cap.model.attributes.remove(1);
    assert!(check(&missing_cap).is_none());
    let mut duplicate_cap = base.clone();
    duplicate_cap.model.attributes[1].values[11] = Some(AttributeValue::Integer(19));
    assert!(check(&duplicate_cap).is_none());
    let mut foreign_cap = base.clone();
    foreign_cap.model.attributes[1].values[11] = Some(AttributeValue::Integer(40));
    assert!(check(&foreign_cap).is_none());
    // The side tail carries no member index; a member tail is the other carrier's form.
    let mut member = base.clone();
    member.model.attributes[2].values[18] = Some(AttributeValue::Integer(4));
    member.model.attributes[2].values[19] = Some(AttributeValue::Integer(1));
    assert!(check(&member).is_none());
    let mut curve_tail = base.clone();
    curve_tail.model.attributes[2].values[14] = Some(AttributeValue::Integer(257));
    assert!(check(&curve_tail).is_none());
    let mut duplicate_side = base.clone();
    let mut side = duplicate_side.model.attributes[2].clone();
    side.target =
        AttributeTarget::Face(json::from_value(json!("test:model:face#f3")).expect("face id"))
            .into();
    side.values[7] = Some(AttributeValue::Integer(17));
    duplicate_side.model.attributes[3] = side;
    assert!(check(&duplicate_side).is_none());
    // Two names on one face: the face is not solely the generator's.
    let mut shared = base.clone();
    shared.model.attributes[2].values[6] = Some(AttributeValue::Integer(2));
    shared.model.attributes[2]
        .values
        .insert(9, Some(AttributeValue::Integer(5)));
    shared.model.attributes[2]
        .values
        .insert(10, Some(AttributeValue::Integer(-1)));
    assert!(check(&shared).is_none());
    // A side face on a non-planar surface.
    let mut curved = base.clone();
    curved.geometry.surfaces[2].geometry = model().geometry.surfaces[2].geometry.clone();
    assert!(check(&curved).is_none());
    // The generator names a face on a second body.
    let mut other_body = base.clone();
    let mut body = other_body.geometry.bodies[0].clone();
    body.id = json::from_value(json!("test:model:body#b2")).expect("id");
    let mut region = other_body.geometry.regions[0].clone();
    region.id = json::from_value(json!("test:model:region#r2")).expect("id");
    region.body = body.id.clone();
    let mut shell = other_body.geometry.shells[0].clone();
    shell.id = json::from_value(json!("test:model:shell#s2")).expect("id");
    shell.region = region.id.clone();
    other_body.geometry.faces[3].shell = shell.id.clone();
    other_body.model.attributes[3].values[7] = Some(AttributeValue::Integer(17));
    other_body.model.attributes[3].values[11] = Some(AttributeValue::Integer(0));
    other_body.geometry.bodies.push(body);
    other_body.geometry.regions.push(region);
    other_body.geometry.shells.push(shell);
    assert!(check(&other_body).is_none());
}

/// A source of history-state models that answers every request with one
/// fixed outcome and records the source indices it was asked for.
struct FixedStates {
    outcome: fn(&CadIr) -> StateModel,
    model: CadIr,
    asked: std::cell::RefCell<Vec<u32>>,
}

impl FixedStates {
    fn new(model: CadIr, outcome: fn(&CadIr) -> StateModel) -> Self {
        Self {
            outcome,
            model,
            asked: std::cell::RefCell::default(),
        }
    }

    fn holding(model: CadIr) -> Self {
        Self::new(model, |model| StateModel::Model(Box::new(model.clone())))
    }
}

impl ResultStates for FixedStates {
    fn after(
        &self,
        _: &DecodeContext<'_>,
        source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        self.asked.borrow_mut().push(source_index);
        Ok(Rc::new((self.outcome)(&self.model)))
    }
}

fn proof(final_model: Option<&CadIr>, states: Option<&dyn ResultStates>) -> Result<(), Unproved> {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    loft_proof(&ctx, final_model, states, 17, [19, 23], legacy_curves)
}

/// The legacy loft of `legacy_model` after a later feature consumed the cap
/// of the first section: no face carries that cap name.
fn consumed_cap() -> CadIr {
    let mut model = legacy_model();
    model.model.attributes.remove(0);
    model
}

/// The legacy loft after a later feature copied its first cap: a second face
/// carries the name of the cap.
fn copied_cap() -> CadIr {
    let mut model = legacy_model();
    let mut copy = model.model.attributes[0].clone();
    copy.target =
        AttributeTarget::Face(json::from_value(json!("test:model:face#f3")).expect("face id"))
            .into();
    model.model.attributes[3] = copy;
    model
}

#[test]
fn a_complete_final_model_proves_the_loft_without_a_state() {
    let complete = legacy_model();
    assert!(proof(Some(&complete), None).is_ok());
    let states = FixedStates::new(CadIr::empty(), |_| StateModel::Unavailable);
    assert!(proof(Some(&complete), Some(&states)).is_ok());
    assert!(states.asked.borrow().is_empty());
}

#[test]
fn a_loft_whose_cap_a_later_feature_consumed_or_copied_is_proved_by_its_state() {
    for later in [consumed_cap(), copied_cap()] {
        assert!(loft_result(&later, 17, [19, 23], legacy_curves).is_none());
        assert!(matches!(proof(Some(&later), None), Err(Unproved::Result)));
        assert!(matches!(proof(None, None), Err(Unproved::Result)));
        let states = FixedStates::holding(legacy_model());
        assert!(proof(Some(&later), Some(&states)).is_ok());
        assert!(proof(None, Some(&states)).is_ok());
        assert_eq!(*states.asked.borrow(), [17, 17]);
    }
}

#[test]
fn the_state_must_hold_the_complete_result_itself() {
    let later = consumed_cap();
    let incomplete = FixedStates::holding(consumed_cap());
    assert!(matches!(
        proof(Some(&later), Some(&incomplete)),
        Err(Unproved::Result)
    ));
    let unavailable = FixedStates::new(CadIr::empty(), |_| StateModel::Unavailable);
    assert!(matches!(
        proof(Some(&later), Some(&unavailable)),
        Err(Unproved::Result)
    ));
}

#[test]
fn a_state_view_over_its_work_cap_keeps_the_loft_unproved_as_bounded() {
    let later = consumed_cap();
    let bounded = FixedStates::new(CadIr::empty(), |_| StateModel::Bounded);
    assert!(matches!(
        proof(Some(&later), Some(&bounded)),
        Err(Unproved::Bounded)
    ));
    // A final model that proves the loft never reads the state.
    assert!(proof(Some(&legacy_model()), Some(&bounded)).is_ok());
}

#[test]
fn a_failed_state_is_its_own_refusal() {
    let later = consumed_cap();
    let failed = FixedStates::new(CadIr::empty(), |_| StateModel::Failed);
    assert!(matches!(
        proof(Some(&later), Some(&failed)),
        Err(Unproved::StateFailed)
    ));
}

fn final_proof(model: &CadIr, bound: bool) -> bool {
    loft_final_proof(model, 17, [19, 23], legacy_curves, bound).is_some()
}

#[test]
fn a_bound_state_proves_a_loft_whose_cap_a_later_feature_consumed_or_copied() {
    for later in [consumed_cap(), copied_cap()] {
        assert!(loft_result(&later, 17, [19, 23], legacy_curves).is_none());
        assert!(final_proof(&later, true));
    }
    // A complete final model needs no state.
    assert!(final_proof(&legacy_model(), false));
}

#[test]
fn a_loft_without_a_bound_state_needs_the_final_model_to_prove_it() {
    for later in [consumed_cap(), copied_cap()] {
        assert!(!final_proof(&later, false));
    }
}

#[test]
fn a_bound_state_does_not_admit_an_owned_face_that_contradicts_the_loft() {
    let check = |edit: &dyn Fn(&mut CadIr)| {
        let mut model = consumed_cap();
        edit(&mut model);
        assert!(!final_proof(&model, true));
    };
    // A cap of a section that the loft does not use.
    check(&|model| model.model.attributes[0].values[10] = Some(AttributeValue::Integer(40)));
    // A side face whose last section differs from the loft.
    check(&|model| model.model.attributes[1].values[16] = Some(AttributeValue::Integer(40)));
    // A side face whose curve is not on the section boundary.
    check(&|model| model.model.attributes[1].values[10] = Some(AttributeValue::Integer(99)));
    // A cap in a form that no loft writes.
    check(&|model| model.model.attributes[0].values[11] = Some(AttributeValue::Integer(261)));
    // A name of another family on a face of the generator.
    let sweep = self::model();
    assert!(loft_final_proof(&sweep, 17, [19, 23], legacy_curves, true).is_none());
}

#[test]
fn a_bound_state_admits_a_final_model_that_holds_no_face_of_the_loft() {
    // Generator 18 names no face: every face of the loft is gone.
    assert!(loft_final_proof(&legacy_model(), 18, [19, 23], legacy_curves, true).is_some());
    assert!(loft_final_proof(&legacy_model(), 18, [19, 23], legacy_curves, false).is_none());
    assert!(loft_final_proof(&legacy_model(), 17, [19, 19], legacy_curves, true).is_none());
}

#[test]
fn a_bound_state_admits_a_face_that_a_later_feature_re_surfaced() {
    // The projector proves the surface of each face on the state after the
    // loft. The final model can hold another surface under the same name.
    let cylinder = || self::model().geometry.surfaces[2].geometry.clone();
    let mut cap = consumed_cap();
    cap.geometry.surfaces[1].geometry = cylinder();
    assert!(final_proof(&cap, true));
    assert!(!final_proof(&cap, false));
    // The side face of the legacy form.
    let mut side = consumed_cap();
    side.geometry.surfaces[2].geometry = cylinder();
    assert!(final_proof(&side, true));
    // The caps and the side face of the schema-44 form, with the side face
    // on a surface that is no loft surface.
    let mut modern = self::model();
    for (i, attribute) in modern.model.attributes.iter_mut().enumerate() {
        attribute.values.truncate(11);
        let (family, values) = match i {
            0 => ("INV_NMX_GEN_TAG", vec![0, 19, 260]),
            1 => ("INV_NMX_GEN_TAG", vec![0, 23, 260]),
            _ => (
                "INV_NMX_ORDERED_GEN_TAG",
                vec![2, 8, 19, 258, 0, 0, 3, 23, 258, 4, 1],
            ),
        };
        attribute.values[5] = Some(AttributeValue::String(family.into()));
        attribute.values.extend(
            values
                .into_iter()
                .map(|value| Some(AttributeValue::Integer(value))),
        );
    }
    modern.geometry.surfaces[0].geometry = cylinder();
    assert!(loft_result(&modern, 17, [19, 23], legacy_curves).is_none());
    assert!(final_proof(&modern, true));
    // A name that no loft writes is still a contradiction.
    modern.model.attributes[2].values[21] = Some(AttributeValue::Integer(0));
    assert!(!final_proof(&modern, true));
}

#[test]
fn a_legacy_attribute_that_the_projector_refuses_refuses_the_screen() {
    // `legacy_owned_faces` refuses the whole loft for these forms, whichever
    // generator the attribute names, and so does the screen.
    let refused = |edit: &dyn Fn(&mut CadIr)| {
        let mut model = consumed_cap();
        edit(&mut model);
        assert!(legacy_loft(&model, 17, [19, 23], legacy_curves).is_none());
        assert!(!final_proof(&model, true));
    };
    // The attribute of the later generator, count 0.
    refused(&|model| model.model.attributes[2].values[6] = Some(AttributeValue::Integer(0)));
    // The attribute of the loft itself, count 0 and a negative count.
    refused(&|model| model.model.attributes[1].values[6] = Some(AttributeValue::Integer(0)));
    refused(&|model| model.model.attributes[1].values[6] = Some(AttributeValue::Integer(-1)));
    // A count that the pairs do not fit.
    refused(&|model| model.model.attributes[2].values[6] = Some(AttributeValue::Integer(40)));
    // A tail that holds a value other than an integer.
    refused(&|model| model.model.attributes[2].values[10] = None);
    // A record too short to hold the count.
    refused(&|model| model.model.attributes[2].values.truncate(6));
}

#[test]
fn a_naming_tag_of_a_real_carrier_reaches_a_typed_loft_through_the_state_index() {
    use crate::feature::history_states::{
        frame_chain, index_chain, prepare, Closed, Framing, Prepared, Reading,
    };
    use crate::feature::history_views::StateViews;
    use crate::kernel::parse_carrier;
    use crate::test_support::history_carrier::{
        framed_carrier, lofted_carrier_bytes, LATER_GENERATOR, LOFT_CURVES, LOFT_GENERATOR,
        LOFT_SECTIONS,
    };
    use cadmpeg_asm::kernel_header::RefWidth;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    let payload = framed_carrier(&lofted_carrier_bytes(RefWidth::Eight));
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(&payload, &arena, &DecodePolicy::default())
        .expect("carrier fits policy");
    let carrier =
        parse_carrier(view, &cadmpeg_ir::identity_key!("token"), 1, 0, 18).expect("carrier parses");
    let header = carrier.header.as_ref().expect("header parses");
    let window = carrier.bytes.window();
    let Framing::Framed(framed) = frame_chain(&ctx, window, header.width).expect("frame") else {
        panic!("the chain frames");
    };
    let Prepared::Ready(chain) = prepare(&ctx, &framed, window, header.width).expect("prepare")
    else {
        panic!("the chain closes");
    };
    let Reading::Read(index) = index_chain(&chain, carrier.schema, 1 << 20) else {
        panic!("the chain reads");
    };
    // The index comes from the tags of the stream: the older state holds the
    // tags of the loft, and only the head state holds the later tag.
    let loft_source = u32::try_from(LOFT_GENERATOR).expect("source");
    let later_source = u32::try_from(LATER_GENERATOR).expect("source");
    assert_eq!(index.first.get(&loft_source), Some(&0));
    assert_eq!(index.first.get(&later_source), Some(&1));
    let closed = Closed::new(*chain, index);
    let views = StateViews::new(&carrier, &closed, 1 << 20);
    let curves = |section: usize| vec![LOFT_CURVES[section]];
    // The head state is the final model of the part; the loft is proved from
    // the state after it alone.
    assert!(loft_proof(&ctx, None, Some(&views), loft_source, LOFT_SECTIONS, curves).is_ok());
    // The tag of the later feature names one cap only, and no state names a
    // source that no tag lists.
    for source in [later_source, 99] {
        assert!(matches!(
            loft_proof(&ctx, None, Some(&views), source, LOFT_SECTIONS, curves),
            Err(Unproved::Result)
        ));
    }
    // The sections must be the ones that the tags name.
    assert!(matches!(
        loft_proof(&ctx, None, Some(&views), loft_source, [23, 19], curves),
        Err(Unproved::Result)
    ));
}
