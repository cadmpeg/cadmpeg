//! Tests for the `parameter_classes` module.

use super::super::names::{class_declarations, object_names};
use super::super::scalars::named_scalars;
use super::super::{COMPACT_SCALAR_HEADER, SCALAR_HEADER, VALUE_ONLY_SCALAR_HEADER};
use super::{stated_parameters, ConfigurationMap, StatedParameter};
use crate::records::FeatureInputLane;
use crate::test_support::carchive::{config_object_stream, config_object_stream_declaring};
use crate::test_support::container::{make_block, outer_header, scan};

/// The map count the synthetic `Config-0` stream of
/// [`config_object_stream_declaring`] ends at; it declares its class at 7.
const CONFIGURATION_MAP_COUNT: u32 = 9;

/// The class index the name records of these streams take.
const NAME_CLASS: u16 = 0x72;

/// A `ResolvedFeatures` stream assembled from named parts.
struct Stream(Vec<u8>);

impl Stream {
    /// A compound-file stream, which opens with a u16 and states no map
    /// count; `counted` opens with the map count `count` instead.
    fn compound() -> Self {
        Self(vec![0x20, 0x00])
    }

    /// A stream that opens with its first class declaration.
    fn declared() -> Self {
        Self(Vec::new())
    }

    fn counted(count: u32) -> Self {
        let mut bytes = count.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x02, 0x00]);
        Self(bytes)
    }

    fn declare(&mut self, class: &str) -> &mut Self {
        self.0.extend_from_slice(&[0xff, 0xff, 0x01, 0x00]);
        self.0
            .extend_from_slice(&u16::try_from(class.len()).unwrap().to_le_bytes());
        self.0.extend_from_slice(class.as_bytes());
        self
    }

    fn token(&mut self, index: u16) -> &mut Self {
        self.0.extend_from_slice(&(0x8000 | index).to_le_bytes());
        self
    }

    fn zero_word(&mut self) -> &mut Self {
        self.0.extend_from_slice(&[0; 4]);
        self
    }

    fn name_record(&mut self, name: &str) -> &mut Self {
        self.0
            .extend_from_slice(&(0x8000 | NAME_CLASS).to_le_bytes());
        self.0.extend_from_slice(&[0xff, 0xfe, 0xff]);
        self.0
            .push(u8::try_from(name.encode_utf16().count()).unwrap());
        for unit in name.encode_utf16() {
            self.0.extend_from_slice(&unit.to_le_bytes());
        }
        self
    }

    /// The name record and the primary-layout scalar of `role` (`0`
    /// driving, `1` display) holding the cell `value`.
    fn scalar(&mut self, name: &str, value: &[u8], role: u8, object: u32) -> &mut Self {
        self.name_record(name);
        self.0.extend_from_slice(SCALAR_HEADER);
        self.0.extend_from_slice(value);
        let trailer = self.0.len();
        self.0.resize(trailer + 35, 0);
        self.0[trailer + 3..trailer + 7].copy_from_slice(&object.to_le_bytes());
        self.0[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
        self.0[trailer + 29] = role;
        self
    }

    /// The name record and a count scalar behind `header`: the i32 `count`,
    /// then its trailer of three zero bytes and the scalar's object
    /// identifier.
    fn count(&mut self, name: &str, header: &[u8], count: i32, object: u32) -> &mut Self {
        self.name_record(name);
        self.0.extend_from_slice(header);
        self.0.extend_from_slice(&count.to_le_bytes());
        self.0.extend_from_slice(&[0, 0, 0]);
        self.0.extend_from_slice(&object.to_le_bytes());
        self.0.resize(self.0.len() + 40, 0);
        self
    }

    /// The lane the decoder's own readers make of the stream.
    fn lane(&self) -> FeatureInputLane {
        let payload = self.0.as_slice();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, &policy)
                .expect("test context");
        let names = object_names(&ctx, payload, "lane").unwrap();
        let scalars = named_scalars(payload, "lane", &names).unwrap();
        FeatureInputLane {
            id: "lane".into(),
            configuration: None,
            native_payload: payload.to_vec().into(),
            classes: class_declarations(payload, "lane").unwrap(),
            names,
            scalars,
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }
    }
}

