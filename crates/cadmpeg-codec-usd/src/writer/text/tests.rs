// SPDX-License-Identifier: Apache-2.0

use super::{
    asset, cut_links, float, floats, identifier, point32, quoted, real, real32, Emitter, Names,
};

#[test]
fn reals_read_back_to_their_value() {
    for value in [
        0.0,
        -0.0,
        1.0,
        0.1,
        1.0 / 3.0,
        1e-300,
        6.02e23,
        -2.5e-7,
        123_456.789,
    ] {
        let text = real(value);
        assert_eq!(
            text.parse::<f64>().unwrap().to_bits(),
            value.to_bits(),
            "{text}"
        );
    }
    assert_eq!(real(f64::INFINITY), "inf");
    assert_eq!(real(f64::NEG_INFINITY), "-inf");
    assert_eq!(real(f64::NAN), "nan");
    assert_eq!(real32(f32::INFINITY), "inf");
    assert_eq!(float(f64::NEG_INFINITY), "-inf");
    assert_eq!(float(0.1), "0.1");
    assert_eq!(real32(0.1), "0.1");
    assert_eq!(real32(1e-7), "1e-7");
}

#[test]
fn strings_escape_quotes_backslashes_and_controls() {
    assert_eq!(quoted("a\"b\\c\nd\u{1}"), "\"a\\\"b\\\\c\\nd\\x01\"");
}

#[test]
fn asset_paths_choose_their_delimiter() {
    assert_eq!(asset("a.usd"), "@a.usd@");
    assert_eq!(asset("a@b.usd"), "@@@a@b.usd@@@");
}

#[test]
fn names_are_identifiers_and_unique() {
    assert_eq!(identifier("Part 1"), "Part_1");
    assert_eq!(identifier("1st"), "_1st");
    assert_eq!(identifier(""), "_");
    let mut names = Names::default();
    names.reserve("Body");
    assert_eq!(names.unique("Body"), "Body_1");
    assert_eq!(names.unique("Body"), "Body_2");
}

#[test]
fn long_arrays_wrap() {
    let mut emitter = Emitter::default();
    let items: Vec<String> = (0..10).map(|item| item.to_string()).collect();
    emitter.array("int[] a", &items, "");
    assert_eq!(
        emitter.finish().text,
        "int[] a = [\n    0, 1, 2, 3, 4, 5, 6, 7,\n    8, 9\n]\n"
    );
}

/// A named prim one of whose points is past the binary32 range is left out
/// with its child prims and its path recorded; its siblings stay. An
/// attribute whose value is past the binary32 range is left out and its
/// prim stays.
#[test]
fn a_prim_with_a_point_past_the_binary32_range_is_left_out() {
    let prim = |emitter: &mut Emitter, name: &str, line: &str| {
        emitter.name_next(name);
        emitter.line(&format!("def Xform {}", quoted(name)));
        emitter.open();
        emitter.line(line);
    };
    let mut emitter = Emitter::default();
    prim(&mut emitter, "A", "double a = 1");
    prim(
        &mut emitter,
        "B",
        &format!("point3f[] points = [{}]", point32(&[1e300, 0.0, 0.0])),
    );
    prim(&mut emitter, "Inner", "double i = 2");
    emitter.line(&format!("float j = {}", float(1e39)));
    emitter.close();
    emitter.close();
    prim(&mut emitter, "C", &format!("double c = {}", real(1.0)));
    emitter.line(&format!("float k = {} (", float(-1e39)));
    emitter.line("    doc = \"k\"");
    emitter.line(")");
    emitter.line(&format!("color3f l = {}", floats(&[1.0, 0.5, 0.25])));
    emitter.close();
    emitter.close();
    emitter.line(&format!("float d = {}", float(f64::MAX)));
    let finished = emitter.finish();
    assert_eq!(
        finished.text,
        "def Xform \"A\"\n{\n    double a = 1\n    def Xform \"C\"\n    {\n        double c = 1\n        color3f l = (1, 0.5, 0.25)\n    }\n}\n"
    );
    assert_eq!(finished.prims, ["/A/B"]);
    assert_eq!(finished.values, ["/A/C.k", "d"]);
}

/// A point that binary32 rounds is counted in its prim, and the count goes
/// with the prim when the prim is left out.
#[test]
fn rounded_points_are_counted_with_their_prim() {
    let mut emitter = Emitter::default();
    for (name, x) in [("Kept", 0.1), ("Gone", 1e300)] {
        emitter.name_next(name);
        emitter.line(&format!("def Mesh {}", quoted(name)));
        emitter.open();
        emitter.rounded_point();
        emitter.line(&format!("point3f[] points = [{}]", point32(&[x, 0.0, 0.0])));
        emitter.close();
    }
    let finished = emitter.finish();
    assert_eq!(finished.rounded, 1);
    assert_eq!(finished.prims, ["/Gone"]);
}

/// A link to a prim left out loses that target; a joint that names one is
/// left out, and so are the links that name the joint.
#[test]
fn links_to_a_prim_left_out_are_cut() {
    let text = "def Xform \"Rig\"\n{\n    def Mesh \"Base\"\n    {\n    }\n    \
                def PhysicsRevoluteJoint \"Hinge\"\n    {\n        \
                rel physics:body0 = </Rig/Base>\n        rel physics:body1 = </Rig/Arm>\n    }\n    \
                def Xform \"User\"\n    {\n        \
                rel a = [</Rig/Base>, </Rig/Arm/Child>]\n        \
                rel b = </Rig/Hinge> (\n            doc = \"b\"\n        )\n        \
                float c.connect = </Rig/Arm.outputs:x>\n        \
                string d = \"</Rig/Arm>\"\n    }\n}\n";
    let cut = cut_links(text, &["/Rig/Arm".to_owned()]);
    assert_eq!(
        cut.text,
        "def Xform \"Rig\"\n{\n    def Mesh \"Base\"\n    {\n    }\n    \
         def Xform \"User\"\n    {\n        rel a = [</Rig/Base>]\n        \
         string d = \"</Rig/Arm>\"\n    }\n}\n"
    );
    assert_eq!(cut.joints, ["/Rig/Hinge"]);
    assert_eq!(
        cut.links,
        [
            "/Rig/User.a </Rig/Arm/Child>",
            "/Rig/User.b </Rig/Hinge>",
            "/Rig/User.c.connect </Rig/Arm.outputs:x>",
        ]
    );
}
