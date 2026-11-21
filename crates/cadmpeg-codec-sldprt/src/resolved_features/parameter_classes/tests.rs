//! Tests for the `parameter_classes` module.

use super::super::names::{class_declarations, object_names};
use super::super::scalars::named_scalars;
use super::super::SCALAR_HEADER;
use super::{stated_parameters, StatedParameter};
use crate::records::FeatureInputLane;

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

    /// The name record and the primary-layout scalar of `role` (`0`
    /// driving, `1` display) holding the cell `value`.
    fn scalar(&mut self, name: &str, value: &[u8], role: u8, object: u32) -> &mut Self {
        self.0
            .extend_from_slice(&(0x8000 | NAME_CLASS).to_le_bytes());
        self.0.extend_from_slice(&[0xff, 0xfe, 0xff]);
        self.0
            .push(u8::try_from(name.encode_utf16().count()).unwrap());
        for unit in name.encode_utf16() {
            self.0.extend_from_slice(&unit.to_le_bytes());
        }
        self.0.extend_from_slice(SCALAR_HEADER);
        self.0.extend_from_slice(value);
        let trailer = self.0.len();
        self.0.resize(trailer + 35, 0);
        self.0[trailer + 3..trailer + 7].copy_from_slice(&object.to_le_bytes());
        self.0[trailer + 24..trailer + 29].copy_from_slice(&[0, 0, 0, 2, 0]);
        self.0[trailer + 29] = role;
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

fn metres(value: f64) -> [u8; 8] {
    value.to_le_bytes()
}

/// The parameter each scalar states, by scalar name in stream order.
fn stated_by_name(lane: &FeatureInputLane) -> Vec<(String, Option<StatedParameter>)> {
    let stated = stated_parameters(lane);
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