/// The lane the decoder reads from a part whose `Config-0` stream is
/// `config` and whose `Config-0-ResolvedFeatures` stream is `stream`, and the
/// `Config-N` class map the decoder gives that lane.
fn continued_lane(config: &[u8], stream: &Stream) -> (FeatureInputLane, Option<ConfigurationMap>) {
    let mut file = outer_header();
    file.extend_from_slice(&make_block(0x20, "Contents/Config-0", config));
    file.extend_from_slice(&make_block(
        0x21,
        "Contents/Config-0-ResolvedFeatures",
        &stream.0,
    ));
    let scan = scan(&file);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let (mut lanes, _, _, mut maps) = super::super::assembly::lanes(
        &ctx,
        &scan,
        &mut cadmpeg_ir::annotations::Annotations::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(lanes.len(), 1);
    let lane = lanes.remove(0);
    let configuration = maps.remove(&lane.id);
    (lane, configuration)
}

fn metres(value: f64) -> [u8; 8] {
    value.to_le_bytes()
}

/// The parameter each scalar states, by scalar name in stream order, where
/// the lane continues `configuration`.
fn stated_in(
    lane: &FeatureInputLane,
    configuration: Option<&ConfigurationMap>,
) -> Vec<(String, Option<StatedParameter>)> {
    let stated = stated_parameters(lane, configuration);
    let names = lane
        .names
        .iter()
        .map(|name| (name.id.as_str(), name.value.as_str()))
        .collect::<std::collections::HashMap<_, _>>();
    lane.scalars
        .iter()
        .map(|scalar| {
            (
                names[scalar.name.as_str()].to_owned(),
                stated.get(scalar.id.as_str()).copied(),
            )
        })
        .collect()
}

/// The parameter each scalar of a lane that continues no `Config-N` map
/// states, by scalar name in stream order.
fn stated_by_name(lane: &FeatureInputLane) -> Vec<(String, Option<StatedParameter>)> {
    stated_in(lane, None)
}

#[test]
fn a_scalar_states_the_parameter_class_declared_or_named_by_its_token() {
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.025), 0, 7)
        .token(300)
        .zero_word()
        .token(302)
        .scalar("D2", &metres(0.017), 0, 8)
        .declare("sgAnglDim")
        .zero_word()
        .declare("moAngleParameter_c")
        .scalar("D3", &metres(0.5), 0, 9)
        .token(300)
        .zero_word()
        .token(302)
        .scalar("D4", &metres(0.04), 1, 10)
        .token(410)
        .zero_word()
        .token(412)
        .scalar("D5", &metres(0.25), 0, 11);
    assert_eq!(
        stated_by_name(&stream.lane()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.025))),
            ("D2".into(), Some(StatedParameter::Length(0.017))),
            ("D3".into(), Some(StatedParameter::Angle(0.5))),
            ("D4".into(), Some(StatedParameter::Length(0.04))),
            ("D5".into(), Some(StatedParameter::Angle(0.25))),
        ]
    );
}

#[test]
fn a_count_states_its_four_byte_cell() {
    let mut stream = Stream::compound();
    stream
        .declare("sgCircularPattCntDim")
        .zero_word()
        .declare("moIntegerParameter_c")
        .scalar("D3", &5i32.to_le_bytes(), 0, 7);
    assert_eq!(
        stated_by_name(&stream.lane()),
        vec![("D3".into(), Some(StatedParameter::Count(5)))]
    );
}

#[test]
fn a_count_behind_the_compact_header_states_the_cell_after_it() {
    let mut compact = Stream::compound();
    compact
        .declare("sgCircularPattCntDim")
        .zero_word()
        .declare("moIntegerParameter_c")
        .count("D3", COMPACT_SCALAR_HEADER, 3, 7);
    assert_eq!(
        stated_by_name(&compact.lane()),
        vec![("D3".into(), Some(StatedParameter::Count(3)))]
    );
    // A zero count behind the value-only header has the compact header's
    // bytes; its trailer follows the zero count.
    let mut value_only = Stream::compound();
    value_only
        .declare("sgCircularPattCntDim")
        .zero_word()
        .declare("moIntegerParameter_c")
        .count("D3", VALUE_ONLY_SCALAR_HEADER, 0, 7);
    assert_eq!(
        stated_by_name(&value_only.lane()),
        vec![("D3".into(), Some(StatedParameter::Count(0)))]
    );
}

