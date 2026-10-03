// SPDX-License-Identifier: Apache-2.0
//! Shared Parasolid stream identity and header primitives.
//!
//! Parasolid is an embedded modelling-kernel layer in both NX and SLDPRT.
//! This crate owns the `parasolid:` dialect rows and the schema-token grammar
//! so hosts cannot disagree about the identity of the same declaration.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch, LayerInstance};
use cadmpeg_core::CodecError;

include!("registry_ids.rs");

/// Declared-key name for the source schema token.
pub const DECLARED_SCHEMA: &str = "schema";
/// Declared-key name for the host location carrying the stream.
pub const DECLARED_CARRIER: &str = "carrier";

/// One exact ASCII `SCH_` token and its location in a supplied prologue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaToken<'a> {
    value: &'a str,
    offset: usize,
}

impl<'a> SchemaToken<'a> {
    /// Exact token text, including the `SCH_` prefix.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.value
    }

    /// Exclusive byte end of the token in the supplied prologue.
    #[must_use]
    pub const fn end(self) -> usize {
        self.offset + self.value.len()
    }
}

/// Find the first complete Parasolid schema token in a bounded prologue.
///
/// The caller owns the carrier-specific bound. The token grammar is shared:
/// `SCH_` followed by one or more ASCII alphanumeric or underscore bytes.
#[must_use]
pub fn find_schema_token(prologue: &[u8]) -> Option<SchemaToken<'_>> {
    prologue
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == b"SCH_")
        .find_map(|(offset, _)| {
            let mut end = offset + 4;
            while end < prologue.len()
                && (prologue[end].is_ascii_alphanumeric() || prologue[end] == b'_')
            {
                end += 1;
            }
            schema_token(prologue, offset, end)
        })
}

/// Find a complete schema token whose byte length immediately precedes it.
///
/// This is the `SLDPRT` embedded-header form. The declared length bounds the
/// token even when the first record begins with an ASCII token character.
#[must_use]
pub fn find_u8_length_prefixed_schema_token(prologue: &[u8]) -> Option<SchemaToken<'_>> {
    prologue
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == b"SCH_")
        .find_map(|(offset, _)| {
            let length = usize::from(*prologue.get(offset.checked_sub(1)?)?);
            let end = offset.checked_add(length)?;
            schema_token(prologue, offset, end)
        })
}

fn schema_token(prologue: &[u8], offset: usize, end: usize) -> Option<SchemaToken<'_>> {
    let bytes = prologue.get(offset..end)?;
    is_schema_token(bytes).then_some(())?;
    let value = std::str::from_utf8(bytes).ok()?;
    Some(SchemaToken { value, offset })
}

/// The shared token grammar: `SCH_` and at least one more token byte.
fn is_schema_token(bytes: &[u8]) -> bool {
    bytes.len() > 4
        && bytes.starts_with(b"SCH_")
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

/// Text offered in the schema position that is not a Parasolid schema token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotSchemaToken;

impl std::fmt::Display for NotSchemaToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("not a Parasolid schema token")
    }
}

impl std::error::Error for NotSchemaToken {}

/// One owned ASCII `SCH_` token, checked against the shared token grammar.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnedSchemaToken(String);

impl OwnedSchemaToken {
    /// Exact token text, including the `SCH_` prefix.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OwnedSchemaToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<SchemaToken<'_>> for OwnedSchemaToken {
    fn from(token: SchemaToken<'_>) -> Self {
        Self(token.value().to_owned())
    }
}

impl TryFrom<&str> for OwnedSchemaToken {
    type Error = NotSchemaToken;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        is_schema_token(value.as_bytes())
            .then(|| Self(value.to_owned()))
            .ok_or(NotSchemaToken)
    }
}

impl TryFrom<String> for OwnedSchemaToken {
    type Error = NotSchemaToken;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        is_schema_token(value.as_bytes())
            .then_some(())
            .ok_or(NotSchemaToken)?;
        Ok(Self(value))
    }
}

/// The host location a Parasolid stream was read from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Carrier(String);

impl Carrier {
    /// Names the host location carrying one Parasolid stream.
    #[must_use]
    pub const fn new(location: String) -> Self {
        Self(location)
    }

