// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;
use std::sync::Arc;

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_asm::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation, SplitBodyTool};

use super::*;
use crate::feature::body_selection::WrapperLink;
use crate::feature::tests::{reference, test_feature, test_header};
use crate::feature::{
    PmDcFeaturePropertyKind, PmDcFeaturePropertyPayload, PmDcKernelEntityLinkPayload,
    KERNEL_ENTITY_LINK_TYPE, SURFACE_BODY_TYPE,
};
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};

const FAMILY_TAG: &str =
    "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib";

fn record(name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index: 0,
        name: name.to_owned(),
        tokens: Arc::from(tokens),
        offset: 0,
        len: 0,
    }
}

/// The five common attribute fields.
fn common() -> Vec<Token> {
    vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(1),
    ]
}

fn tagged(name: &str, rest: &[Token]) -> Record {
    let mut tokens = common();
    tokens.extend_from_slice(rest);
    record(name, tokens)
}

fn longs(values: &[i64]) -> Vec<Token> {
    values.iter().copied().map(Token::Long).collect()
}

fn custom(family: &str, values: &[i64]) -> Record {
    let mut rest = vec![Token::Str(family.to_owned())];
    rest.extend(longs(values));
    tagged("ATTRIB_CUSTOM-attrib", &rest)
}

#[test]
fn naming_tag_lists_every_source_of_its_pairs_in_schemas_38_40_42() {
    for schema in [38, 40, 42] {
        let one = tagged(FAMILY_TAG, &longs(&[schema, 1, 24, -1, 3, 0, 1]));
        assert_eq!(naming_sources(&one), Some(vec![24]));
        let two = tagged(FAMILY_TAG, &longs(&[schema, 2, 24, -1, 63, 5, 3, 0, 1]));
        assert_eq!(naming_sources(&two), Some(vec![24, 63]));
    }
    let hole = tagged(
        "NMx_Hole_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib",
        &longs(&[43, 1, 251, -1]),
    );
    assert_eq!(naming_sources(&hole), Some(vec![251]));
    let feature = tagged(
        "NMx_Brep_Feature_tag-NMx_Brep_tag-NamingMatching-attrib",
        &longs(&[38, 1, 20, -1]),
    );
    assert_eq!(naming_sources(&feature), Some(vec![20]));
}

#[test]
fn naming_tag_refuses_forms_outside_the_pair_layout() {
    let zero_count = tagged(FAMILY_TAG, &longs(&[38, 0]));
    assert_eq!(naming_sources(&zero_count), None);
    let short = tagged(FAMILY_TAG, &longs(&[38, 2, 24, -1, 63]));
    assert_eq!(naming_sources(&short), None);
    let negative_source = tagged(FAMILY_TAG, &longs(&[38, 1, -2, -1]));
    assert_eq!(naming_sources(&negative_source), None);
    let mut reference_name = common();
    reference_name.extend(longs(&[38, 1, 24]));
    reference_name.push(Token::Ref(3));
    assert_eq!(naming_sources(&record(FAMILY_TAG, reference_name)), None);
    let mut no_schema = common();
    no_schema.push(Token::Ref(3));
    assert_eq!(naming_sources(&record(FAMILY_TAG, no_schema)), None);
}

#[test]
fn naming_tag_refuses_classes_that_are_not_naming_tags() {
    let values = longs(&[38, 1, 281, -1, 2, 1]);
    for name in [
        "NMx_Matched_Entity-NamingMatching-attrib",
        "NMx_feature_dependency_attrib-NamingMatching-attrib",
        "NMx_Feature_Orientation-NamingMatching-attrib",
        "NMx_FFColor_Entity-NamingMatching-attrib",
        "Other_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib",
        "NMx_Brep_Name_tag-NMx_Brep_tag-attrib",
    ] {
        assert_eq!(naming_sources(&tagged(name, &values)), None, "{name}");
    }
}

#[test]
fn schema_44_tag_lists_every_source_of_its_pairs() {
    for family in SCHEMA_44_FAMILIES {
        assert_eq!(
            naming_sources(&custom(family, &[1, 44, 1, 60, -1, 2, 0, 1])),
            Some(vec![60]),
            "{family}"
        );
    }
    assert_eq!(
        naming_sources(&custom(
            "INV_NMX_GEN_TAG",
            &[1, 44, 2, 24, -1, 63, -1, 3, 0, 1]
        )),
        Some(vec![24, 63])
    );
}

