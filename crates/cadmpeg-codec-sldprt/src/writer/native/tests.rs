// SPDX-License-Identifier: Apache-2.0
//! The skeleton's object streams read to their end with the decoder's exact
//! walker, and its `KeyWords` and class map take the native form.

use super::{ids, skeleton, Body, Facts, Numbering};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn streams(bodies: usize) -> Vec<(String, Vec<u8>)> {
    streams_of_kinds(&vec![false; bodies])
}

/// The skeleton of a part with one body per entry, a sheet body where the
/// entry is `true`.
fn streams_of_kinds(sheets: &[bool]) -> Vec<(String, Vec<u8>)> {
    let numbering = Numbering::new(None, sheets.len(), &[]).expect("the part numbers");
    let bodies = sheets
        .iter()
        .enumerate()
        .map(|(index, &sheet)| Body::numbered("Bracket", &numbering, index, sheet))
        .collect::<Result<Vec<_>, _>>()
        .expect("bodies number");
    skeleton(&Facts {
        numbering,
        name: "Bracket",
        active: "Default",
        features: &[],
        bodies: &bodies,
    })
    .expect("the skeleton writes")
}

fn stream<'a>(streams: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    streams
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map_or_else(
            || panic!("{name} is written"),
            |(_, payload)| payload.as_slice(),
        )
}

#[test]
fn object_streams_walk_to_their_end_with_a_reconciled_class_map() {
    for bodies in [0, 1, 3] {
        let streams = streams(bodies);
        let config = stream(&streams, "Contents/Config-0");
        let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures");
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(config, &arena, &DecodePolicy::desktop())
            .expect("context");
        let count = crate::archive::resolved_count(resolved).expect("map count");
        let map = crate::archive::class_index_map(config, count, &ctx)
            .expect("class map")
            .expect("the Config-0 map reaches the ResolvedFeatures count");
        assert_eq!(map.get(&2).map(String::as_str), Some("moPart_c"));
        assert_eq!(map.get(&4).map(String::as_str), Some("moNodeName_c"));
        let version = crate::carchive::walk_version(None, config);
        assert_eq!(version, Some(super::VERSION));
        let walk = crate::carchive::walk_configuration(&ctx, config, resolved, version)
            .expect("walk")
            .expect("the walk runs");
        assert_eq!(walk.status, crate::carchive::Status::End, "{bodies} bodies");
        let marks = walk.marks.iter().filter_map(|mark| mark.owner.as_deref());
        let owners = marks.collect::<Vec<_>>();
        let expected = (1..=bodies)
            .map(|index| format!("Imported{index}"))
            .collect::<Vec<_>>();
        assert_eq!(owners, expected);
    }
}

#[test]
fn the_part_object_states_its_section_lengths() {
    let streams = streams(2);
    let config = stream(&streams, "Contents/Config-0");
    let word = |at: usize| {
        let bytes = config[at..at + 4].try_into().expect("four bytes");
        usize::try_from(u32::from_le_bytes(bytes)).expect("a length fits usize")
    };
    let first = 14;
    let second = first + 4 + word(first);
    assert_eq!(word(second), config.len() - second - 4);
    assert!(config[second - 20..second].iter().all(|&byte| byte == 0));
}

#[test]
fn keywords_group_records_by_tag_and_order_them_by_identifier_text() {
    let streams = streams(2);
    let keywords = stream(&streams, "swXmlContents/KeyWords");
    assert_eq!(keywords[0], 0x86);
    let text = std::str::from_utf8(&keywords[1..]).expect("UTF-8");
    let document = roxmltree::Document::parse(text).expect("XML");
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "Keywords");
    let records = root
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|node| {
            (
                node.tag_name().name().to_owned(),
                node.attribute("id").unwrap_or_default().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let mut sorted = records.clone();
    sorted.sort();
    assert_eq!(records, sorted);
    assert_eq!(records[0].0, "Configuration");
    let names = root
        .children()
        .filter_map(|node| node.attribute("Name"))
        .collect::<Vec<_>>();
    for name in [
        "Front Plane",
        "Top Plane",
        "Right Plane",
        "Origin",
        "Imported1",
        "Imported2",
    ] {
        assert!(names.contains(&name), "{name}");
    }
}

/// The `id` of each `KeyWords` record, by its name.
fn keyword_ids(streams: &[(String, Vec<u8>)]) -> std::collections::BTreeMap<String, u32> {
    let keywords = stream(streams, "swXmlContents/KeyWords");
    let text = std::str::from_utf8(&keywords[1..]).expect("UTF-8");
    let document = roxmltree::Document::parse(text).expect("XML");
    document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() != "Configuration")
        .map(|node| {
            let id = node.attribute("id").and_then(|id| id.parse().ok());
            (
                node.attribute("Name").unwrap_or_default().to_owned(),
                id.expect("a numeric identifier"),
            )
        })
        .collect()
}

