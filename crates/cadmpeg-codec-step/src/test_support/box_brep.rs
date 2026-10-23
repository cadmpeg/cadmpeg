// SPDX-License-Identifier: Apache-2.0
//! Axis-aligned box B-rep records for STEP orientation tests.

use std::fmt::Write as _;

/// How a box writes the use of each of its faces in the base shell.
#[derive(Clone, Copy)]
pub(crate) enum BoxFaceUse {
    /// `ADVANCED_FACE` with `same_sense` and bound orientation true: the
    /// oriented normal points out of the box.
    Outward,
    /// `ADVANCED_FACE` with `same_sense` and bound orientation false: the
    /// oriented normal points into the box.
    InwardFlags,
    /// An outward `ADVANCED_FACE` used through `ORIENTED_FACE(.F.)`: the
    /// oriented normal points into the box.
    InwardOrientedFace,
}

/// The records of one box closed shell.
pub(crate) struct BoxShell {
    /// The Part 21 records, one per line.
    pub(crate) records: String,
    /// The `CLOSED_SHELL` instance number.
    pub(crate) shell: u64,
    /// The first instance number after the box records.
    pub(crate) next: u64,
}

/// Instance numbers allocated in record order.
struct Records {
    text: String,
    next: u64,
}

impl Records {
    fn push(&mut self, body: &str) -> u64 {
        let id = self.next;
        writeln!(self.text, "#{id}={body};").expect("write to a String");
        self.next += 1;
        id
    }
}

fn real(value: f64) -> String {
    format!("{value:?}")
}

fn triple(value: [f64; 3]) -> String {
    format!("({},{},{})", real(value[0]), real(value[1]), real(value[2]))
}

/// The corner whose coordinate along each axis is `max` when that axis's
/// bit of `index` is set and `min` otherwise.
fn corner(index: usize, min: [f64; 3], max: [f64; 3]) -> [f64; 3] {
    let mut point = min;
    for (axis, coordinate) in point.iter_mut().enumerate() {
        if index & (1 << axis) != 0 {
            *coordinate = max[axis];
        }
    }
    point
}

fn unit(axis: usize, sign: f64) -> [f64; 3] {
    let mut direction = [0.0; 3];
    direction[axis] = sign;
    direction
}

/// A closed shell that bounds the box `min..max`, numbered from `first`.
/// Each face is a plane whose axis is the outward normal and whose bound
/// is one edge loop counterclockwise about that normal. `face_use` selects
/// how the base shell uses each face.
pub(crate) fn box_shell(
    first: u64,
    min: [f64; 3],
    max: [f64; 3],
    face_use: BoxFaceUse,
) -> BoxShell {
    let mut records = Records {
        text: String::new(),
        next: first,
    };
    let mut points = [0; 8];
    let mut vertices = [0; 8];
    for index in 0..8 {
        points[index] = records.push(&format!(
            "CARTESIAN_POINT('',{})",
            triple(corner(index, min, max))
        ));
        vertices[index] = records.push(&format!("VERTEX_POINT('',#{})", points[index]));
    }
    let mut edges = std::collections::BTreeMap::new();
    for start in 0..8 {
        for axis in 0..3 {
            let bit = 1 << axis;
            if start & bit != 0 {
                continue;
            }
            let end = start | bit;
            let direction = records.push(&format!("DIRECTION('',{})", triple(unit(axis, 1.0))));
            let vector = records.push(&format!(
                "VECTOR('',#{direction},{})",
                real(max[axis] - min[axis])
            ));
            let line = records.push(&format!("LINE('',#{},#{vector})", points[start]));
            let edge = records.push(&format!(
                "EDGE_CURVE('',#{},#{},#{line},.T.)",
                vertices[start], vertices[end]
            ));
            edges.insert((start, end), edge);
        }
    }
    let (face_flag, bound_flag) = match face_use {
        BoxFaceUse::Outward | BoxFaceUse::InwardOrientedFace => (".T.", ".T."),
        BoxFaceUse::InwardFlags => (".F.", ".F."),
    };
    let mut faces = Vec::new();
    for axis in 0..3 {
        let (first_axis, second_axis) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in [0, 1] {
            let base = if side == 1 { 1 << axis } else { 0 };
            let at = |a: usize, b: usize| base | (a << first_axis) | (b << second_axis);
            let mut ring = [at(0, 0), at(1, 0), at(1, 1), at(0, 1)];
            if side == 0 {
                ring.reverse();
            }
            let mut uses = Vec::new();
            for (position, &start) in ring.iter().enumerate() {
                let end = ring[(position + 1) % ring.len()];
                let (edge, flag) = match edges.get(&(start, end)) {
                    Some(edge) => (*edge, ".T."),
                    None => (edges[&(end, start)], ".F."),
                };
                uses.push(format!(
                    "#{}",
                    records.push(&format!("ORIENTED_EDGE('',*,*,#{edge},{flag})"))
                ));
            }
            let edge_loop = records.push(&format!("EDGE_LOOP('',({}))", uses.join(",")));
            let bound = records.push(&format!("FACE_OUTER_BOUND('',#{edge_loop},{bound_flag})"));
            let sign = if side == 1 { 1.0 } else { -1.0 };
            let normal = records.push(&format!("DIRECTION('',{})", triple(unit(axis, sign))));
            let reference =
                records.push(&format!("DIRECTION('',{})", triple(unit(first_axis, 1.0))));
            let placement = records.push(&format!(
                "AXIS2_PLACEMENT_3D('',#{},#{normal},#{reference})",
                points[ring[0]]
            ));
            let plane = records.push(&format!("PLANE('',#{placement})"));
            let mut face = records.push(&format!(
                "ADVANCED_FACE('',(#{bound}),#{plane},{face_flag})"
            ));
            if matches!(face_use, BoxFaceUse::InwardOrientedFace) {
                face = records.push(&format!("ORIENTED_FACE('',*,#{face},.F.)"));
            }
            faces.push(format!("#{face}"));
        }
    }
    let shell = records.push(&format!("CLOSED_SHELL('',({}))", faces.join(",")));
    BoxShell {
        records: records.text,
        shell,
        next: records.next,
    }
}

/// The unit, context, and representation records that place `item` in a
/// millimetre model, numbered from `first`.
pub(crate) fn solid_representation(first: u64, item: u64) -> String {
    let unit = first;
    let angle = first + 1;
    let context = first + 2;
    let representation = first + 3;
    format!(
        "#{unit}=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));\n\
         #{angle}=(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.));\n\
         #{context}=(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{unit},#{angle})) REPRESENTATION_CONTEXT('model','3D'));\n\
         #{representation}=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{item}),#{context});\n"
    )
}