#[test]
fn schema_44_tag_refuses_other_families_and_forms() {
    let body = [1, 44, 1, 60, -1];
    for family in [
        "INV_NMX_ORDERED_GEN_TAG",
        "INV_NMX_MATCHED_ATTRIB",
        "INV_NMX_DUP",
        "INV_FEATURE_ORIENTATION_ATTR",
    ] {
        assert_eq!(naming_sources(&custom(family, &body)), None, "{family}");
    }
    assert_eq!(
        naming_sources(&custom("INV_NMX_GEN_TAG", &[1, 43, 1, 60, -1])),
        None
    );
    assert_eq!(
        naming_sources(&custom("INV_NMX_GEN_TAG", &[2, 44, 1, 60, -1])),
        None
    );
    assert_eq!(
        naming_sources(&custom("INV_NMX_GEN_TAG", &[1, 44, 0])),
        None
    );
    assert_eq!(
        naming_sources(&custom("INV_NMX_GEN_TAG", &[1, 44, 1, 60])),
        None
    );
    let mut numeric_family = common();
    numeric_family.extend(longs(&[7, 1, 44, 1, 60, -1]));
    assert_eq!(
        naming_sources(&record("ATTRIB_CUSTOM-attrib", numeric_family)),
        None
    );
}

#[test]
fn body_records_classify_by_key() {
    let body = |key| record("body", vec![Token::Ref(-1), Token::Long(key)]);
    assert_eq!(classify(&body(7)), Some(Class::Body(7)));
    assert_eq!(classify(&body(-1)), None);
    assert_eq!(classify(&record("body", vec![Token::Ref(-1)])), None);
    assert_eq!(
        classify(&record("face", vec![Token::Ref(-1), Token::Long(7)])),
        None
    );
}

// A carrier stream of three states, built byte by byte.
//
// Live records, newest state: 0 asmheader, 1 body key 7, 2 naming tag for
// source 5, 3 schema-44 tag for source 6. Archived revision 4 is a body with
// key 9. State chain from the head: H (inserts entity 3), M (replaces entity 1
// with revision 4 and inserts entity 2), L (inserts entity 1). From the oldest
// state the models are: L {asmheader, body 9}; M {asmheader, body 7, tag 5};
// H adds tag 6.

const BINARY_FILE_8_LEN: usize = 47;
const BINARY_FILE_4_LEN: usize = 31;

fn put_int(out: &mut Vec<u8>, width: RefWidth, tag: u8, value: i64) {
    out.push(tag);
    match width {
        RefWidth::Four => out.extend_from_slice(
            &i32::try_from(value)
                .expect("fixture value fits i32")
                .to_le_bytes(),
        ),
        RefWidth::Eight => out.extend_from_slice(&value.to_le_bytes()),
    }
}

fn header(width: RefWidth) -> Vec<u8> {
    let (magic, len): (&[u8], usize) = match width {
        RefWidth::Four => (b"ASM BinaryFile4", BINARY_FILE_4_LEN),
        RefWidth::Eight => (b"ASM BinaryFile8", BINARY_FILE_8_LEN),
    };
    let mut bytes = magic.to_vec();
    bytes.resize(len, 0);
    for _ in 0..3 {
        bytes.extend_from_slice(&[0x07, 0]);
    }
    for _ in 0..3 {
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
    }
    bytes
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    let parts: Vec<&str> = name.split('-').collect();
    for (index, part) in parts.iter().enumerate() {
        out.push(if index + 1 == parts.len() { 0x0d } else { 0x0e });
        out.push(u8::try_from(part.len()).expect("fixture name fits a byte"));
        out.extend_from_slice(part.as_bytes());
    }
}

fn put_record(out: &mut Vec<u8>, width: RefWidth, name: &str, tokens: &[Token]) {
    put_name(out, name);
    for token in tokens {
        match token {
            Token::Long(value) => put_int(out, width, 0x04, *value),
            Token::Ref(value) => put_int(out, width, 0x0c, *value),
            Token::Str(text) => {
                out.push(0x07);
                out.push(u8::try_from(text.len()).expect("fixture string fits a byte"));
                out.extend_from_slice(text.as_bytes());
            }
            other => panic!("fixture token {other:?}"),
        }
    }
    out.push(0x11);
}

