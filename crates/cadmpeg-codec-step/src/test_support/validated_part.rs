// SPDX-License-Identifier: Apache-2.0
//! Part records with axis-aligned box bodies and CAx-IF geometric
//! validation properties for declared-measure tests.

use std::fmt::Write as _;

use super::box_brep::{box_shell, BoxFaceUse};

/// One body of a part: a solid box, or a box without its face at
/// `z = max.z` as an open shell of a shell-based surface model.
#[derive(Clone, Copy)]
pub(crate) enum PartBody {
    Solid { min: [f64; 3], max: [f64; 3] },
    Sheet { min: [f64; 3], max: [f64; 3] },
}

/// The records of one part and the instance numbers that validation
/// properties name.
pub(crate) struct Part {
    pub(crate) records: String,
    /// `PRODUCT_DEFINITION_SHAPE` of the part.
    pub(crate) shape: u64,
    /// The length unit of the model context.
    pub(crate) length_unit: u64,
    /// The model context.
    pub(crate) context: u64,
    /// The shape representation that lists the bodies.
    pub(crate) representation: u64,
    /// The `MANIFOLD_SOLID_BREP` or `SHELL_BASED_SURFACE_MODEL` of each body.
    pub(crate) items: Vec<u64>,
}

/// A part `#1..#8` whose shape representation lists `bodies`, numbered from
/// 100.
pub(crate) fn part(bodies: &[PartBody]) -> Part {
    let mut records = String::new();
    let mut next = 100;
    let mut items = Vec::new();
    for body in bodies {
        let (min, max, sheet) = match *body {
            PartBody::Solid { min, max } => (min, max, false),
            PartBody::Sheet { min, max } => (min, max, true),
        };
        let shell = box_shell(next, min, max, BoxFaceUse::Outward);
        let item = shell.next;
        if sheet {
            let closed = format!("#{}=CLOSED_SHELL('',(", shell.shell);
            for line in shell.records.lines() {
                match line.strip_prefix(&closed) {
                    Some(faces) => {
                        let faces: Vec<&str> = faces.trim_end_matches("));").split(',').collect();
                        let open = &faces[..faces.len() - 1];
                        writeln!(
                            records,
                            "#{}=OPEN_SHELL('',({}));",
                            shell.shell,
                            open.join(",")
                        )
                    }
                    None => writeln!(records, "{line}"),
                }
                .expect("write to a String");
            }
            writeln!(
                records,
                "#{item}=SHELL_BASED_SURFACE_MODEL('',(#{}));",
                shell.shell
            )
            .expect("write to a String");
        } else {
            records.push_str(&shell.records);
            writeln!(records, "#{item}=MANIFOLD_SOLID_BREP('',#{});", shell.shell)
                .expect("write to a String");
        }
        items.push(item);
        next = item + 1;
    }
    let length_unit = next;
    let context = next + 2;
    let representation = next + 3;
    let listed: Vec<String> = items.iter().map(|item| format!("#{item}")).collect();
    write!(
        records,
        "#{length_unit}=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #{angle}=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #{context}=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length_unit},#{angle})) REPRESENTATION_CONTEXT('model','3D'));\n\
         #{representation}=SHAPE_REPRESENTATION('',({listed}),#{context});\n\
         #1=APPLICATION_CONTEXT('mechanical design');\n\
         #2=PRODUCT_CONTEXT('',#1,'mechanical');\n\
         #3=PRODUCT('P','Part','',(#2));\n\
         #4=PRODUCT_DEFINITION_FORMATION('','',#3);\n\
         #5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');\n\
         #6=PRODUCT_DEFINITION('design','',#4,#5);\n\
         #7=PRODUCT_DEFINITION_SHAPE('','',#6);\n\
         #8=SHAPE_DEFINITION_REPRESENTATION(#7,#{representation});\n",
        angle = next + 1,
        listed = listed.join(","),
    )
    .expect("write to a String");
    Part {
        records,
        shape: 7,
        length_unit,
        context,
        representation,
        items,
    }
}

/// One validation item of a combined validation representation.
#[derive(Clone, Copy)]
pub(crate) enum Item {
    /// A `MEASURE_REPRESENTATION_ITEM` with an `AREA_MEASURE`.
    Area(&'static str, f64),
    /// A `MEASURE_REPRESENTATION_ITEM` with a `VOLUME_MEASURE`.
    Volume(&'static str, f64),
    /// A `MEASURE_REPRESENTATION_ITEM` with a `LENGTH_MEASURE`.
    Length(&'static str, f64),
    /// A `CARTESIAN_POINT`.
    Point(&'static str, [f64; 3]),
}

/// The records of one `'geometric validation property'` on `definition`,
/// numbered from `first`, whose combined representation holds `items`
/// from `first + 7` on.
pub(crate) fn validation_property(
    part: &Part,
    first: u64,
    definition: u64,
    items: &[Item],
) -> String {
    let property = first;
    let representation = first + 1;
    let area_unit = first + 3;
    let area_element = first + 4;
    let volume_unit = first + 5;
    let volume_element = first + 6;
    let length_unit = part.length_unit;
    let mut records = format!(
        "#{property}=PROPERTY_DEFINITION('geometric validation property','validated',#{definition});\n\
         #{relation}=PROPERTY_DEFINITION_REPRESENTATION(#{property},#{representation});\n\
         #{area_unit}=DERIVED_UNIT((#{area_element}));\n\
         #{area_element}=DERIVED_UNIT_ELEMENT(#{length_unit},2.);\n\
         #{volume_unit}=DERIVED_UNIT((#{volume_element}));\n\
         #{volume_element}=DERIVED_UNIT_ELEMENT(#{length_unit},3.);\n",
        relation = first + 2,
    );
    let mut listed = Vec::new();
    for (next, item) in (first + 7..).zip(items) {
        let body = match *item {
            Item::Area(name, value) => {
                format!(
                    "MEASURE_REPRESENTATION_ITEM('{name}',AREA_MEASURE({value:?}),#{area_unit})"
                )
            }
            Item::Volume(name, value) => format!(
                "MEASURE_REPRESENTATION_ITEM('{name}',VOLUME_MEASURE({value:?}),#{volume_unit})"
            ),
            Item::Length(name, value) => format!(
                "MEASURE_REPRESENTATION_ITEM('{name}',LENGTH_MEASURE({value:?}),#{length_unit})"
            ),
            Item::Point(name, [x, y, z]) => {
                format!("CARTESIAN_POINT('{name}',({x:?},{y:?},{z:?}))")
            }
        };
        writeln!(records, "#{next}={body};").expect("write to a String");
        listed.push(format!("#{next}"));
    }
    writeln!(
        records,
        "#{representation}=REPRESENTATION('',({}),#{});",
        listed.join(","),
        part.context
    )
    .expect("write to a String");
    records
}
