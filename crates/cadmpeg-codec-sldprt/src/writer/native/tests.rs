// SPDX-License-Identifier: Apache-2.0
//! The skeleton's object streams read to their end with the decoder's exact
//! walker, and its `KeyWords` and class map take the native form.

use super::{skeleton, Body, Facts};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn streams(bodies: usize) -> Vec<(String, Vec<u8>)> {
    let bodies = (0..bodies)
        .map(|index| Body::numbered("Bracket", 0, index))
        .collect::<Result<Vec<_>, _>>()
        .expect("bodies number");
    skeleton(&Facts {
        id_base: 0,
        name: "Bracket",
        active: "Default",
        sketches: &[],
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
        let count = crate::archive::resolved_features_count(resolved).expect("map count");
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

#[test]
fn tree_identifiers_follow_the_identifier_base() {
    let bodies = [Body::numbered("Bracket", 40, 0).expect("body numbers")];
    let streams = skeleton(&Facts {
        id_base: 40,
        name: "Bracket",
        active: "Default",
        sketches: &[],
        bodies: &bodies,
    })
    .expect("the skeleton writes");
    let keywords = stream(&streams, "swXmlContents/KeyWords");
    let text = std::str::from_utf8(&keywords[1..]).expect("UTF-8");
    let document = roxmltree::Document::parse(text).expect("XML");
    let ids = document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() != "Configuration")
        .map(|node| node.attribute("id").and_then(|id| id.parse::<u32>().ok()))
        .collect::<Option<Vec<_>>>()
        .expect("numeric identifiers");
    assert!(ids.iter().all(|&id| id > 40), "{ids:?}");
    assert_eq!(bodies[0].feature_id, 40 + super::ids::MARKUPS + 1);
}

/// A sketch on the Top plane: a line, an arc from its end and a circle.
fn top_sketch() -> super::sketch::Sketch {
    use super::sketch::{Curve, Point, Sketch};
    let point = |at: [f64; 2], kind: u32| Point { at, kind };
    Sketch {
        name: "Sketch1".into(),
        feature_id: 7,
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
        body: b"PS\0\0 profile body".repeat(8),
    }
}

#[test]
fn a_user_sketch_walks_to_the_end_and_holds_its_profile_body() {
    let sketches = [top_sketch()];
    let bodies = [Body::numbered("Bracket", 7, 0).expect("body numbers")];
    let streams = skeleton(&Facts {
        id_base: 7,
        name: "Bracket",
        active: "Default",
        sketches: &sketches,
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