/// A `delta_state` record with one board of `(old, new)` changes.
fn put_state(
    out: &mut Vec<u8>,
    width: RefWidth,
    id: i64,
    previous: i64,
    next: i64,
    node: i64,
    changes: &[(i64, i64)],
) {
    put_name(out, "delta_state");
    for value in [id, 1, 0] {
        put_int(out, width, 0x04, value);
    }
    for value in [previous, next, node, -1, -1] {
        put_int(out, width, 0x0c, value);
    }
    out.push(0x0b);
    put_int(out, width, 0x04, 1);
    put_int(out, width, 0x0c, node);
    put_int(out, width, 0x04, 1);
    for &(old, new) in changes {
        put_int(out, width, 0x04, 1);
        put_int(out, width, 0x0c, old);
        put_int(out, width, 0x0c, new);
    }
    put_int(out, width, 0x04, 0);
    put_int(out, width, 0x04, 0);
    out.push(0x11);
}

fn tag_5_tokens() -> Vec<Token> {
    let mut tokens = common();
    tokens.extend(longs(&[38, 1, 5, -1, 0]));
    tokens
}

fn tag_6_tokens() -> Vec<Token> {
    let mut tokens = common();
    tokens.push(Token::Str("INV_NMX_GEN_TAG".to_owned()));
    tokens.extend(longs(&[1, 44, 1, 6, -1, 2]));
    tokens
}

/// Options that break the fixture in one place.
#[derive(Clone, Copy, Default)]
struct Break {
    /// Drop the last change of the oldest state, which leaves slot 1 live.
    open_chain: bool,
    /// Corrupt the tag of the first header integer of the middle state.
    bad_header: bool,
}

fn carrier(width: RefWidth, flaw: Break) -> Vec<u8> {
    let mut out = header(width);
    put_record(&mut out, width, "asmheader", &[]);
    put_record(&mut out, width, "body", &[Token::Ref(-1), Token::Long(7)]);
    put_record(&mut out, width, FAMILY_TAG, &tag_5_tokens());
    put_record(&mut out, width, "ATTRIB_CUSTOM-attrib", &tag_6_tokens());
    put_state(&mut out, width, 13, -1, 1, 0, &[(-1, 3)]);
    let middle = out.len();
    put_state(&mut out, width, 12, 0, 2, 1, &[(4, 1), (-1, 2)]);
    put_record(&mut out, width, "body", &[Token::Ref(-1), Token::Long(9)]);
    let oldest: &[(i64, i64)] = if flaw.open_chain { &[] } else { &[(-1, 1)] };
    put_state(&mut out, width, 11, 1, -1, 2, oldest);
    if flaw.bad_header {
        // The identifier name of the record is 13 bytes; the tag of the first
        // header integer follows it.
        out[middle + 13] = 0x7f;
    }
    out
}

/// The state index of a stream, or `None` when it holds no `delta_state`.
fn read(bytes: &[u8], width: RefWidth, limit: usize) -> Option<Reading> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("context");
    let framed = match frame_chain(&ctx, bytes, width).expect("frame") {
        Framing::Framed(chain) => chain,
        Framing::Absent => return None,
        Framing::Unread(reason) => return Some(Reading::Unread(reason)),
    };
    Some(
        match prepare(&ctx, &framed, bytes, width).expect("prepare") {
            Prepared::Unread(reason) => Reading::Unread(reason),
            Prepared::Ready(chain) => index_chain(&chain, 44, limit),
        },
    )
}

#[test]
fn the_state_index_gives_the_oldest_state_of_each_source_in_both_reference_widths() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let bytes = carrier(width, Break::default());
        let Some(Reading::Read(index)) = read(&bytes, width, MAX_STATE_SCAN_WORK) else {
            panic!("chain did not read at {width:?}");
        };
        let ids: Vec<i64> = index.states.iter().map(|state| state.state_id).collect();
        assert_eq!(ids, [11, 12, 13]);
        let keys: Vec<Vec<i64>> = index
            .states
            .iter()
            .map(|state| state.body_keys.clone())
            .collect();
        assert_eq!(keys, [vec![9], vec![7], vec![7]]);
        assert_eq!(index.first.get(&5), Some(&1));
        assert_eq!(index.first.get(&6), Some(&2));
        assert_eq!(index.first.len(), 2);
        // The fixture reads as a schema-44 carrier, and the tag of source 5 stores 38.
        assert_eq!(index.schema_mismatch, BTreeSet::from([5]));
    }
}

