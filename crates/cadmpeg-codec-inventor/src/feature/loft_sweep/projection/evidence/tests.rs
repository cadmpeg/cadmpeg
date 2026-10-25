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
    assert!(sweep(&model, 17, 2.0, 8, &[9], ends).is_some());
    assert!(sweep(&model, 18, 2.0, 8, &[9], ends).is_none());
    assert!(sweep(&model, 17, 3.0, 8, &[9], ends).is_none());
    assert!(sweep(&model, 17, 2.0, 7, &[9], ends).is_none());
    assert!(sweep(&model, 17, 2.0, 8, &[9, 10], ends).is_none());
    assert!(sweep(
        &model,
        17,
        2.0,
        8,
        &[9],
        [ends[0], Point3::new(0.0, 0.0, 4.0)]
    )
    .is_none());
    assert!(loft(&model, 17, [19, 23]).is_none());
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
    assert!(sweep(&shifted, 17, 2.0, 8, &[9], ends).is_none());

    let mut missing = model.clone();
    missing.attributes.pop();
    assert!(sweep(&missing, 17, 2.0, 8, &[9], ends).is_none());
    let mut duplicate = model;
    duplicate.attributes.push(duplicate.attributes[0].clone());
    assert!(sweep(&duplicate, 17, 2.0, 8, &[9], ends).is_none());
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
    assert!(loft(&model, 17, [19, 23]).is_some());
    assert!(loft(&model, 17, [23, 19]).is_none());
    assert!(loft(&model, 17, [19, 24]).is_none());
    model.attributes[2].values[21] = AttributeValue::Integer(2);
    assert!(loft(&model, 17, [19, 23]).is_none());
}
