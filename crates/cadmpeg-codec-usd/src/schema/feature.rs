// SPDX-License-Identifier: Apache-2.0
//! The typed attributes of the `CadFeature` subtypes (spec §18.7): the
//! leaves of the CADIR feature definition that each subtype states as an
//! attribute instead of in `cad:feature:payload`.

use super::{boolean, integer, reals, token, tokens, vector, Issues};
use crate::stage::Prim;

/// The value of one definition leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leaf {
    /// A token of the CADIR wire form.
    Token,
    /// Free text, such as an identity.
    Text,
    /// A length, in millimetres in the IR and in stage units in a stage.
    Length,
    /// An angle, in radians in the IR and in degrees in a stage.
    Angle,
    /// A non-negative count.
    Count,
    Flag,
    /// A direction `{x, y, z}`.
    Vector,
    /// A position `{x, y, z}`, in millimetres in the IR.
    Point,
    /// A list of CADIR identities.
    Identities,
}

/// One typed leaf: its path in the definition, `*` for each element of an
/// array, and its value.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Field {
    pub(crate) path: &'static [&'static str],
    pub(crate) leaf: Leaf,
}

const fn field(path: &'static [&'static str], leaf: Leaf) -> Field {
    Field { path, leaf }
}

/// A `CadFeature` subtype: its prim type, the attribute namespace, and its
/// typed leaves.
pub(crate) struct Family {
    pub(crate) tag: &'static str,
    pub(crate) prim_type: &'static str,
    pub(crate) namespace: &'static str,
    pub(crate) fields: &'static [Field],
}

/// The leaves of one extrude side at `side` of `extent`.
macro_rules! extrude_side {
    ($side:literal) => {
        [
            field(&["extent", $side, "termination", "kind"], Leaf::Token),
            field(&["extent", $side, "termination", "length"], Leaf::Length),
            field(&["extent", $side, "draft"], Leaf::Angle),
        ]
    };
}

/// The leaves of one revolve termination at `name` of the extent.
macro_rules! revolve_termination {
    ($name:literal) => {
        [
            field(&["construction", "extent", $name, "kind"], Leaf::Token),
            field(&["construction", "extent", $name, "angle"], Leaf::Angle),
        ]
    };
}

const EXTRUDE_SIDE: [Field; 3] = extrude_side!("side");
const EXTRUDE_FIRST: [Field; 3] = extrude_side!("first");
const EXTRUDE_SECOND: [Field; 3] = extrude_side!("second");

const EXTRUDE: &[Field] = &[
    field(&["op"], Leaf::Token),
    field(&["direction", "kind"], Leaf::Token),
    field(&["direction", "vector"], Leaf::Vector),
    field(&["direction", "source", "kind"], Leaf::Token),
    field(&["start", "kind"], Leaf::Token),
    field(&["start", "offset"], Leaf::Length),
    field(&["extent", "kind"], Leaf::Token),
    EXTRUDE_SIDE[0],
    EXTRUDE_SIDE[1],
    EXTRUDE_SIDE[2],
    EXTRUDE_FIRST[0],
    EXTRUDE_FIRST[1],
    EXTRUDE_FIRST[2],
    EXTRUDE_SECOND[0],
    EXTRUDE_SECOND[1],
    EXTRUDE_SECOND[2],
    field(&["profile", "kind"], Leaf::Token),
    field(&["profile", "value"], Leaf::Text),
    field(&["solid"], Leaf::Flag),
    field(&["inner_wire_taper"], Leaf::Token),
    field(&["length_along_profile_normal"], Leaf::Flag),
    field(&["allow_multi_profile_faces"], Leaf::Flag),
];

const REVOLVE_TERMINATION: [Field; 2] = revolve_termination!("termination");
const REVOLVE_FIRST: [Field; 2] = revolve_termination!("first");
const REVOLVE_SECOND: [Field; 2] = revolve_termination!("second");