#[test]
fn a_stream_without_states_is_absent() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let mut bytes = header(width);
        put_record(&mut bytes, width, "asmheader", &[]);
        assert_eq!(read(&bytes, width, MAX_STATE_SCAN_WORK), None);
    }
}

#[test]
fn a_chain_that_does_not_close_or_frame_is_unread() {
    for width in [RefWidth::Four, RefWidth::Eight] {
        let open = carrier(
            width,
            Break {
                open_chain: true,
                ..Break::default()
            },
        );
        assert_eq!(
            read(&open, width, MAX_STATE_SCAN_WORK),
            Some(Reading::Unread(Unread::Unclosed))
        );
        let broken = carrier(
            width,
            Break {
                bad_header: true,
                ..Break::default()
            },
        );
        assert_eq!(
            read(&broken, width, MAX_STATE_SCAN_WORK),
            Some(Reading::Unread(Unread::Unclosed))
        );
    }
}

#[test]
fn a_scan_over_the_work_cap_is_bounded() {
    let bytes = carrier(RefWidth::Eight, Break::default());
    assert_eq!(
        read(&bytes, RefWidth::Eight, 1),
        Some(Reading::Unread(Unread::Bounded))
    );
}

fn index() -> StateIndex {
    StateIndex {
        states: vec![
            IndexedState {
                state_id: 11,
                body_keys: vec![9],
            },
            IndexedState {
                state_id: 12,
                body_keys: vec![7],
            },
            IndexedState {
                state_id: 13,
                body_keys: vec![7],
            },
        ],
        first: HashMap::from([(5, 1), (6, 2), (8, 0)]),
        schema_mismatch: BTreeSet::new(),
    }
}

fn blank(source_index: u32) -> FeatureHistoryState {
    FeatureHistoryState {
        id: "state".to_owned(),
        feature: "feature".to_owned(),
        source_index,
        kind: String::new(),
        state_after: None,
        state_before: None,
        body_key: None,
        history_body: None,
        binding: Binding::Bound,
    }
}

fn unbound(reason: UnboundReason) -> Binding {
    Binding::Unbound { reason }
}

#[test]
fn a_feature_binds_to_the_state_that_adds_its_tag_and_the_state_before() {
    let record = bind(&index(), blank(5), Check::None, None);
    assert_eq!(record.binding, Binding::Bound);
    assert_eq!(
        record.state_after,
        Some(StateRef {
            ordinal: 1,
            state_id: 12
        })
    );
    assert_eq!(record.state_before, Some(0));
    assert_eq!(record.body_key, None);
}

#[test]
fn a_feature_without_a_tag_is_unbound() {
    let record = bind(&index(), blank(99), Check::None, None);
    assert_eq!(record.binding, unbound(UnboundReason::NoTag));
    assert_eq!(record.state_after, None);
    assert_eq!(record.state_before, None);
    let split = bind(&index(), blank(99), Check::Split, Some(9));
    assert_eq!(split.binding, unbound(UnboundReason::NoTag));
}

#[test]
fn a_split_needs_a_body_key_that_leaves_between_the_two_states() {
    let bound = bind(&index(), blank(5), Check::Split, Some(9));
    assert_eq!(bound.binding, Binding::Bound);
    assert_eq!(bound.body_key, Some(9));
    // The key stays in both states, or is in neither, or only appears.
    for key in [7, 3] {
        let record = bind(&index(), blank(5), Check::Split, Some(key));
        assert_eq!(
            record.binding,
            unbound(UnboundReason::BodyKeyMismatch),
            "{key}"
        );
    }
    let unresolved = bind(&index(), blank(5), Check::Split, None);
    assert_eq!(
        unresolved.binding,
        unbound(UnboundReason::BodyKeyUnresolved)
    );
}