#[test]
fn a_token_no_declaration_of_the_lane_answers_states_no_class() {
    // The token is used before the lane's only parameter-class declaration,
    // so it names a class declared outside the lane.
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .token(282)
        .scalar("D1", &metres(0.012), 0, 7)
        .declare("sgAnglDim")
        .zero_word()
        .declare("moAngleParameter_c")
        .scalar("A1", &metres(0.7), 0, 8)
        .token(300)
        .zero_word()
        .token(282)
        .scalar("D2", &metres(0.003), 0, 9);
    assert_eq!(
        stated_by_name(&stream.lane()),
        vec![
            ("D1".into(), None),
            ("A1".into(), Some(StatedParameter::Angle(0.7))),
            ("D2".into(), None),
        ]
    );
}

#[test]
fn two_assignments_of_tokens_to_declarations_state_no_class() {
    // One token and two declarations before its first use: either class fits.
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .declare("sgAnglDim")
        .zero_word()
        .declare("moAngleParameter_c")
        .scalar("A1", &metres(0.2), 0, 8)
        .token(500)
        .zero_word()
        .token(502)
        .scalar("D2", &metres(0.02), 0, 9);
    let stated = stated_by_name(&stream.lane());
    assert_eq!(stated[2], ("D2".into(), None));
}

#[test]
fn a_token_every_assignment_gives_one_class_has_that_class() {
    // Token 502 is first used before the integer declaration, so every
    // assignment gives it the length class; token 602 is first used after
    // both the integer and the angle declarations.
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(500)
        .zero_word()
        .token(502)
        .scalar("D2", &metres(0.02), 0, 8)
        .declare("sgCircularPattCntDim")
        .zero_word()
        .declare("moIntegerParameter_c")
        .scalar("D3", &4i32.to_le_bytes(), 0, 9)
        .declare("sgAnglDim")
        .zero_word()
        .declare("moAngleParameter_c")
        .scalar("A1", &metres(0.3), 0, 10)
        .token(600)
        .zero_word()
        .token(602)
        .scalar("A2", &metres(0.4), 0, 11);
    let stated = stated_by_name(&stream.lane());
    assert_eq!(
        stated[1],
        ("D2".into(), Some(StatedParameter::Length(0.02)))
    );
    assert_eq!(stated[4], ("A2".into(), None));
}

#[test]
fn a_parameter_class_declared_twice_names_no_token() {
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .declare("moLengthParameter_c")
        .scalar("D2", &metres(0.02), 0, 8)
        .token(500)
        .zero_word()
        .token(502)
        .scalar("D3", &metres(0.03), 0, 9);
    assert_eq!(
        stated_by_name(&stream.lane()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), Some(StatedParameter::Length(0.02))),
            ("D3".into(), None),
        ]
    );
}

#[test]
fn a_token_below_the_stream_map_count_names_a_configuration_class() {
    let mut stream = Stream::counted(400);
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(401)
        .zero_word()
        .token(120)
        .scalar("D2", &metres(0.02), 0, 8);
    assert_eq!(
        stated_by_name(&stream.lane()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), None),
        ]
    );
}

#[test]
fn a_stream_that_opens_with_a_declaration_states_no_map_count() {
    // Its first four bytes are the declaration marker, which read as a u32
    // would put every token below the count.
    let mut stream = Stream::declared();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(401)
        .zero_word()
        .token(469)
        .scalar("D2", &metres(0.02), 0, 8);
    assert_eq!(
        stated_by_name(&stream.lane()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), Some(StatedParameter::Length(0.02))),
        ]
    );
}