#[test]
fn the_default_tree_takes_the_native_identifiers_and_user_objects_follow_it() {
    let (streams, walk) = extruded_top_sketch_streams();
    let ids = keyword_ids(&streams);
    let expected = [
        ("Annotations", 1),
        ("Front Plane", 2),
        ("Top Plane", 3),
        ("Right Plane", 4),
        ("Origin", 5),
        ("Lights, Cameras and Scene", 6),
        ("Design Binder", 7),
        ("Comments", 8),
        ("Solid Bodies", 9),
        ("Surface Bodies", 10),
        (super::MATERIAL_NAME, 11),
        ("Ambient", 12),
        ("Directional1", 13),
        ("Directional2", 14),
        ("Equations", 16),
        ("", 17),
        ("Sensors", 19),
        ("Favorites", 20),
        ("History", 21),
        ("Selection Sets", 22),
        ("Sketch1", 23),
        ("Boss-Extrude1", 24),
        ("Notes", 28),
        ("Notes1___EndTag___", 29),
        ("Markups", 30),
    ]
    .map(|(name, id)| (name.to_owned(), id));
    assert_eq!(ids, std::collections::BTreeMap::from(expected));
    // The sketch's two dimensions and the extrusion's depth take the three
    // identifiers between the user features and the notes folders; each
    // writes its identifier eleven bytes after its value.
    let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures");
    let parameters = walk
        .parameters
        .iter()
        .map(|parameter| u32_at(resolved, parameter.value_at + 11))
        .collect::<Vec<_>>();
    assert_eq!(parameters, [25, 26, 27]);
}

#[test]
fn stored_bodies_follow_the_history_summary_and_precede_the_last_folders() {
    let numbering = Numbering::new(Some(40), 2, &[]).expect("the part numbers");
    assert_eq!(
        numbering,
        Numbering {
            first_body: 41,
            first_parameter: 43,
            notes: 43,
            notes_end: 44,
            markups: 45,
        }
    );
    let bodies = [0, 1].map(|index| {
        Body::numbered("Bracket", &numbering, index, false)
            .expect("body numbers")
            .feature_id
    });
    assert_eq!(bodies, [41, 42]);
    let empty = Numbering::new(None, 0, &[]).expect("the part numbers");
    assert_eq!(empty.first_body, ids::FIRST_USER);
    assert_eq!(empty.markups, ids::FIRST_USER + 2);
    assert!(Numbering::new(Some(u32::MAX - 2), 1, &[]).is_err());
}

/// A sketch on the Top plane: a line, an arc from its end and a circle.
fn top_sketch() -> super::sketch::Sketch {
    use super::sketch::{Curve, Dimension, DimensionClass, Operand, Point, Relation, Sketch};
    let point = |at: [f64; 2], kind: u32| Point { at, kind };
    Sketch {
        name: "Sketch1".into(),
        feature_id: 23,
        plane: super::tree::Plane::Top,
        points: vec![
            point([0.0, 0.0], 1),
            point([0.01, 0.0], 1),
            point([0.01, 0.01], 2),
            point([0.0, 0.01], 2),
            point([0.03, 0.0], 1),
            point([0.02, 0.0], 1),
        ],
        curves: vec![
            Curve::Line { start: 0, end: 1 },
            Curve::Arc {
                start: 1,
                end: 3,
                centre: 0,
            },
            Curve::Circle { on: 4, centre: 5 },
        ],
        relations: vec![
            Relation {
                code: 4,
                operands: vec![Operand::Line(0)],
            },
            Relation {
                code: 6,
                operands: vec![Operand::Line(0), Operand::Arc(0)],
            },
        ],
        dimensions: vec![
            Dimension {
                class: DimensionClass::Circle,
                name: "d1".into(),
                value: 0.01,
                code: 15,
                operands: vec![Operand::Arc(1)],
            },
            Dimension {
                class: DimensionClass::PointPoint,
                name: "d2".into(),
                value: 0.01,
                code: 1,
                operands: vec![Operand::Point(0), Operand::Point(1)],
            },
        ],
        chains: vec![vec![0, 1], vec![2]],
        body: b"PS\0\0 profile body".repeat(8),
    }
}