#[test]
fn a_base_needs_a_body_key_that_appears_between_the_two_states() {
    let bound = bind(&index(), blank(5), Check::Base, Some(7));
    assert_eq!(bound.binding, Binding::Bound);
    assert_eq!(bound.body_key, Some(7));
    for key in [9, 3] {
        let record = bind(&index(), blank(5), Check::Base, Some(key));
        assert_eq!(
            record.binding,
            unbound(UnboundReason::BodyKeyMismatch),
            "{key}"
        );
    }
    // Source 6 is added in the newest state, which keeps key 7 from the state
    // before it.
    let kept = bind(&index(), blank(6), Check::Base, Some(7));
    assert_eq!(kept.binding, unbound(UnboundReason::BodyKeyMismatch));
}

#[test]
fn a_body_check_at_the_oldest_state_has_no_predecessor() {
    let record = bind(&index(), blank(8), Check::Base, Some(9));
    assert_eq!(record.state_before, None);
    assert_eq!(record.binding, unbound(UnboundReason::NoPredecessor));
    let plain = bind(&index(), blank(8), Check::None, None);
    assert_eq!(plain.binding, Binding::Bound);
    assert_eq!(plain.state_before, None);
}

fn link(ordinal: u32, entity_id: u32) -> PmDcKernelEntityLink {
    Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 18,
            header_value: 0,
            header_id: 77,
            selection_source: 0,
            state: 0,
            body: reference(1),
            entity_id,
            entity_type: 42,
        },
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        token(),
        ordinal,
    )
}

fn token() -> cadmpeg_ir::ids::IdentityKey {
    cadmpeg_ir::identity_key!("fixture")
}

/// A surface-body wrapper record at `ordinal`.
fn wrapper_property(ordinal: u32) -> PmDcFeatureProperty {
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 18,
            header: test_header(),
            kind: PmDcFeaturePropertyKind::SurfaceBody {
                body: PmDcReference::from_packed(ordinal),
            },
        },
        RecordTypeId::from_bytes(SURFACE_BODY_TYPE),
        token(),
        ordinal,
    )
}

fn source(ordinal: u32, source_index: u32, slots: &[(usize, u32)]) -> PmDcFeature {
    let feature = test_feature(ordinal, if slots.is_empty() { 0 } else { 3 }, slots);
    let mut feature = Located::new(
        (*feature).clone(),
        RecordTypeId::from_bytes(crate::feature::FEATURE_TYPE),
        token(),
        ordinal,
    );
    feature.edit(|payload| payload.header.source_index = source_index);
    feature
}

fn neutral(
    feature: &PmDcFeature,
    tag: &str,
    definition: FeatureDefinition,
) -> cadmpeg_ir::features::Feature {
    let ctx_arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &ctx_arena, &policy).expect("context");
    let mut model = crate::feature::native_feature(&ctx, &feature.identity, None, feature.id())
        .expect("feature");
    model.source_tag = Some(tag.to_owned());
    model.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(definition);
    model
}

fn stored(feature: &PmDcFeature, tag: &str) -> cadmpeg_ir::features::Feature {
    neutral(
        feature,
        tag,
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
    )
}

fn wrapper_link(wrapper: &PmDcFeatureProperty, link: &PmDcKernelEntityLink) -> WrapperLink {
    WrapperLink {
        id: format!("body-wrapper-link#{}", wrapper.identity.record_ordinal),
        wrapper: wrapper.id(),
        link: link.id(),
        reference: PmDcReference::new(1, false).expect("reference"),
    }
}

