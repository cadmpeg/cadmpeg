// SPDX-License-Identifier: Apache-2.0
//! Synthetic models for geometry tests and the committed fixtures.

use super::hsf::{
    box_shells, face_tag, matrix, range_block, translation, BodySpec, Item, ModelSpec, NodeSpec,
    Stream, IDENTITY,
};
use super::solidworks::{SolidWorksModel, SwBody, SwItem, SwNode, SwState, SwSub};
use super::{assembly_package, assembly_package_of, part_package, SOLIDWORKS_VERSION};

/// Millimetres per metre, for expectations.
pub(crate) const MM: f32 = 1000.0;
/// Edge of the cube, metres.
pub(crate) const CUBE_EDGE: f32 = 0.01;
/// Extent of the box body, metres.
pub(crate) const BOX_SIZE: [f32; 3] = [0.02, 0.01, 0.005];
/// Translation of the box instance, metres.
pub(crate) const BOX_OFFSET: [f32; 3] = [0.05, 0.02, 0.01];
/// Translation of the second cube, metres.
pub(crate) const CUBE_OFFSET: [f32; 3] = [0.0, 0.03, 0.0];
/// Segment of the instance the display state hides.
pub(crate) const HIDDEN_SEGMENT: &str = "assempart4";

/// A part with one 10 mm cube made of six shells and a colour.
pub(crate) fn cube_part() -> ModelSpec {
    ModelSpec {
        document: "synthetic cube.sldprt".into(),
        bodies: vec![BodySpec {
            colour: Some([200, 100, 50]),
            subs: vec![box_shells([CUBE_EDGE; 3])],
        }],
        tree: vec![NodeSpec::Part {
            segment: "instance0".into(),
            name: "synthetic cube".into(),
            body: 0,
            matrix: Some(IDENTITY),
            user_name: false,
        }],
        state: None,
    }
}

