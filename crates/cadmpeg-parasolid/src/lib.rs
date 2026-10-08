// SPDX-License-Identifier: Apache-2.0
//! Shared Parasolid stream identity and header primitives.
//!
//! Parasolid is an embedded modelling-kernel layer in both NX and SLDPRT.
//! This crate owns the `parasolid:` dialect rows and the schema-token grammar
//! so hosts cannot disagree about the identity of the same declaration.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
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
pub fn find_schema_token<'a>(
    ctx: &DecodeContext<'_>,
    prologue: &'a [u8],
) -> Result<Option<SchemaToken<'a>>, CodecError> {
    ctx.find_map(
        prologue.windows(SCHEMA_MARKER.len()).enumerate(),
        |(offset, window)| {
            if window != SCHEMA_MARKER {
                return Ok(None);
            }
            let body = &prologue[offset + SCHEMA_MARKER.len()..];
            let body_len = ctx
                .position_by(
                    body,
                    |byte| Ok(!is_token_byte(*byte)),
                    "Parasolid schema token extent",
                )?
                .unwrap_or(body.len());
            // The extent search proved every byte a token byte, so the token
            // is complete exactly when it has a byte after the marker.
            if body_len == 0 {
                return Ok(None);
            }
            let end = offset + SCHEMA_MARKER.len() + body_len;
            Ok(ctx
                .validate_utf8(&prologue[offset..end], "Parasolid schema token UTF-8")?
                .ok()
                .map(|value| SchemaToken { value, offset }))
        },
        "Parasolid schema marker search",
    )
}

/// Find a complete schema token whose byte length immediately precedes it.
/// The declared length bounds the SLDPRT token before its first record.
pub fn find_u8_length_prefixed_schema_token<'a>(
    ctx: &DecodeContext<'_>,
    prologue: &'a [u8],
) -> Result<Option<SchemaToken<'a>>, CodecError> {
    ctx.find_map(
        prologue.windows(SCHEMA_MARKER.len()).enumerate(),
        |(offset, window)| {
            if window != SCHEMA_MARKER {
                return Ok(None);
            }
            let Some(prefix) = offset.checked_sub(1).and_then(|index| prologue.get(index)) else {
                return Ok(None);
            };
            schema_token(ctx, prologue, offset, offset + usize::from(*prefix))
        },
        "Parasolid prefixed schema marker search",
    )
}

/// The fixed four-byte marker that opens every schema token.
const SCHEMA_MARKER: &[u8; 4] = b"SCH_";

const fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn schema_token<'a>(
    ctx: &DecodeContext<'_>,
    prologue: &'a [u8],
    offset: usize,
    end: usize,
) -> Result<Option<SchemaToken<'a>>, CodecError> {
    let Some(bytes) = prologue.get(offset..end) else {
        return Ok(None);
    };
    if !is_schema_token(ctx, bytes)? {
        return Ok(None);
    }
    let Ok(value) = ctx.validate_utf8(bytes, "Parasolid schema token UTF-8")? else {
        return Ok(None);
    };
    Ok(Some(SchemaToken { value, offset }))
}