#[test]
fn the_join_resolves_split_and_base_body_keys_through_their_links() {
    let split = source(1, 5, &[]);
    let base = source(2, 5, &[(0, 11), (1, 12)]);
    let native_base = source(3, 6, &[(0, 11), (1, 13)]);
    let plain = source(4, 6, &[]);
    let untagged = source(5, 77, &[]);
    let wrappers = [10, 11, 12, 13].map(wrapper_property);
    let links = [link(20, 9), link(21, 7)];
    let model = [
        neutral(
            &split,
            SPLIT_TAG,
            FeatureDefinition::Operation(FeatureOperation::SplitBody {
                targets: BodySelection::Native(wrappers[0].id()),
                tool: SplitBodyTool::Faces(cadmpeg_ir::features::FaceSelection::Native(
                    "tool".to_owned(),
                )),
            }),
        ),
        stored(&base, BASE_TAG),
        neutral(
            &native_base,
            "native",
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Base".to_owned()),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        stored(&plain, "extrude"),
        stored(&untagged, "extrude"),
    ];
    let joins = [
        wrapper_link(&wrappers[0], &links[0]),
        wrapper_link(&wrappers[2], &links[1]),
        wrapper_link(&wrappers[3], &links[1]),
    ];
    let features = [split, base, native_base, plain, untagged];
    let inputs = |wrapper_links| Inputs {
        features: &features,
        model_features: &model,
        kernel_links: &links,
        wrapper_links,
        properties: &wrappers,
    };
    let records = join(&index(), &inputs(&joins));
    let summary: Vec<_> = records
        .iter()
        .map(|record| (record.kind.as_str(), record.binding, record.body_key))
        .collect();
    // Source 5 first appears in state 1, which gains key 7 and loses key 9.
    // Source 6 first appears in state 2, which keeps key 7.
    assert_eq!(
        summary,
        [
            (SPLIT_TAG, Binding::Bound, Some(9)),
            (BASE_TAG, Binding::Bound, Some(7)),
            (
                NATIVE_BASE_KIND,
                unbound(UnboundReason::BodyKeyMismatch),
                Some(7)
            ),
            ("extrude", Binding::Bound, None),
            ("extrude", unbound(UnboundReason::NoTag), None),
        ]
    );
    let carrier = ActiveCarrierRecord::NotApplicable {
        id: "fixture".to_owned(),
    };
    assert!(validate(&records, &features, &carrier, &[])
        .iter()
        .all(|finding| finding.contains("active carrier")));

    // Without a link join the body key is unresolved. A Split or Base is never
    // bound by the movement of a key alone.
    let unjoined = join(&index(), &inputs(&[]));
    assert_eq!(
        unjoined[0].binding,
        unbound(UnboundReason::BodyKeyUnresolved)
    );
    assert_eq!(
        unjoined[1].binding,
        unbound(UnboundReason::BodyKeyUnresolved)
    );

    // A Base whose property vector is not the two-wrapper form has no result wrapper.
    let short = source(2, 5, &[(0, 11)]);
    let features = [short];
    let model = [stored(&features[0], BASE_TAG)];
    let records = join(
        &index(),
        &Inputs {
            features: &features,
            model_features: &model,
            kernel_links: &links,
            wrapper_links: &joins,
            properties: &wrappers,
        },
    );
    assert_eq!(
        records[0].binding,
        unbound(UnboundReason::BodyKeyUnresolved)
    );
}

#[test]
fn a_split_with_a_local_target_binds_through_its_wrapper_link() {
    let split = source(1, 5, &[]);
    let wrappers = [wrapper_property(10)];
    let links = [link(20, 9)];
    let local = BodySelection::local(vec![wrappers[0].id()], wrappers[0].id()).expect("local");
    let model = [neutral(
        &split,
        SPLIT_TAG,
        FeatureDefinition::Operation(FeatureOperation::SplitBody {
            targets: local,
            tool: SplitBodyTool::Faces(cadmpeg_ir::features::FaceSelection::Native(
                "tool".to_owned(),
            )),
        }),
    )];
    let joins = [wrapper_link(&wrappers[0], &links[0])];
    let features = [split];
    let records = join(
        &index(),
        &Inputs {
            features: &features,
            model_features: &model,
            kernel_links: &links,
            wrapper_links: &joins,
            properties: &wrappers,
        },
    );
    // Source 5 first appears in state 1, which loses key 9.
    assert_eq!(records[0].binding, Binding::Bound);
    assert_eq!(records[0].body_key, Some(9));
}

#[test]
fn a_validated_arena_refuses_a_forged_record() {
    let features = [source(1, 5, &[])];
    let model = [stored(&features[0], "extrude")];
    let inputs = Inputs {
        features: &features,
        model_features: &model,
        kernel_links: &[],
        wrapper_links: &[],
        properties: &[],
    };
    let carrier = ActiveCarrierRecord::Unavailable {
        id: "fixture".to_owned(),
        detail: "none".to_owned(),
    };
    let records = join(&index(), &inputs);
    let ok: Vec<_> = validate(&records, &features, &carrier, &[]);
    assert_eq!(ok.len(), 1, "{ok:?}");
    let selected = |records: &[FeatureHistoryState]| {
        validate(records, &features, &carrier, &[])
            .into_iter()
            .filter(|finding| !finding.contains("active carrier"))
            .collect::<Vec<_>>()
    };
    assert!(selected(&records).is_empty());
    let mut wrong_source = records.clone();
    wrong_source[0].source_index = 6;
    assert_eq!(selected(&wrong_source).len(), 1);
    let mut wrong_order = records.clone();
    wrong_order[0].state_before = Some(0);
    wrong_order[0].state_after = Some(StateRef {
        ordinal: 2,
        state_id: 13,
    });
    assert_eq!(selected(&wrong_order).len(), 1);
    let mut repeated = records.clone();
    repeated.push(records[0].clone());
    assert!(selected(&repeated).len() >= 2);
    let mut no_state = records.clone();
    no_state[0].state_after = None;
    no_state[0].state_before = None;
    assert_eq!(selected(&no_state).len(), 1);
}

/// The source indices of a naming tag, without its stored schema.
fn naming_sources(record: &Record) -> Option<Vec<u32>> {
    naming_tag(record).map(|(_, sources)| sources)
}

#[test]
fn the_schema_assumption_counts_retained_states_whether_bound_or_not() {
    let mut mismatched = index();
    mismatched.schema_mismatch = BTreeSet::from([5, 6]);
    let state = StateRef {
        ordinal: 1,
        state_id: 12,
    };
    let mut bound = blank(1);
    bound.source_index = 5;
    bound.state_after = Some(state);
    let mut unresolved = blank(2);
    unresolved.source_index = 6;
    unresolved.state_after = Some(state);
    unresolved.binding = unbound(UnboundReason::BodyKeyUnresolved);
    // A feature with a retained state from a tag of the carrier schema.
    let mut matching = blank(3);
    matching.source_index = 8;
    matching.state_after = Some(state);
    // A listed source with no retained state.
    let mut stateless = blank(4);
    stateless.source_index = 5;
    stateless.state_after = None;
    stateless.binding = unbound(UnboundReason::NoTag);
    assert_eq!(
        schema_assumed(&[bound, unresolved, matching, stateless], &mismatched),
        2
    );
}

#[test]
fn a_binding_through_a_tag_of_another_schema_states_the_assumption() {
    // A tag keeps its stored schema, which classify reports with its sources.
    let tag = tagged(
        "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib",
        &[
            Token::Long(99),
            Token::Long(1),
            Token::Long(7),
            Token::Long(-1),
        ],
    );
    assert_eq!(naming_tag(&tag), Some((99, vec![7])));
    assert_eq!(classify(&tag), Some(Class::Naming(99, vec![7])));
    let notes = losses(&Projection {
        records: Vec::new(),
        unread: None,
        schema_assumed: 2,
        bodies: crate::feature::history_bodies::Built::default(),
    });
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].code.local_code(),
        "source.kernel-history-tag-schema-assumed"
    );
    assert!(notes[0].message.starts_with("2 feature(s)"));
}