    /// The location text, as the report records it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Carrier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Registry row named by one schema token, or the residual row.
///
/// Identity comes from this shared schema-token map so hosts cannot disagree
/// about the identity of the same declaration.
fn schema_row(schema: &str) -> DialectId {
    if schema.eq_ignore_ascii_case("SCH_SW_33103_11000") {
        PARASOLID_SCH_SW_33103
    } else if schema.eq_ignore_ascii_case("SCH_SW_32001_11000") {
        PARASOLID_SCH_SW_32001
    } else if schema
        .rsplit_once('_')
        .is_some_and(|(_, suffix)| suffix.eq_ignore_ascii_case("13006"))
    {
        PARASOLID_FORMAT_13006
    } else {
        PARASOLID_UNKNOWN
    }
}

/// Classify one schema-bearing Parasolid stream and record its host carrier.
///
/// `instance` identifies the carrier when the host contains more than one
/// Parasolid stream. The schema and carrier are always retained verbatim as
/// declarations, independent of whether the schema has a named registry row.
/// `verified` lists the rows whose grammar the host applied and verified; every
/// other row, and the residual row, is admitted without verification.
pub fn classify_layer(
    ctx: &DecodeContext<'_>,
    schema: OwnedSchemaToken,
    carrier: Carrier,
    instance: LayerInstance,
    verified: &[DialectId],
) -> Result<ClassifiedLayer, CodecError> {
    let work = u64_from_index(schema.value().len())
        .checked_mul(3)
        .and_then(|work| work.checked_add(u64_from_index(verified.len())))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("Parasolid classification work", u64::MAX, u64::MAX)
        })?;
    ctx.charge_work(work, "classify Parasolid schema")?;
    let id = schema_row(schema.value());
    let mut declared = BTreeMap::new();
    let schema_key =
        ctx.format_retained(format_args!("schema"), "retain Parasolid declaration key")?;
    let schema_key =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, schema_key, "validate nonblank text")?
            .ok_or_else(|| CodecError::malformed("empty Parasolid schema declaration key"))?;
    ctx.insert_btree_map(
        &mut declared,
        schema_key,
        schema.0,
        "collect Parasolid declarations",
    )?;
    let carrier_key =
        ctx.format_retained(format_args!("carrier"), "retain Parasolid declaration key")?;
    let carrier_key =
        cadmpeg_core::text::NonBlankString::for_decode(ctx, carrier_key, "validate nonblank text")?
            .ok_or_else(|| CodecError::malformed("empty Parasolid carrier declaration key"))?;
    let carrier_text = ctx.format_retained(
        format_args!("{carrier}"),
        "retain Parasolid carrier declaration",
    )?;
    ctx.insert_btree_map(
        &mut declared,
        carrier_key,
        carrier_text,
        "collect Parasolid declarations",
    )?;
    let matched = if verified.contains(&id) {
        DialectMatch::admitted(id)
    } else {
        DialectMatch::residual(id)
    }
    .with_declared(declared);
    let matched = match instance {
        LayerInstance::Sole => matched,
        LayerInstance::Tagged => matched.with_instance(
            ctx.format_retained(format_args!("{carrier}"), "retain Parasolid layer instance")?,
        ),
    };
    Ok(ClassifiedLayer { matched, carrier })
}

/// One classified Parasolid layer and the carrier it was read from.
///
/// A sole layer carries no instance tag, so the carrier is not recoverable from
/// the match alone; it is a field here rather than a lookup or a placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedLayer {
    matched: DialectMatch,
    carrier: Carrier,
}

impl ClassifiedLayer {
    /// The dialect match this layer contributes.
    #[must_use]
    pub const fn matched(&self) -> &DialectMatch {
        &self.matched
    }

    /// The host location the classified stream was read from.
    #[must_use]
    pub const fn carrier(&self) -> &Carrier {
        &self.carrier
    }

    /// Consumes the layer, yielding the match alone.
    #[must_use]
    pub fn into_matched(self) -> DialectMatch {
        self.matched
    }
}