/// The shared token grammar: `SCH_` and at least one more token byte.
fn is_schema_token(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<bool, CodecError> {
    Ok(bytes.len() > SCHEMA_MARKER.len()
        && bytes.starts_with(SCHEMA_MARKER)
        && ctx.all_by(
            bytes,
            |byte| Ok(is_token_byte(*byte)),
            "Parasolid schema token grammar",
        )?)
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

impl SchemaToken<'_> {
    /// Retain this validated token under the caller's context.
    pub fn into_owned(self, ctx: &DecodeContext<'_>) -> Result<OwnedSchemaToken, CodecError> {
        Ok(OwnedSchemaToken(ctx.copy_retained_text(
            self.value,
            "Parasolid schema token",
        )?))
    }
}

impl OwnedSchemaToken {
    /// Validate owned token text under the caller's context.
    pub fn parse(
        ctx: &DecodeContext<'_>,
        value: String,
    ) -> Result<Result<Self, NotSchemaToken>, CodecError> {
        Ok(if is_schema_token(ctx, value.as_bytes())? {
            Ok(Self(value))
        } else {
            Err(NotSchemaToken)
        })
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
fn schema_row(ctx: &DecodeContext<'_>, schema: &str) -> Result<DialectId, CodecError> {
    Ok(
        if ctx.eq_ignore_ascii_case(
            schema,
            "SCH_SW_33103_11000",
            "Parasolid schema row comparison",
        )? {
            PARASOLID_SCH_SW_33103
        } else if ctx.eq_ignore_ascii_case(
            schema,
            "SCH_SW_32001_11000",
            "Parasolid schema row comparison",
        )? {
            PARASOLID_SCH_SW_32001
        } else if let Some((_, suffix)) =
            ctx.rsplit_once(schema, "_", "Parasolid schema format suffix")?
        {
            if ctx.eq_ignore_ascii_case(suffix, "13006", "Parasolid schema suffix comparison")? {
                PARASOLID_FORMAT_13006
            } else {
                PARASOLID_UNKNOWN
            }
        } else {
            PARASOLID_UNKNOWN
        },
    )
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
    let id = schema_row(ctx, schema.value())?;
    let mut declared = BTreeMap::new();
    let schema_key = cadmpeg_core::nonblank_const!("schema");
    ctx.insert_btree_map(
        &mut declared,
        schema_key,
        schema.0,
        "collect Parasolid declarations",
    )?;
    let carrier_key = cadmpeg_core::nonblank_const!("carrier");
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
    let matched = if ctx.any_by(
        verified,
        |candidate| {
            ctx.equal(
                candidate.as_str(),
                id.as_str(),
                "compare Parasolid verified row",
            )
        },
        "scan Parasolid verified rows",
    )? {
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
    let mut layers = ctx.collection_vec(streams.len(), "collect Parasolid classified layers")?;
    for (schema, carrier) in ctx.admit_iter(streams, "scan Parasolid schema carriers")? {
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
    extras: Vec<ClassifiedLayer>,
) -> Result<Vec<String>, cadmpeg_core::CodecError> {
    let mut collisions = Vec::new();
    for layer in ctx.admit_iter(extras, "scan Parasolid extra layers")? {
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
                return Err(CodecError::ResourceLimit(limit))
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

    // A Parasolid match declares exactly the schema and carrier keys
    // `classify_layer` inserts, so each lookup compares fixed keys.
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
        OwnedSchemaToken::parse(&cadmpeg_test_support::service_decode_context(), text.into())
            .expect("service token admission")
            .expect("the fixture text is a schema token")
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
        // One backing node and nine carrier bytes precede the tagged instance copy.
        let node_bytes = 11
            * (std::mem::size_of::<cadmpeg_core::text::NonBlankString>()
                + std::mem::size_of::<String>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<String>();
        policy.limits.max_retained_bytes = 9 + cadmpeg_core::decode::u64_from_index(node_bytes);
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
        let error = push_extras(&ctx, &mut layers, vec![later.clone()])
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
        let token = find_schema_token(
            &cadmpeg_test_support::service_decode_context(),
            b"prologue\0SCH_3501171_35102_13006\0body",
        )
        .expect("service token search")
        .expect("complete token");
        assert_eq!(token.value(), "SCH_3501171_35102_13006");
        assert_eq!(token.offset, 9);
        assert_eq!(token.end(), 32);

        assert!(
            find_schema_token(&cadmpeg_test_support::service_decode_context(), b"SCH_")
                .expect("service token search")
                .is_none()
        );
        assert_eq!(
            find_schema_token(
                &cadmpeg_test_support::service_decode_context(),
                b"SCH_-SCH_REAL"
            )
            .expect("service token search")
            .expect("the first complete token is selected")
            .value(),
            "SCH_REAL"
        );
        assert_eq!(
            find_schema_token(
                &cadmpeg_test_support::service_decode_context(),
                b"SCH_TEST-ignored"
            )
            .expect("service token search")
            .expect("token before delimiter")
            .value(),
            "SCH_TEST"
        );

        let mut prefixed = b"padding".to_vec();
        prefixed.push(8);
        prefixed.extend_from_slice(b"SCH_TEST");
        prefixed.extend_from_slice(b"1234");
        let token = find_u8_length_prefixed_schema_token(
            &cadmpeg_test_support::service_decode_context(),
            &prefixed,
        )
        .expect("service token search")
        .expect("the declared length bounds the token");
        assert_eq!(token.value(), "SCH_TEST");
        assert_eq!(token.end(), 16);
    }

    #[test]
    fn schema_token_searches_pay_only_for_the_bytes_they_visit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut prologue = b"SCH_TEST\0".to_vec();
        prologue.resize(1 << 20, 0);
        let mut prefixed = b"\x08SCH_TEST".to_vec();
        prefixed.resize(1 << 20, 0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert_eq!(
            find_schema_token(&ctx, &prologue)
                .expect("an early token fits the budget")
                .expect("token")
                .value(),
            "SCH_TEST"
        );
        assert_eq!(
            find_u8_length_prefixed_schema_token(&ctx, &prefixed)
                .expect("an early token fits the budget")
                .expect("token")
                .value(),
            "SCH_TEST"
        );
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
            push_extras(&ctx, &mut layers, vec![first.clone(), later]).expect("layers fit policy");

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

    #[test]
    fn schema_token_discovery_propagates_resource_refusals() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        assert!(matches!(find_schema_token(&ctx, b"SCH_TEST"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
        assert!(
            matches!(find_u8_length_prefixed_schema_token(&ctx, b"\x08SCH_TEST"),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
        );
        assert!(matches!(OwnedSchemaToken::parse(&ctx, "SCH_TEST".into()),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
        policy.limits.max_work_units = DecodePolicy::service().limits.max_work_units;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let token = find_schema_token(&ctx, b"SCH_TEST")
            .expect("scan admission")
            .expect("token");
        assert!(matches!(token.into_owned(&ctx),
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
    }
}