#[test]
fn losses_count_unbound_split_and_base_features_and_unread_chains() {
    let mut record = blank(1);
    record.kind = SPLIT_TAG.to_owned();
    record.binding = unbound(UnboundReason::NoTag);
    let mut plain = blank(2);
    plain.kind = "extrude".to_owned();
    plain.binding = unbound(UnboundReason::NoTag);
    let notes = losses(&Projection {
        records: vec![record, plain],
        unread: None,
        schema_assumed: 0,
        bodies: crate::feature::history_bodies::Built::default(),
    });
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].code.local_code(), "feature.history-state-unbound");
    let unread = losses(&Projection {
        records: Vec::new(),
        unread: Some(Unread::Unclosed),
        schema_assumed: 0,
        bodies: crate::feature::history_bodies::Built::default(),
    });
    assert_eq!(
        unread[0].code.local_code(),
        "source.kernel-history-unclosed"
    );
    let bounded = losses(&Projection {
        records: Vec::new(),
        unread: Some(Unread::Bounded),
        schema_assumed: 0,
        bodies: crate::feature::history_bodies::Built::default(),
    });
    assert_eq!(
        bounded[0].code.local_code(),
        "source.kernel-history-bounded"
    );
    assert!(losses(&Projection {
        records: Vec::new(),
        unread: None,
        schema_assumed: 0,
        bodies: crate::feature::history_bodies::Built::default(),
    })
    .is_empty());
}