/// Classify every Parasolid carrier in one host document.
///
/// A lone layer needs no instance. Several layers use their carrier paths as
/// stable instance keys, so hosts cannot disagree about when identity needs a
/// disambiguator.
pub fn extra_layers(
    ctx: &DecodeContext<'_>,
    streams: Vec<(OwnedSchemaToken, Carrier)>,
    verified: &[DialectId],
) -> Result<Vec<ClassifiedLayer>, CodecError> {
    let instance = if streams.len() > 1 {
        LayerInstance::Tagged
    } else {
        LayerInstance::Sole
    };
    ctx.charge_work(
        u64_from_index(streams.len()),
        "scan Parasolid schema carriers",
    )?;
    let mut layers = ctx.collection_vec(streams.len(), "collect Parasolid classified layers")?;
    for (schema, carrier) in streams {
        layers.push(classify_layer(ctx, schema, carrier, instance, verified)?);
    }
    Ok(layers)
}

/// Adds classified Parasolid layers and reports every uniqueness collision.
///
/// Hosts own their loss-code vocabulary. This helper owns the shared layer-set
/// operation and its explanation so NX and SLDPRT cannot describe the same
/// Parasolid collision differently.
pub fn push_extras(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    layers: &mut DialectLayers,
    extras: impl IntoIterator<Item = ClassifiedLayer>,
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let mut collisions = Vec::new();
    for layer in extras {
        let ClassifiedLayer { matched, carrier } = layer;
        match layers.insert_for_decode(ctx, matched, "collect Parasolid dialect layers") {
            Ok(()) => {}
            Err(cadmpeg_core::dialect::DialectLayerError::Duplicate(rejected)) => {
                let message = ctx.format_retained(format_args!(
                    "the container produced a duplicate {} dialect layer at carrier {carrier}; the later classification was omitted",
                    rejected.format()
                ), "retain Parasolid collision message")?;
                ctx.push_vec(
                    &mut collisions,
                    message,
                    "collect Parasolid collision messages",
                )?;
            }
            Err(cadmpeg_core::dialect::DialectLayerError::ResourceLimit(limit)) => {
                return Err(limit.into())
            }
        }
    }
    Ok(collisions)
}

