// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::json;

fn model() -> Model {
    let mut value = serde_json::to_value(Model::default()).expect("empty model");
    let partial = json!({
        "bodies": [{"id":"test:model:body#b", "kind":"solid", "regions":["test:model:region#r"]}],
        "regions": [{"id":"test:model:region#r", "body":"test:model:body#b", "shells":["test:model:shell#s"]}],
        "shells": [{"id":"test:model:shell#s", "region":"test:model:region#r", "members":[{"kind":"face","id":"test:model:face#f0"},{"kind":"face","id":"test:model:face#f1"},{"kind":"face","id":"test:model:face#f2"}]}],
        "faces": (0..3).map(|i| json!({"id":format!("test:model:face#f{i}"),"shell":"test:model:shell#s", "surface":format!("test:model:surface#g{i}"), "sense":"forward", "loops":{"classification":"unspecified","loops":if i<2 {vec![format!("test:model:loop#l{i}")]} else {vec![]}}})).collect::<Vec<_>>(),
        "loops": (0..2).map(|i|json!({"id":format!("test:model:loop#l{i}"), "face":format!("test:model:face#f{i}"), "boundary":{"kind":"ring","coedges":[format!("test:model:coedge#c{i}")]}})).collect::<Vec<_>>(),
        "coedges": (0..2).map(|i|json!({"id":format!("test:model:coedge#c{i}"),"owner_loop":format!("test:model:loop#l{i}"), "edge":format!("test:model:edge#e{i}"), "radial_next":format!("test:model:coedge#c{i}"), "sense":"forward"})).collect::<Vec<_>>(),
        "edges": (0..2).map(|i|json!({"id":format!("test:model:edge#e{i}"), "carrier":{"curve":format!("test:model:curve#c{i}"),"param_range":[0.0,std::f64::consts::TAU]}, "start":format!("test:model:vertex#v{i}"), "end":format!("test:model:vertex#v{i}")})).collect::<Vec<_>>(),
        "curves": (0..2).map(|i|json!({"id":format!("test:model:curve#c{i}"), "geometry":{"kind":"circle", "center":{"x":0.0,"y":0.0,"z":f64::from(i)*5.0},"axis":{"x":0.0,"y":0.0,"z":1.0},"ref_direction":{"x":1.0,"y":0.0,"z":0.0},"radius":2.0}})).collect::<Vec<_>>(),
        "surfaces": [
            {"id":"test:model:surface#g0", "geometry":{"kind":"plane","origin":{"x":0.0,"y":0.0,"z":0.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}},
            {"id":"test:model:surface#g1", "geometry":{"kind":"plane","origin":{"x":0.0,"y":0.0,"z":5.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}}},
            {"id":"test:model:surface#g2", "geometry":{"kind":"cylinder","origin":{"x":0.0,"y":0.0,"z":0.0},"axis":{"x":0.0,"y":0.0,"z":1.0},"ref_direction":{"x":1.0,"y":0.0,"z":0.0},"radius":2.0}}
        ],
        "attributes": (0..3).map(|i| {
            let mut values = vec![json!({"kind":"reference","value":"x"});5];
            values.push(json!({"kind":"string","value":"INV_NMX_SWEEPGENERATED_TAG"}));
            values.extend([1,44,1,17,-1].into_iter().map(|value|json!({"kind":"integer","value":value})));
            values.extend(match i {0=>vec![0,0,4],1=>vec![0,0,12],_=>vec![8,9,20]}.into_iter().chain([0;7]).map(|value|json!({"kind":"integer","value":value})));
            json!({"id":format!("test:model:attribute#a{i}"),"name":"ATTRIB_CUSTOM-attrib","target":{"kind":"face","id":format!("test:model:face#f{i}")},"values":values})
        }).collect::<Vec<_>>()
    });
    value
        .as_object_mut()
        .expect("model object")
        .extend(partial.as_object().expect("partial model").clone());
    serde_json::from_value(value).expect("synthetic owned solid")
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
    shifted.curves[1].geometry =
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
    missing.attributes.pop();
    assert!(sweep(&missing, 17, 2.0, 8, &[(9, None)], ends).is_none());
    let mut duplicate = model;
    duplicate.attributes.push(duplicate.attributes[0].clone());
    assert!(sweep(&duplicate, 17, 2.0, 8, &[(9, None)], ends).is_none());
}

fn cylinder_model(radius: f64) -> Model {
    let mut model = model();
    model.surfaces[2].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
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

fn torus_model(center: Point3) -> Model {
    torus_model_with_minor(center, 2.0)
}

fn torus_model_with_minor(center: Point3, minor: f64) -> Model {
    let mut model = model();
    model.surfaces[2].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
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
        serde_json::from_value::<cadmpeg_ir::geometry::surface_payloads::LoftSurfacePayload>(value)
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
        "form":{"kind":"support", "type_code":1, "first_flag":false,
            "subdata":{"form":"type211", "dimensions":[1,1], "row":[0.0,1.0]}}
    });
    let entry = json!({"parameter":0.0, "profile":[member], "path":{"auxiliaries":[], "flag":-1}});
    json!({
        "sections":[{"entries":[entry,entry]}, {"entries":[]}],
        "parameters":{"kind":"ordered_ranges", "ranges":[[0.0,1.0],[0.0,1.0]]},
        "closures":[0,0],"singularities":[0,0],"mode":0,"bridge":[]
    })
}