#[test]
fn a_user_sketch_walks_to_the_end_and_holds_its_profile_body() {
    let features = [super::Feature::Sketch(top_sketch())];
    let numbering = Numbering::new(Some(23), 1, &features).expect("the part numbers");
    let bodies = [Body::numbered("Bracket", &numbering, 0, false).expect("body numbers")];
    let streams = skeleton(&Facts {
        numbering,
        name: "Bracket",
        active: "Default",
        features: &features,
        bodies: &bodies,
    })
    .expect("the skeleton writes");
    let config = stream(&streams, "Contents/Config-0");
    let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(resolved, &arena, &DecodePolicy::desktop())
        .expect("context");
    let walk = crate::carchive::walk_configuration(&ctx, config, resolved, Some(super::VERSION))
        .expect("walk")
        .expect("the walk runs");
    assert_eq!(walk.status, crate::carchive::Status::End);
    let profiles = walk
        .trace
        .iter()
        .filter(|object| &*object.class == "moProfileFeature_c")
        .count();
    assert_eq!(profiles, 1);
    let chains = walk
        .trace
        .iter()
        .filter(|object| &*object.class == "moSketchChain_c")
        .count();
    assert_eq!(chains, 2, "one chain per profile chain");
    assert_eq!(walk.sketch_profiles.len(), 1);
    let profile = walk.sketch_profiles[0];
    let wrapped = &resolved[profile.stream + 4..];
    assert_eq!(
        &wrapped[..16],
        &crate::layout::zlb_wrapper_header::MAGIC_VALUE
    );
    let marks = walk.marks.iter().filter_map(|mark| mark.owner.as_deref());
    assert_eq!(marks.collect::<Vec<_>>(), ["Imported1"]);
}

/// The streams of a part holding `top_sketch` and a 5 mm blind extrusion of
/// it, numbered as the history summary numbers them, and the decoder's exact
/// walk of its `ResolvedFeatures` stream.
fn extruded_top_sketch_streams() -> (Vec<(String, Vec<u8>)>, crate::carchive::Walk) {
    let features = [
        super::Feature::Sketch(top_sketch()),
        super::Feature::Extrusion(super::extrude::Extrusion {
            name: "Boss-Extrude1".into(),
            feature_id: 24,
            sketch_id: 23,
            depth: 0.005,
            reversed: false,
        }),
    ];
    let streams = skeleton(&Facts {
        numbering: Numbering::new(Some(24), 0, &features).expect("the part numbers"),
        name: "Bracket",
        active: "Default",
        features: &features,
        bodies: &[],
    })
    .expect("the skeleton writes");
    let config = stream(&streams, "Contents/Config-0");
    let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(resolved, &arena, &DecodePolicy::desktop())
        .expect("context");
    let walk = crate::carchive::walk_configuration(&ctx, config, resolved, Some(super::VERSION))
        .expect("walk")
        .expect("the walk runs");
    assert_eq!(walk.status, crate::carchive::Status::End);
    (streams, walk)
}

/// The `ResolvedFeatures` stream of [`extruded_top_sketch_streams`] and the
/// walk of it.
fn extruded_top_sketch() -> (Vec<u8>, crate::carchive::Walk) {
    let (streams, walk) = extruded_top_sketch_streams();
    let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures").to_vec();
    (resolved, walk)
}

#[test]
fn an_extrusion_writes_the_extrusion_family_word() {
    let (resolved, _) = extruded_top_sketch();
    // The name record is followed by four zero bytes and the family word,
    // `0x0140` for a `moExtrusion_c` (sldprt.md §2).
    let name_end = last_cstring_end(&resolved, "Boss-Extrude1");
    assert_eq!(u32_at(&resolved, name_end), 0);
    let family = u16::from_le_bytes([resolved[name_end + 4], resolved[name_end + 5]]);
    assert_eq!(family, 0x0140);
}