const REVOLVE: &[Field] = &[
    field(&["op"], Leaf::Token),
    field(&["construction", "state"], Leaf::Token),
    field(&["construction", "axis", "origin"], Leaf::Point),
    field(&["construction", "axis", "direction"], Leaf::Vector),
    field(&["construction", "extent", "kind"], Leaf::Token),
    REVOLVE_TERMINATION[0],
    REVOLVE_TERMINATION[1],
    REVOLVE_FIRST[0],
    REVOLVE_FIRST[1],
    REVOLVE_SECOND[0],
    REVOLVE_SECOND[1],
    field(&["construction", "profile", "kind"], Leaf::Token),
    field(&["construction", "profile", "value"], Leaf::Text),
    field(&["construction", "solid"], Leaf::Flag),
    field(&["construction", "allow_multi_profile_faces"], Leaf::Flag),
];

const FILLET: &[Field] = &[
    field(&["groups", "*", "radius", "kind"], Leaf::Token),
    field(&["groups", "*", "radius", "radius"], Leaf::Length),
    field(&["groups", "*", "radius", "chord_length"], Leaf::Length),
];

const CHAMFER: &[Field] = &[
    field(&["flip_direction"], Leaf::Flag),
    field(&["groups", "*", "spec", "kind"], Leaf::Token),
    field(&["groups", "*", "spec", "distance"], Leaf::Length),
    field(&["groups", "*", "spec", "first"], Leaf::Length),
    field(&["groups", "*", "spec", "second"], Leaf::Length),
    field(&["groups", "*", "spec", "angle"], Leaf::Angle),
];

const PATTERN: &[Field] = &[
    field(&["pattern", "kind"], Leaf::Token),
    field(&["pattern", "count"], Leaf::Count),
    field(&["pattern", "spacing"], Leaf::Length),
    field(&["pattern", "direction"], Leaf::Vector),
    field(&["pattern", "axis_origin"], Leaf::Point),
    field(&["pattern", "axis_dir"], Leaf::Vector),
    field(&["pattern", "angle"], Leaf::Angle),
];

const HOLE: &[Field] = &[
    field(&["profile", "kind"], Leaf::Token),
    field(&["profile", "value"], Leaf::Text),
    field(&["profile_filter"], Leaf::Token),
    field(&["direction"], Leaf::Vector),
    field(&["extent", "kind"], Leaf::Token),
    field(&["extent", "length"], Leaf::Length),
    field(&["bottom", "kind"], Leaf::Token),
    field(&["bottom", "included_angle"], Leaf::Angle),
    field(&["bottom", "depth_to_tip"], Leaf::Flag),
    field(&["shape", "diameter"], Leaf::Length),
    field(&["shape", "construction", "construction"], Leaf::Token),
    field(&["shape", "construction", "kind", "kind"], Leaf::Token),
    field(&["shape", "construction", "kind", "angle"], Leaf::Angle),
    field(&["shape", "construction", "kind", "diameter"], Leaf::Length),
    field(&["shape", "construction", "kind", "depth"], Leaf::Length),
    field(&["allow_multi_profile_faces"], Leaf::Flag),
];

const SHELL: &[Field] = &[
    field(&["thickness"], Leaf::Length),
    field(&["join"], Leaf::Token),
    field(&["mode"], Leaf::Token),
    field(&["outward"], Leaf::Flag),
    field(&["allow_self_intersections"], Leaf::Flag),
    field(&["resolve_intersections"], Leaf::Flag),
    field(&["removed_faces", "kind"], Leaf::Token),
    field(&["removed_faces", "value", "faces"], Leaf::Identities),
    field(&["removed_faces", "value", "native"], Leaf::Text),
];

const DRAFT: &[Field] = &[
    field(&["angle"], Leaf::Angle),
    field(&["outward"], Leaf::Flag),
    field(&["faces", "kind"], Leaf::Token),
    field(&["faces", "value", "faces"], Leaf::Identities),
    field(&["faces", "value", "native"], Leaf::Text),
    field(&["anchor", "kind"], Leaf::Token),
    field(&["anchor", "pull", "direction"], Leaf::Vector),
    field(&["anchor", "plane", "kind"], Leaf::Token),
    field(&["anchor", "plane", "value"], Leaf::Text),
];

const MIRROR: &[Field] = &[
    field(&["plane_normal"], Leaf::Vector),
    field(&["plane_origin"], Leaf::Point),
    field(&["source", "kind"], Leaf::Token),
    field(&["source", "value"], Leaf::Text),
];