#[test]
fn a_token_below_the_map_count_names_the_class_config_n_declares() {
    // `Config-0` declares `moLengthParameter_c` at 7; the lane's own
    // declarations take 9 on.
    let mut stream = Stream::counted(CONFIGURATION_MAP_COUNT);
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moAngleParameter_c")
        .scalar("A1", &metres(0.5), 0, 7)
        .token(9)
        .zero_word()
        .token(7)
        .scalar("D1", &metres(0.017), 0, 8);
    let (lane, configuration) = continued_lane(&config_object_stream(), &stream);
    assert_eq!(
        stated_in(&lane, configuration.as_ref()),
        vec![
            ("A1".into(), Some(StatedParameter::Angle(0.5))),
            ("D1".into(), Some(StatedParameter::Length(0.017))),
        ]
    );
}

#[test]
fn a_token_config_n_holds_no_parameter_class_at_names_none() {
    // `Config-0` holds `moNoteData_c` at 7. The lane's own length class
    // takes 11, after `sgPntPntDist` and its object at 9 and 10.
    let mut stream = Stream::counted(CONFIGURATION_MAP_COUNT);
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(9)
        .zero_word()
        .token(7)
        .scalar("D2", &metres(0.02), 0, 8)
        .token(9)
        .zero_word()
        .token(11)
        .scalar("D3", &metres(0.03), 0, 9);
    let (lane, configuration) =
        continued_lane(&config_object_stream_declaring("moNoteData_c"), &stream);
    assert_eq!(
        stated_in(&lane, configuration.as_ref()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), None),
            ("D3".into(), Some(StatedParameter::Length(0.03))),
        ]
    );
}

#[test]
fn a_compound_lane_token_names_the_class_the_config_n_walk_places() {
    // The compound-file stream states no count; `Config-0` ends at 9. Token
    // 7 is first used after the lane's only parameter declaration, so the
    // lane's declarations alone would give it the length class; `Config-0`
    // holds an angle at 7.
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(9)
        .zero_word()
        .token(7)
        .scalar("D2", &metres(0.02), 0, 8);
    let (lane, configuration) = continued_lane(
        &config_object_stream_declaring("moAngleParameter_c"),
        &stream,
    );
    assert_eq!(
        stated_in(&lane, configuration.as_ref()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), Some(StatedParameter::Angle(0.02))),
        ]
    );
}

#[test]
fn a_compound_lane_token_at_no_config_n_class_takes_the_lane_class() {
    // The walk of `Config-0` places classes at 2, 4 and 7 only. Token 8
    // lies below the walk's end, 9, and names the lane's length class.
    let mut stream = Stream::compound();
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(6)
        .zero_word()
        .token(8)
        .scalar("D2", &metres(0.02), 0, 8);
    let (lane, configuration) = continued_lane(&config_object_stream(), &stream);
    assert_eq!(
        stated_in(&lane, configuration.as_ref()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), Some(StatedParameter::Length(0.02))),
        ]
    );
}

#[test]
fn a_token_below_a_count_the_config_n_walk_misses_names_none() {
    // `Config-0` ends at 9, so its walk does not reconcile with the count
    // 12. Token 7 lies below the count and names no class; the lane's own
    // length class at 14 keeps its tokens.
    let mut stream = Stream::counted(12);
    stream
        .declare("sgPntPntDist")
        .zero_word()
        .declare("moLengthParameter_c")
        .scalar("D1", &metres(0.01), 0, 7)
        .token(12)
        .zero_word()
        .token(7)
        .scalar("D2", &metres(0.02), 0, 8)
        .token(12)
        .zero_word()
        .token(14)
        .scalar("D3", &metres(0.03), 0, 9);
    let (lane, configuration) = continued_lane(&config_object_stream(), &stream);
    assert_eq!(
        configuration,
        Some(ConfigurationMap::Counted {
            end: 12,
            parameters: None
        })
    );
    assert_eq!(
        stated_in(&lane, configuration.as_ref()),
        vec![
            ("D1".into(), Some(StatedParameter::Length(0.01))),
            ("D2".into(), None),
            ("D3".into(), Some(StatedParameter::Length(0.03))),
        ]
    );
}