#[test]
fn a_top_plane_sketch_references_its_plane_with_the_plane_rotation() {
    let (resolved, _) = extruded_top_sketch();
    // The Top plane's feature number `3` and the time stamp, u16 `0`, the
    // Top plane's word `2`, u16 `0`, flag `1`, the rotation row by row (its
    // columns are the plane's axes), a zero translation and f64 `1`.
    let axes = super::tree::Plane::Top.axes();
    let mut reference = [ids::TOP_PLANE, super::TIME_STAMP]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    reference.extend([0, 0, 2, 0, 0, 0, 1]);
    reference.extend(
        (0..3)
            .flat_map(|row| axes.iter().map(move |axis| axis[row]))
            .chain([0.0, 0.0, 0.0, 1.0])
            .flat_map(f64::to_le_bytes),
    );
    // The trailer: three zero bytes, u32 `4`, i32 `-1`, u32 `0`, an
    // identifier past the extruded body's `101`, u32 `1`.
    reference.extend([0, 0, 0]);
    reference.extend(
        [4_u32, u32::MAX, 0, 102, 1]
            .iter()
            .flat_map(|word| word.to_le_bytes()),
    );
    let sketch = cstring_at(&resolved, "Sketch1");
    assert!(
        resolved[sketch..]
            .windows(reference.len())
            .any(|window| window == reference),
        "the sketch's moCompRefPlane_c names the Top plane with its rotation and identifier 102"
    );
}

#[test]
fn the_origin_sketch_holds_the_plane_reference_trailer_of_native_origin_sketches() {
    let (resolved, _) = extruded_top_sketch();
    // After the Front plane's zero frame and f64 `1`: three zero bytes, u32
    // `4`, i32 `-7`, u32 `0`, identifier `101`, u32 `0`, two empty hop lists
    // and four zero bytes, before the user sketch.
    let mut trailer = 1.0_f64.to_le_bytes().to_vec();
    trailer.extend([0, 0, 0]);
    trailer.extend(4_u32.to_le_bytes());
    trailer.extend((-7_i32).to_le_bytes());
    trailer.extend(0_u32.to_le_bytes());
    trailer.extend(super::tree::ORIGIN_PLANE_REFERENCE.to_le_bytes());
    trailer.extend([0; 12]);
    let origin = cstring_at(&resolved, "Origin");
    let sketch = cstring_at(&resolved, "Sketch1");
    assert!(
        resolved[origin..sketch]
            .windows(trailer.len())
            .any(|window| window == trailer),
        "the origin sketch's moCompRefPlane_c ends with the native trailer"
    );
    assert_eq!(super::tree::ORIGIN_PLANE_REFERENCE, 101);
}