const COMBINE: &[Field] = &[
    field(&["op"], Leaf::Token),
    field(&["operands", "target", "kind"], Leaf::Token),
    field(&["operands", "target", "value"], Leaf::Text),
    field(&["operands", "tools", "kind"], Leaf::Token),
    field(&["operands", "tools", "value"], Leaf::Text),
];

const PRIMITIVE: &[Field] = &[
    field(&["op"], Leaf::Token),
    field(&["solid", "kind"], Leaf::Token),
    field(&["solid", "length"], Leaf::Length),
    field(&["solid", "width"], Leaf::Length),
    field(&["solid", "height"], Leaf::Length),
    field(&["solid", "radius"], Leaf::Length),
    field(&["solid", "radius1"], Leaf::Length),
    field(&["solid", "radius2"], Leaf::Length),
    field(&["solid", "angle"], Leaf::Angle),
];

const DATUM_PLANE: &[Field] = &[
    field(&["frame", "origin"], Leaf::Point),
    field(&["frame", "normal"], Leaf::Vector),
    field(&["frame", "u_axis"], Leaf::Vector),
];

const DATUM_AXIS: &[Field] = &[
    field(&["origin"], Leaf::Point),
    field(&["direction"], Leaf::Vector),
];

const DATUM_POINT: &[Field] = &[field(&["position"], Leaf::Point)];

const DATUM_FRAME: &[Field] = &[
    field(&["frame", "origin"], Leaf::Point),
    field(&["frame", "x_axis"], Leaf::Vector),
    field(&["frame", "y_axis"], Leaf::Vector),
    field(&["frame", "z_axis"], Leaf::Vector),
];

/// The `CadFeature` subtypes (spec §18.7).
pub(crate) const FAMILIES: &[Family] = &[
    Family {
        tag: "extrude",
        prim_type: "CadExtrudeFeature",
        namespace: "cad:extrude",
        fields: EXTRUDE,
    },
    Family {
        tag: "revolve",
        prim_type: "CadRevolveFeature",
        namespace: "cad:revolve",
        fields: REVOLVE,
    },
    Family {
        tag: "fillet",
        prim_type: "CadFilletFeature",
        namespace: "cad:fillet",
        fields: FILLET,
    },
    Family {
        tag: "chamfer",
        prim_type: "CadChamferFeature",
        namespace: "cad:chamfer",
        fields: CHAMFER,
    },
    Family {
        tag: "pattern",
        prim_type: "CadPatternFeature",
        namespace: "cad:pattern",
        fields: PATTERN,
    },
    Family {
        tag: "hole",
        prim_type: "CadHoleFeature",
        namespace: "cad:hole",
        fields: HOLE,
    },
    Family {
        tag: "shell",
        prim_type: "CadShellFeature",
        namespace: "cad:shell",
        fields: SHELL,
    },
    Family {
        tag: "draft",
        prim_type: "CadDraftFeature",
        namespace: "cad:draft",
        fields: DRAFT,
    },
    Family {
        tag: "mirror_shape",
        prim_type: "CadMirrorFeature",
        namespace: "cad:mirror",
        fields: MIRROR,
    },
    Family {
        tag: "combine",
        prim_type: "CadCombineFeature",
        namespace: "cad:combine",
        fields: COMBINE,
    },
    Family {
        tag: "primitive",
        prim_type: "CadPrimitiveFeature",
        namespace: "cad:primitive",
        fields: PRIMITIVE,
    },
    Family {
        tag: "datum_plane",
        prim_type: "CadDatumPlaneFeature",
        namespace: "cad:datumPlane",
        fields: DATUM_PLANE,
    },
    Family {
        tag: "datum_axis",
        prim_type: "CadDatumAxisFeature",
        namespace: "cad:datumAxis",
        fields: DATUM_AXIS,
    },
    Family {
        tag: "datum_point",
        prim_type: "CadDatumPointFeature",
        namespace: "cad:datumPoint",
        fields: DATUM_POINT,
    },
    Family {
        tag: "datum_coordinate_system",
        prim_type: "CadDatumFrameFeature",
        namespace: "cad:datumFrame",
        fields: DATUM_FRAME,
    },
];

/// The family of feature definition tag `tag`.
pub(crate) fn family(tag: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|family| family.tag == tag)
}