#[test]
fn loft_caps_and_side_members_must_name_the_ordered_section_sketches() {
    use cadmpeg_ir::geometry::{
        nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis},
        ProceduralSurface, SurfaceGeometry,
    };
    let mut model = model();
    for (i, attribute) in model.attributes.iter_mut().enumerate() {
        attribute.values.truncate(11);
        let (family, values) = match i {
            0 => ("INV_NMX_GEN_TAG", vec![0, 19, 260]),
            1 => ("INV_NMX_GEN_TAG", vec![0, 23, 260]),
            _ => (
                "INV_NMX_ORDERED_GEN_TAG",
                vec![2, 8, 19, 258, 0, 0, 3, 23, 258, 4, 1],
            ),
        };
        attribute.values[5] = AttributeValue::String(family.into());
        attribute
            .values
            .extend(values.into_iter().map(AttributeValue::Integer));
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
    let payload = serde_json::from_value(loft_payload()).expect("payload");
    model.procedural_surfaces.push(ProceduralSurface::new(
        id.clone(),
        ProceduralSurfaceDefinition::Loft(payload),
        None,
    ));
    model.surfaces[2].geometry = SurfaceGeometry::Procedural {
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
    different_last_curve.attributes[2].values[17] = AttributeValue::Integer(5);
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
        for (slot, value) in swapped.attributes[2].values[11..].iter_mut().zip(values) {
            *slot = AttributeValue::Integer(value);
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
    wrong_curve.attributes[2].values[12] = AttributeValue::Integer(99);
    assert!(
        loft(&wrong_curve, 17, [19, 23], |section, name| [(0, 8), (1, 3)]
            .contains(&(section, name)))
        .is_none()
    );
    model.attributes[2].values[21] = AttributeValue::Integer(2);
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
fn legacy_model() -> Model {
    let mut model = model();
    model.surfaces[2].geometry = serde_json::from_value(json!({
        "kind":"plane","origin":{"x":0.0,"y":0.0,"z":0.0},
        "normal":{"x":1.0,"y":0.0,"z":0.0},"u_axis":{"x":0.0,"y":1.0,"z":0.0}
    }))
    .expect("plane");
    let mut later = model.faces[2].clone();
    later.id = serde_json::from_value(json!("test:model:face#f3")).expect("id");
    model.faces.push(later);
    model.attributes.push(model.attributes[2].clone());
    let face = |i: usize| {
        AttributeTarget::Face(
            serde_json::from_value(json!(format!("test:model:face#f{i}"))).expect("face id"),
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
    for (attribute, (class, target, generator, tail)) in model.attributes.iter_mut().zip(attributes)
    {
        attribute.name = class.into();
        attribute.target = face(target);
        attribute.values.truncate(5);
        attribute.values.extend(
            [38, 1, generator, -1]
                .into_iter()
                .chain(tail)
                .map(AttributeValue::Integer),
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
    let check = |model: &Model| legacy_loft(model, 17, [19, 23], legacy_curves);
    let mut missing_cap = base.clone();
    missing_cap.attributes.remove(1);
    assert!(check(&missing_cap).is_none());
    let mut duplicate_cap = base.clone();
    duplicate_cap.attributes[1].values[11] = AttributeValue::Integer(19);
    assert!(check(&duplicate_cap).is_none());
    let mut foreign_cap = base.clone();
    foreign_cap.attributes[1].values[11] = AttributeValue::Integer(40);
    assert!(check(&foreign_cap).is_none());
    // The side tail carries no member index; a member tail is the other carrier's form.
    let mut member = base.clone();
    member.attributes[2].values[18] = AttributeValue::Integer(4);
    member.attributes[2].values[19] = AttributeValue::Integer(1);
    assert!(check(&member).is_none());
    let mut curve_tail = base.clone();
    curve_tail.attributes[2].values[14] = AttributeValue::Integer(257);
    assert!(check(&curve_tail).is_none());
    let mut duplicate_side = base.clone();
    let mut side = duplicate_side.attributes[2].clone();
    side.target = AttributeTarget::Face(
        serde_json::from_value(json!("test:model:face#f3")).expect("face id"),
    );
    side.values[7] = AttributeValue::Integer(17);
    duplicate_side.attributes[3] = side;
    assert!(check(&duplicate_side).is_none());
    // Two names on one face: the face is not solely the generator's.
    let mut shared = base.clone();
    shared.attributes[2].values[6] = AttributeValue::Integer(2);
    shared.attributes[2]
        .values
        .insert(9, AttributeValue::Integer(5));
    shared.attributes[2]
        .values
        .insert(10, AttributeValue::Integer(-1));
    assert!(check(&shared).is_none());
    // A side face on a non-planar surface.
    let mut curved = base.clone();
    curved.surfaces[2].geometry = model().surfaces[2].geometry.clone();
    assert!(check(&curved).is_none());
    // The generator names a face on a second body.
    let mut other_body = base.clone();
    let mut body = other_body.bodies[0].clone();
    body.id = serde_json::from_value(json!("test:model:body#b2")).expect("id");
    let mut region = other_body.regions[0].clone();
    region.id = serde_json::from_value(json!("test:model:region#r2")).expect("id");
    region.body = body.id.clone();
    let mut shell = other_body.shells[0].clone();
    shell.id = serde_json::from_value(json!("test:model:shell#s2")).expect("id");
    shell.region = region.id.clone();
    other_body.faces[3].shell = shell.id.clone();
    other_body.attributes[3].values[7] = AttributeValue::Integer(17);
    other_body.attributes[3].values[11] = AttributeValue::Integer(0);
    other_body.bodies.push(body);
    other_body.regions.push(region);
    other_body.shells.push(shell);
    assert!(check(&other_body).is_none());
}