#[test]
fn chains_name_curves_by_the_chain_identifiers_their_records_store() {
    let (resolved, walk) = extruded_top_sketch();
    // A record head: `ff ff`, schema `1f 00`, slot `3`, eight `ff` bytes and
    // f32 `-1`; the ten bytes before a curve record hold its chain
    // identifier at -8.
    let mut head = vec![0xff, 0xff, 0x1f, 0x00, 0x03];
    head.extend([0xff; 8]);
    head.extend((-1.0_f32).to_le_bytes());
    let sketch = cstring_at(&resolved, "Sketch1");
    let records = resolved[sketch..]
        .windows(head.len())
        .enumerate()
        .filter(|(_, window)| *window == head.as_slice())
        .map(|(at, _)| sketch + at)
        .collect::<Vec<_>>();
    let sketch = top_sketch();
    let curves = &records[sketch.points.len()..sketch.points.len() + sketch.curves.len()];
    let stored = curves
        .iter()
        .map(|&record| u32_at(&resolved, record - 8))
        .collect::<Vec<_>>();
    let chains = walk
        .trace
        .iter()
        .filter(|object| &*object.class == "moSketchChain_c")
        .map(|object| {
            let count = u16::from_le_bytes([resolved[object.body], resolved[object.body + 1]]);
            (0..usize::from(count))
                .map(|index| u32_at(&resolved, object.body + 2 + 4 * index))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let expected = sketch
        .chains
        .iter()
        .map(|chain| chain.iter().map(|&curve| stored[curve]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(chains, expected);
    let mut distinct = stored.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), stored.len(), "{stored:?}");
    assert!(!stored.contains(&0), "{stored:?}");
}

/// The class map of the written `Config-0`, read with the decoder.
fn class_map(streams: &[(String, Vec<u8>)]) -> std::collections::BTreeMap<u32, String> {
    let config = stream(streams, "Contents/Config-0");
    let resolved = stream(streams, "Contents/Config-0-ResolvedFeatures");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(config, &arena, &DecodePolicy::desktop()).expect("context");
    let count = crate::archive::resolved_count(resolved).expect("map count");
    crate::archive::class_index_map(config, count, &ctx)
        .expect("class map")
        .expect("the Config-0 map reaches the ResolvedFeatures count")
}

#[test]
fn a_sheet_body_takes_the_surface_body_class() {
    let streams = streams_of_kinds(&[false, true]);
    let classes = class_map(&streams);
    for class in ["moCompSolidBody_c", "moCompSurfaceBody_c"] {
        assert!(classes.values().any(|name| name == class), "{class}");
    }
    let config = stream(&streams, "Contents/Config-0");
    let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures");
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(config, &arena, &DecodePolicy::desktop()).expect("context");
    let version = crate::carchive::walk_version(None, config);
    let walk = crate::carchive::walk_configuration(&ctx, config, resolved, version)
        .expect("walk")
        .expect("the walk runs");
    assert_eq!(walk.status, crate::carchive::Status::End);
}

/// The `CString` holding `text`.
fn cstring(text: &str) -> Vec<u8> {
    let mut needle = vec![0xff, 0xfe, 0xff, u8::try_from(text.len()).expect("short")];
    needle.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    needle
}

/// The offset of the first `CString` holding `text` in `bytes`.
fn cstring_at(bytes: &[u8], text: &str) -> usize {
    let needle = cstring(text);
    bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .unwrap_or_else(|| panic!("{text} is written"))
}

/// The end of the last `CString` holding `text` in `bytes`.
fn last_cstring_end(bytes: &[u8], text: &str) -> usize {
    let needle = cstring(text);
    bytes
        .windows(needle.len())
        .rposition(|window| window == needle)
        .map_or_else(|| panic!("{text} is written"), |at| at + needle.len())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

#[test]
fn the_unit_names_follow_their_count() {
    let streams = streams(0);
    let config = stream(&streams, "Contents/Config-0");
    let first = cstring_at(config, "m");
    assert_eq!(u32_at(config, first - 4), 10);
}

#[test]
fn the_sketch_block_manager_lists_the_notes_area_view() {
    let streams = streams(0);
    let config = stream(&streams, "Contents/Config-0");
    // The annotation views take 200 and 201; the notes-area view 202.
    let notes = cstring_at(config, "Notes Area");
    let list = (0..notes)
        .rev()
        .find(|&at| {
            u32_at(config, at) == 202
                && u32_at(config, at + 4) == 199
                && u32_at(config, at + 8) == 1
                && u32_at(config, at + 12) == 202
        })
        .expect("the notes-area view index, u32 199, one identifier and the index");
    assert!(list < notes);
    // The notes-area view's 50 bytes follow its 115 view bytes.
    let tail = notes + 4 + 2 * "Notes Area".len() + 1 + 115;
    assert_eq!(config[tail + 10], 4);
}

#[test]
fn thread_data_precedes_the_exploded_view_manager() {
    let streams = streams(0);
    let config = stream(&streams, "Contents/Config-0");
    let class = b"moPrtExplViewManager_c";
    let declaration = config
        .windows(class.len())
        .position(|window| window == class)
        .expect("the manager is declared")
        - 6;
    let thread = &config[declaration - 25..declaration];
    assert!(thread[..13].iter().all(|&byte| byte == 0));
    assert_eq!(u32_at(thread, 13), 1);
    assert_eq!(u32_at(thread, 17), 0);
}

#[test]
fn a_stored_body_ends_with_its_name() {
    let streams = streams(2);
    let resolved = stream(&streams, "Contents/Config-0-ResolvedFeatures");
    let name = last_cstring_end(resolved, "Imported1");
    let tag = u16::from_le_bytes([resolved[name], resolved[name + 1]]);
    assert_eq!(tag & 0x8000, 0x8000, "a class tag follows the name");
    assert_eq!(last_cstring_end(resolved, "Imported2"), resolved.len());
}