/// The family whose prim type is `prim_type`.
pub(crate) fn family_of_type(prim_type: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|family| family.prim_type == prim_type)
}

/// The attribute of `field` in `family`: the namespace and the path, each
/// segment in camel case.
pub(crate) fn attribute(family: &Family, field: &Field) -> String {
    let mut name = family.namespace.to_owned();
    for segment in field.path.iter().filter(|segment| **segment != "*") {
        name.push(':');
        let mut upper = false;
        for character in segment.chars() {
            if character == '_' {
                upper = true;
            } else if upper {
                name.extend(character.to_uppercase());
                upper = false;
            } else {
                name.push(character);
            }
        }
    }
    name
}

/// Whether `field` lies under an array element.
pub(crate) fn in_array(field: &Field) -> bool {
    field.path.contains(&"*")
}

/// Sets the leaves of `family` that `prim` states into `definition`, with
/// lengths multiplied by `scale`.
pub(crate) fn restore(
    family: &Family,
    prim: &Prim<'_>,
    definition: &mut serde_json::Value,
    scale: f64,
    issues: &mut Issues,
) {
    for field in family.fields {
        let name = attribute(family, field);
        if in_array(field) {
            let values: Vec<Option<serde_json::Value>> = match field.leaf {
                Leaf::Token | Leaf::Text => tokens(prim, &name, issues)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|text| (!text.is_empty()).then(|| text.into()))
                    .collect(),
                Leaf::Length | Leaf::Angle => reals(prim, &name, 1, issues)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|value| {
                        (!value.is_nan())
                            .then(|| number(convert_in(field.leaf, value, scale)))
                            .flatten()
                    })
                    .collect(),
                Leaf::Count | Leaf::Flag | Leaf::Vector | Leaf::Point | Leaf::Identities => {
                    Vec::new()
                }
            };
            for (index, value) in values.into_iter().enumerate() {
                if let Some(value) = value {
                    insert(definition, field.path, Some(index), value);
                }
            }
            continue;
        }
        let value = match field.leaf {
            Leaf::Token | Leaf::Text => token(prim, &name, issues).map(Into::into),
            Leaf::Identities => tokens(prim, &name, issues).map(Into::into),
            Leaf::Length | Leaf::Angle => vector(prim, &name, 1, issues)
                .and_then(|values| values.first().copied())
                .and_then(|value| number(convert_in(field.leaf, value, scale))),
            Leaf::Count => integer(prim, &name, issues)
                .and_then(|value| u64::try_from(value).ok())
                .map(Into::into),
            Leaf::Flag => boolean(prim, &name, issues).map(Into::into),
            Leaf::Vector | Leaf::Point => vector(prim, &name, 3, issues).and_then(|values| {
                let [x, y, z] = <[f64; 3]>::try_from(values.as_slice()).ok()?;
                let unit = if field.leaf == Leaf::Point {
                    scale
                } else {
                    1.0
                };
                Some(serde_json::json!({
                    "x": number(x * unit)?,
                    "y": number(y * unit)?,
                    "z": number(z * unit)?,
                }))
            }),
        };
        if let Some(value) = value {
            insert(definition, field.path, None, value);
        }
    }
}

/// A stage value of `leaf` in IR units.
fn convert_in(leaf: Leaf, value: f64, scale: f64) -> f64 {
    match leaf {
        Leaf::Length | Leaf::Point => value * scale,
        Leaf::Angle => value.to_radians(),
        _ => value,
    }
}

fn number(value: f64) -> Option<serde_json::Value> {
    serde_json::Number::from_f64(value).map(serde_json::Value::Number)
}

/// Sets `value` at `path` of `definition`, creating the objects on the way;
/// `index` selects the element at `*`.
fn insert(
    definition: &mut serde_json::Value,
    path: &[&str],
    index: Option<usize>,
    value: serde_json::Value,
) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut node = definition;
    for segment in parents {
        node = if *segment == "*" {
            let Some(element) = index.and_then(|index| node.get_mut(index)) else {
                return;
            };
            element
        } else {
            let serde_json::Value::Object(map) = node else {
                return;
            };
            map.entry((*segment).to_owned())
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        };
    }
    if let serde_json::Value::Object(map) = node {
        map.insert((*last).to_owned(), value);
    }
}