/// The body of a box whose +X face is an instance of its -X face, turned half
/// a revolution about the Y axis through the middle of the box.
fn box_body() -> BodySpec {
    let mut items = box_shells(BOX_SIZE);
    if let Some(slot) = items.get_mut(5) {
        *slot = Item::Instance {
            source: 4,
            matrix: matrix(
                [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
                [BOX_SIZE[0], 0.0, BOX_SIZE[2]],
            ),
        };
    }
    BodySpec {
        colour: Some([0, 0, 255]),
        subs: vec![items],
    }
}

/// An assembly of two bodies and three instances. The box sits in a
/// sub-assembly inside a sub-assembly, rotated 90 degrees about Z, and the
/// second cube sits in the outer sub-assembly and is hidden by the active
/// display state.
pub(crate) fn assembly() -> ModelSpec {
    let cube = BodySpec {
        colour: Some([255, 0, 0]),
        subs: vec![box_shells([CUBE_EDGE; 3])],
    };
    let rotated = matrix(
        [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        BOX_OFFSET,
    );
    ModelSpec {
        document: "synthetic assembly.sldasm".into(),
        bodies: vec![cube, box_body()],
        tree: vec![
            NodeSpec::Part {
                segment: "assempart0".into(),
                name: "Cube-1".into(),
                body: 0,
                matrix: None,
                user_name: false,
            },
            NodeSpec::Sub {
                segment: "subassem1".into(),
                name: "Group A".into(),
                children: vec![
                    NodeSpec::Sub {
                        segment: "subassem2".into(),
                        name: "Group B".into(),
                        children: vec![NodeSpec::Part {
                            segment: "assempart3".into(),
                            name: "Box-1".into(),
                            body: 1,
                            matrix: Some(rotated),
                            user_name: true,
                        }],
                    },
                    NodeSpec::Part {
                        segment: HIDDEN_SEGMENT.into(),
                        name: "Cube-2".into(),
                        body: 0,
                        matrix: Some(translation(CUBE_OFFSET)),
                        user_name: false,
                    },
                ],
            },
        ],
        state: Some((
            "disp0".into(),
            vec![(
                format!("config0/sheet0/view0/3dmodel/includes/subassem1/{HIDDEN_SEGMENT}"),
                false,
            )],
        )),
    }
}

/// The bytes of the committed `synthetic_cube.eprt` fixture.
pub(crate) fn cube_fixture() -> Vec<u8> {
    part_package(&cube_part().stream())
}

/// The bytes of the committed `synthetic_assembly.easm` fixture.
pub(crate) fn assembly_fixture() -> Vec<u8> {
    assembly_package(&assembly().stream())
}

/// The default body colour of `SolidWorks`.
pub(crate) const SW_DEFAULT_COLOUR: [f32; 3] = [0.792_157, 0.819_608, 0.933_333];
/// Extent of the box body of the `SolidWorks` assembly, metres.
pub(crate) const SW_BOX_SIZE: [f32; 3] = [0.02, 0.01, 0.01];
/// Translation of the rotated box leaf, metres.
pub(crate) const SW_BOX_OFFSET: [f32; 3] = [0.05, 0.02, 0.01];
/// Segment of the leaf two sub-assemblies deep.
pub(crate) const SW_DEEP_SEGMENT: &str = "assempart4";
/// Segment of the leaf of the shared two-body set that the state lists.
pub(crate) const SW_LISTED_SEGMENT: &str = "assempart5";
/// Segment of the leaf of the shared two-body set that hides by default.
pub(crate) const SW_DEFAULT_HIDDEN_SEGMENT: &str = "assempart6";
/// Segment of the sub-assembly that the default state hides.
pub(crate) const SW_HIDDEN_SUB_SEGMENT: &str = "subassem7";
/// Segment of the leaf inside the hidden sub-assembly.
pub(crate) const SW_INHERITING_SEGMENT: &str = "assempart8";
/// Segment of the leaf that the default state hides.
pub(crate) const SW_HIDDEN_SEGMENT: &str = "assempart9";

fn sw_cube(colour: [f32; 3], sub_colour: Option<[f32; 3]>) -> SwBody {
    SwBody {
        colour: Some(colour),
        subs: vec![SwSub {
            items: box_shells([CUBE_EDGE; 3])
                .into_iter()
                .map(SwItem::Local)
                .collect(),
            colour: sub_colour,
        }],
    }
}

/// A `SolidWorks`-style assembly: three bodies, one of them (the box) holding a
/// shell that instances a shell of the first body; a part two sub-assemblies
/// deep; a two-body part whose body set a second part re-uses; a sub-assembly
/// and a part that the default display state hides; a part that hides by
/// default; and a part with a colour override.
pub(crate) fn solidworks_assembly() -> SolidWorksModel {
    let mut box_items: Vec<SwItem> = box_shells(SW_BOX_SIZE)
        .into_iter()
        .map(SwItem::Local)
        .collect();
    if let Some(slot) = box_items.get_mut(5) {
        *slot = SwItem::Foreign {
            body: 0,
            source: 4,
            matrix: matrix(
                [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
                [SW_BOX_SIZE[0], 0.0, SW_BOX_SIZE[2]],
            ),
        };
    }
    let rotated = matrix(
        [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        SW_BOX_OFFSET,
    );
    let leaf = |segment: &str, name: &str, bodies: &[usize], matrix: [f32; 12]| SwNode::Leaf {
        segment: segment.into(),
        name: name.into(),
        bodies: bodies.to_vec(),
        matrix,
        colour: None,
        hidden_by_default: false,
    };
    let overridden = SwNode::Leaf {
        segment: "assempart1".into(),
        name: "Cube-1".into(),
        bodies: vec![0],
        matrix: translation([0.1, 0.0, 0.0]),
        colour: Some([1.0, 0.0, 0.0]),
        hidden_by_default: false,
    };
    let default_hidden = SwNode::Leaf {
        segment: SW_DEFAULT_HIDDEN_SEGMENT.into(),
        name: "Pair-2".into(),
        bodies: vec![0, 2],
        matrix: translation([0.03, 0.03, 0.0]),
        colour: None,
        hidden_by_default: true,
    };
    SolidWorksModel {
        document: "synthetic solidworks assembly.sldasm".into(),
        bodies: vec![
            sw_cube(SW_DEFAULT_COLOUR, Some([1.0, 0.0, 0.0])),
            SwBody {
                colour: Some([0.0, 0.0, 1.0]),
                subs: vec![SwSub {
                    items: box_items,
                    colour: None,
                }],
            },
            sw_cube([0.0, 1.0, 0.0], None),
        ],
        tree: vec![
            overridden,
            SwNode::Sub {
                segment: "subassem2".into(),
                name: "Group A".into(),
                children: vec![
                    SwNode::Sub {
                        segment: "subassem3".into(),
                        name: "Group B".into(),
                        children: vec![leaf(SW_DEEP_SEGMENT, "Box-1", &[1], rotated)],
                    },
                    leaf(
                        SW_LISTED_SEGMENT,
                        "Pair-1",
                        &[0, 2],
                        translation([0.0, 0.03, 0.0]),
                    ),
                    default_hidden,
                ],
            },
            SwNode::Sub {
                segment: SW_HIDDEN_SUB_SEGMENT.into(),
                name: "Group C".into(),
                children: vec![leaf(
                    SW_INHERITING_SEGMENT,
                    "Cube-3",
                    &[2],
                    translation([0.0, 0.0, 0.05]),
                )],
            },
            leaf(
                SW_HIDDEN_SEGMENT,
                "Cube-4",
                &[2],
                translation([0.0, 0.0, 0.07]),
            ),
        ],
        states: vec![
            SwState {
                name: "disp0".into(),
                components: vec![
                    (SW_HIDDEN_SUB_SEGMENT.into(), false),
                    (SW_HIDDEN_SEGMENT.into(), false),
                    (format!("subassem2/{SW_LISTED_SEGMENT}"), true),
                ],
            },
            SwState {
                name: "disp1".into(),
                components: vec![("subassem2".into(), false)],
            },
            SwState {
                name: "disp2".into(),
                components: vec![("subassem2".into(), true)],
            },
        ],
        default_state: Some("disp0".into()),
    }
}

/// The bytes of the committed `synthetic_sw_assembly.easm` fixture.
pub(crate) fn solidworks_fixture() -> Vec<u8> {
    assembly_package_of(SOLIDWORKS_VERSION, &solidworks_assembly().stream())
}

/// A stream of the 2D annotation records: a wide option, a hole marker
/// segment with its weight, pattern, and colour chain, and a geometry side
/// object for each of its two slots, then a textured shell with a range
/// block and a face tag.
pub(crate) fn annotation_stream() -> Vec<u8> {
    let point = std::iter::once(b'X')
        .chain(std::iter::repeat_n(0x3f, 12))
        .collect::<Vec<u8>>();
    let line = std::iter::once(b'l')
        .chain(std::iter::repeat_n(0x3f, 24))
        .collect::<Vec<u8>>();
    let mut s = Stream::new();
    s.header(3)
        .camera("default")
        .wide_user_option("config name=`D\u{e9}faut`");
    s.open("2dmodel").open("hole markers").open("axis99");
    s.user_option("diameter=0.01")
        .eq_record()
        .line_pattern("edw-chain")
        .edge_pattern("edw-chain")
        .colour([191, 191, 191]);
    s.open("star").marker(9).close().slot().close().slot();
    s.close().close().separator();
    s.side_geometry(0, &[point.clone(), point].concat()).close();
    s.side_geometry(1, &line).close();
    let tail = [range_block(3, [0.0, 0.5]), face_tag("swfaceid=4", false)].concat();
    s.shell_with_tail(2, &[[0.0; 3]; 3], &[vec![0, 1, 2]], &tail);
    s.separator().trailer_and_end();
    s.into_bytes()
}