/// Explain why a Parasolid layer was admitted without verification.
///
/// Host codecs own their loss vocabulary. This helper owns the interpretation
/// of the declarations produced by [`classify_layer`], so every host wraps the
/// same kernel fact in its codec-specific loss code.
pub fn unverified_message(
    ctx: &DecodeContext<'_>,
    matched: &DialectMatch,
) -> Result<Option<String>, CodecError> {
    if matched.format() != FORMAT
        || !matches!(
            matched.admission(),
            Admission::Unverified { .. } | Admission::Residual
        )
    {
        return Ok(None);
    }

    let schema = matched
        .declared()
        .get(DECLARED_SCHEMA)
        .map_or("<unrecorded>", String::as_str);
    let carrier = matched
        .declared()
        .get(DECLARED_CARRIER)
        .map_or("<unrecorded>", String::as_str);
    if matched.dialect() == &PARASOLID_UNKNOWN {
        return Ok(Some(ctx.format_retained(
            format_args!(
            "The Parasolid stream at {carrier} declares schema {schema:?}, which has no declared \
             grammar. It was admitted as the `{}` residual layer without substituting another \
             schema grammar; bounded structural recovery retains the source stream.",
            matched.dialect()
        ),
            "retain Parasolid unverified message",
        )?));
    }
    Ok(Some(ctx.format_retained(
        format_args!(
        "The Parasolid stream at {carrier} declares schema {schema:?}, which maps to the named \
         `{}` row, but the host did not verify that row's schema grammar. It was admitted \
         without substituting another schema grammar; bounded structural recovery retains the \
         source stream.",
        matched.dialect()
    ),
        "retain Parasolid unverified message",
    )?))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch, LayerInstance};

    use super::{
        classify_layer, extra_layers, find_schema_token, find_u8_length_prefixed_schema_token,
        push_extras, unverified_message, Carrier, OwnedSchemaToken, DECLARED_CARRIER,
        DECLARED_SCHEMA, FORMAT, PARASOLID_FORMAT_13006, PARASOLID_SCH_SW_32001,
        PARASOLID_SCH_SW_33103,
    };

    const ALL_ROWS: [DialectId; 3] = [
        PARASOLID_SCH_SW_33103,
        PARASOLID_SCH_SW_32001,
        PARASOLID_FORMAT_13006,
    ];

    fn token(text: &str) -> OwnedSchemaToken {
        OwnedSchemaToken::try_from(text).expect("the fixture text is a schema token")
    }

    fn carrier(text: &str) -> Carrier {
        Carrier::new(text.to_owned())
    }

    #[test]
    fn parasolid_classification_admits_copies_and_declarations() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

        for dimension in [
            ResourceDimension::RetainedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let error = classify_layer(
                &ctx,
                token("SCH_TEST"),
                carrier("stream@12"),
                LayerInstance::Sole,
                &[],
            )
            .expect_err("classification uses caller limits");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension)
            );
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two declaration keys share one backing node; their copies use 13 + 9 text bytes.
        let node_bytes = 11
            * (std::mem::size_of::<cadmpeg_core::text::NonBlankString>()
                + std::mem::size_of::<String>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<String>();
        policy.limits.max_retained_bytes =
            13 + 9 + cadmpeg_core::decode::u64_from_index(node_bytes);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = classify_layer(
            &ctx,
            token("SCH_TEST"),
            carrier("stream@12"),
            LayerInstance::Tagged,
            &[],
        )
        .expect_err("instance copy has no remaining bytes");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "retain Parasolid layer instance"));
    }

    #[test]
    fn parasolid_extra_layers_admit_output_vector_storage() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = extra_layers(&ctx, vec![(token("SCH_TEST"), carrier("stream@12"))], &[])
            .expect_err("classified vector needs retained storage");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "collect Parasolid classified layers"));
    }

    #[test]
    fn parasolid_diagnostic_copies_use_the_caller_context() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let service = cadmpeg_test_support::service_decode_context();
        let first = classify_layer(
            &service,
            token("SCH_TEST"),
            carrier("stream@12"),
            LayerInstance::Tagged,
            &[],
        )
        .expect("service context admits the first classified layer");
        let later = first.clone();
        let mut layers = DialectLayers::of(first.into_matched());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let error = push_extras(&ctx, &mut layers, [later.clone()])
            .expect_err("collision message needs bytes");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "retain Parasolid collision message"));
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fresh empty root");
        let error =
            unverified_message(&ctx, later.matched()).expect_err("unverified message needs bytes");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "retain Parasolid unverified message"));
    }

    #[test]
    fn schema_token_uses_one_exact_ascii_grammar() {
        let token =
            find_schema_token(b"prologue\0SCH_3501171_35102_13006\0body").expect("complete token");
        assert_eq!(token.value(), "SCH_3501171_35102_13006");
        assert_eq!(token.offset, 9);
        assert_eq!(token.end(), 32);

        assert!(find_schema_token(b"SCH_").is_none());
        assert_eq!(
            find_schema_token(b"SCH_-SCH_REAL")
                .expect("the first complete token is selected")
                .value(),
            "SCH_REAL"
        );
        assert_eq!(
            find_schema_token(b"SCH_TEST-ignored")
                .expect("token before delimiter")
                .value(),
            "SCH_TEST"
        );

        let mut prefixed = b"padding".to_vec();
        prefixed.push(8);
        prefixed.extend_from_slice(b"SCH_TEST");
        prefixed.extend_from_slice(b"1234");
        let token = find_u8_length_prefixed_schema_token(&prefixed)
            .expect("the declared length bounds the token");
        assert_eq!(token.value(), "SCH_TEST");
        assert_eq!(token.end(), 16);
    }

    #[test]
    fn named_schemas_and_the_format_suffix_map_to_their_rows_case_insensitively() {
        let ctx = cadmpeg_test_support::service_decode_context();
        for (schema, expected) in [
            ("SCH_sw_33103_11000", "parasolid:sch-sw-33103"),
            ("SCH_Sw_32001_11000", "parasolid:sch-sw-32001"),
            ("SCH_3201255_32001_13006", "parasolid:format-13006"),
        ] {
            let matched = classify_layer(
                &ctx,
                token(schema),
                carrier("stream@12"),
                LayerInstance::Sole,
                &ALL_ROWS,
            )
            .expect("classification fits policy");
            assert_eq!(matched.matched().dialect().as_str(), expected);
            assert_eq!(matched.matched().admission(), &Admission::Admitted);
            assert_eq!(matched.matched().declared()[DECLARED_SCHEMA], schema);
            assert_eq!(matched.matched().declared()[DECLARED_CARRIER], "stream@12");
            assert_eq!(matched.matched().instance(), None);
        }
    }

    #[test]
    fn residual_schemas_use_residual_admission_without_a_substitution() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let matched = classify_layer(
            &ctx,
            token("SCH_TEST_1_9999"),
            carrier("block@7:body+3"),
            LayerInstance::Tagged,
            &ALL_ROWS,
        )
        .expect("classification fits policy");

        assert_eq!(matched.matched().dialect().as_str(), "parasolid:unknown");
        assert_eq!(matched.matched().admission(), &Admission::Residual);
        assert_eq!(
            matched.matched().declared()[DECLARED_SCHEMA],
            "SCH_TEST_1_9999"
        );
        assert_eq!(
            matched.matched().declared()[DECLARED_CARRIER],
            "block@7:body+3"
        );
        assert_eq!(matched.matched().instance(), Some("block@7:body+3"));
        let message = unverified_message(&ctx, matched.matched())
            .expect("message fits policy")
            .expect("residual layer explains its recovery");
        assert!(message.contains("SCH_TEST_1_9999"));
        assert!(message.contains("block@7:body+3"));
    }

    #[test]
    fn several_layers_receive_carrier_instances() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let layers = extra_layers(
            &ctx,
            vec![
                (token("SCH_SW_33103_11000"), carrier("stream@12")),
                (token("SCH_TEST_1_9999"), carrier("stream@48")),
            ],
            &ALL_ROWS,
        )
        .expect("classification fits policy");
        assert_eq!(layers[0].matched().instance(), Some("stream@12"));
        assert_eq!(layers[1].matched().instance(), Some("stream@48"));

        let one = extra_layers(
            &ctx,
            vec![(token("SCH_SW_33103_11000"), carrier("stream@12"))],
            &ALL_ROWS,
        )
        .expect("classification fits policy");
        assert_eq!(one[0].matched().instance(), None);
    }

    #[test]
    fn push_extras_preserves_the_first_layer_and_reports_later_collisions() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut layers = DialectLayers::of(DialectMatch::admitted(
            DialectId::parse("nx:splmsstr").expect("valid host dialect id"),
        ));
        let first = classify_layer(
            &ctx,
            token("SCH_SW_33103_11000"),
            carrier("stream@12"),
            LayerInstance::Tagged,
            &[],
        )
        .expect("classification fits policy");
        let later = classify_layer(
            &ctx,
            token("SCH_SW_32001_11000"),
            carrier("stream@12"),
            LayerInstance::Tagged,
            &[],
        )
        .expect("classification fits policy");

        let collisions =
            push_extras(&ctx, &mut layers, [first.clone(), later]).expect("layers fit policy");

        assert_eq!(layers.iter().skip(1).collect::<Vec<_>>(), [first.matched()]);
        assert_eq!(
            collisions,
            [
                "the container produced a duplicate parasolid dialect layer at carrier stream@12; \
              the later classification was omitted"
            ]
        );
    }

    #[test]
    fn every_parasolid_registry_row_is_produced() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let ids: BTreeSet<_> = [
            "SCH_SW_33103_11000",
            "SCH_SW_32001_11000",
            "SCH_3501171_35102_13006",
            "SCH_TEST_1_9999",
        ]
        .map(|schema| {
            classify_layer(
                &ctx,
                token(schema),
                carrier("carrier"),
                LayerInstance::Sole,
                &ALL_ROWS,
            )
            .expect("classification fits policy")
            .matched()
            .dialect()
            .to_string()
        })
        .into_iter()
        .collect();
        assert_eq!(
            ids,
            cadmpeg_test_support::registry_ids(FORMAT).expect("identity registry parses")
        );
    }

    #[test]
    fn a_known_row_can_be_identified_without_claiming_host_verification() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let matched = classify_layer(
            &ctx,
            token("SCH_3501171_35102_13006"),
            carrier("stream@12"),
            LayerInstance::Sole,
            &[PARASOLID_SCH_SW_33103],
        )
        .expect("classification fits policy");

        assert_eq!(
            matched.matched().dialect().as_str(),
            "parasolid:format-13006"
        );
        assert_eq!(matched.matched().admission(), &Admission::Residual);
        let message = unverified_message(&ctx, matched.matched())
            .expect("message fits policy")
            .expect("unverified row explains its admission");
        assert!(message.contains("host did not verify"));
        assert!(message.contains("parasolid:format-13006"));
    }
}
