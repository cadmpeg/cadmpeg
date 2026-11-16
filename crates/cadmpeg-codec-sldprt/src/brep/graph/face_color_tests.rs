// SPDX-License-Identifier: Apache-2.0

//! Face colors: the current version of each face selects at most one color.

use super::unique_face_colors;
use crate::brep::entity;
use cadmpeg_ir::topology::Color;

fn face_color(face_attr: u32, color_attr: u32, face_seq: u32, rgb: [f64; 3]) -> entity::FaceColor {
    entity::FaceColor {
        face_attr,
        color_attr,
        face_seq,
        stream_order: 0,
        color: Color::new(rgb[0], rgb[1], rgb[2], 1.0).expect("valid color"),
        offset: usize::try_from(face_attr).unwrap(),
        target: None,
    }
}

fn face_color_version(face_attr: u32, seq: u32, stream_order: usize) -> entity::FaceColorVersion {
    entity::FaceColorVersion {
        face_attr,
        seq,
        stream_order,
    }
}

#[test]
fn current_uncolored_face_version_removes_an_older_color() {
    let colors = vec![face_color(700, 900, 1, [0.25, 0.5, 0.75])];

    let (resolved, unresolved) = unique_face_colors(
        colors,
        vec![face_color_version(700, 1, 0), face_color_version(700, 2, 0)],
    );

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 0);
}

#[test]
fn later_stream_replaces_an_equal_sequence_face_color() {
    let old = face_color(700, 900, 2, [0.25, 0.5, 0.75]);
    let mut current = face_color(700, 901, 2, [0.75, 0.5, 0.25]);
    current.stream_order = 1;

    let (resolved, unresolved) = unique_face_colors(
        vec![old, current],
        vec![face_color_version(700, 2, 0), face_color_version(700, 2, 1)],
    );

    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].color_attr, 901);
    assert_eq!(unresolved, 0);
}

#[test]
fn conflicting_current_face_colors_remain_unresolved() {
    let colors = vec![
        face_color(700, 900, 2, [0.25, 0.5, 0.75]),
        face_color(700, 901, 2, [0.75, 0.5, 0.25]),
    ];

    let (resolved, unresolved) = unique_face_colors(
        colors,
        vec![face_color_version(700, 2, 0), face_color_version(700, 2, 0)],
    );

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 1);
}

#[test]
fn conflicting_reuse_of_one_color_identity_remains_unresolved() {
    let colors = vec![
        face_color(700, 900, 2, [0.25, 0.5, 0.75]),
        face_color(701, 900, 2, [0.75, 0.5, 0.25]),
    ];

    let (resolved, unresolved) = unique_face_colors(
        colors,
        vec![face_color_version(700, 2, 0), face_color_version(701, 2, 0)],
    );

    assert!(resolved.is_empty());
    assert_eq!(unresolved, 2);
}
