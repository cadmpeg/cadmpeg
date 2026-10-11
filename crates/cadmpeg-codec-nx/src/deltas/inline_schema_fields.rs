// SPDX-License-Identifier: Apache-2.0
//! Shared source and native inline-schema payloads.

use super::attdef_state::AttdefState;
use super::precision_state::PrecisionState;
use super::type101_state::Type101State;
use super::type38_state::Type38State;
use super::type70_state::Type70State;
use crate::framing::xmt_reference::NonNullXmt;
use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_ir::units::FiniteVector;
use serde::{Deserialize, Serialize};

/// Body of an inline schema declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub(crate) enum InlineSchemaFields {
    /// Type 12 `BODY` schema header without following instance state.
    BodyHeader,
    /// REGION declaration state.
    Region {
        /// Non-null stream-local declaration identity.
        xmt: NonNullXmt,
        /// Serialized big-endian state word.
        state_word: u32,
        /// Four ordered stream-local XMT references.
        references: [u32; 4],
    },
    /// `ATTDEF_LIST` declaration state.
    AttdefList {
        #[serde(flatten)]
        state: AttdefState,
    },
    /// Type 70 declaration state.
    Type70 {
        #[serde(flatten)]
        state: Type70State,
    },
    /// Type 100 declaration and its precision state.
    Type100 {
        #[serde(flatten)]
        state: PrecisionState,
    },
    /// Type 101 declaration and its schema-bound instance state.
    Type101 {
        #[serde(flatten)]
        state: Type101State,
    },
    /// Type 101 declaration with the compact fixed state.
    Type101Compact,
    /// Type 38 intersection-data declaration state.
    Type38 {
        #[serde(flatten)]
        state: Type38State,
    },
    /// Type 41 term-use declaration state.
    Type41 {
        /// Non-null stream-local term-use reference.
        reference: NonNullXmt,
        /// Eleven finite binary64 state values.
        numeric_values: FiniteVector<11>,
    },
}

impl DecodeCost for InlineSchemaFields {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::BodyHeader | Self::Type101Compact => Ok(1),
            Self::Region {
                xmt,
                state_word,
                references,
            } => (1_u8, u32::from(*xmt), state_word, references).decode_cost(ctx, operation),
            Self::AttdefList { state } => (1_u8, state).decode_cost(ctx, operation),
            Self::Type70 { state } => (1_u8, state).decode_cost(ctx, operation),
            Self::Type100 { state } => (1_u8, state).decode_cost(ctx, operation),
            Self::Type101 { state } => (1_u8, state).decode_cost(ctx, operation),
            Self::Type38 { state } => (1_u8, state).decode_cost(ctx, operation),
            Self::Type41 {
                reference,
                numeric_values,
            } => (1_u8, u32::from(*reference), numeric_values.as_raw()).decode_cost(ctx, operation),
        }
    }
}

/// Schema-bound type-12 `BODY` instance state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum InlineBodyStateFields {
    /// Compact reference form followed by status zero.
    Compact {
        /// Non-null stream-local XMT reference.
        reference: NonNullXmt,
    },
    /// Revision form with a bounded opaque state tail.
    Revision {
        /// Monotonic kernel revision identity.
        node_id: u32,
        /// Eight ordered status-framed XMT references.
        references: [u32; 8],
        /// Exact state bytes following the reference prefix.
        state_bytes: BodyStateBytes,
    },
}

#[cfg(test)]
mod tests {
    use super::InlineSchemaFields;
    use cadmpeg_ir::units::FiniteVector;

    #[test]
    fn schema_equality_cost_counts_stored_fields() {
        use super::{AttdefState, PrecisionState, Type101State, Type38State, Type70State};
        use crate::deltas::type38_state::{IntersectionMarker, Type38StateParts};
        use crate::framing::xmt_reference::NonNullXmt;
        use cadmpeg_core::decode::cost::DecodeCost;
        use cadmpeg_core::decode::ResourceDimension;
        let xmt = |value| NonNullXmt::try_from(value).unwrap();
        let links = [xmt(4), xmt(5)];
        let descending = [xmt(13), xmt(12), xmt(11)];
        let state38 = |numeric_values| {
            Type38State::new(&Type38StateParts {
                xmt: xmt(10),
                node_id: 7,
                leading_references: [2, 3, 4, 5, 6],
                leading_statuses: [1; 5],
                marker: IntersectionMarker::Type2b,
                linked_references: &links,
                state_references: &descending,
                numeric_values,
            })
            .unwrap()
        };
        // Costs count the enum tag and stored scalars, references, and numeric lanes.
        let cases = [
            (InlineSchemaFields::BodyHeader, 1),
            (InlineSchemaFields::Type101Compact, 1),
            (
                InlineSchemaFields::Region {
                    xmt: xmt(10),
                    state_word: 3,
                    references: [2; 4],
                },
                1 + 4 + 4 + 4 * 4,
            ),
            (
                InlineSchemaFields::AttdefList {
                    state: AttdefState::new(10, 4, 2, vec![5, 6, 1, 1]).unwrap(),
                },
                1 + 4 + 4 * 4 + 4 + 4,
            ),
            (
                InlineSchemaFields::Type70 {
                    state: Type70State::new(10, 7, [2; 4], 3, 8).unwrap(),
                },
                1 + 4 + 4 + 4 * 4 + 2 + 4,
            ),
            (
                InlineSchemaFields::Type100 {
                    state: PrecisionState::new(
                        10,
                        [2, 11, 1],
                        [
                            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 1.0,
                        ],
                    )
                    .unwrap(),
                },
                1 + 4 + 3 * 8,
            ),
            (
                InlineSchemaFields::Type101 {
                    state: Type101State::new([2; 4], None, [0, 0, 7], 3).unwrap(),
                },
                1 + 4 * 4 + 1 + 1 + 4 + 8,
            ),
            (
                InlineSchemaFields::Type101 {
                    state: Type101State::new([2; 4], Some(10), [19, 9, 7], 3).unwrap(),
                },
                1 + 4 * 4 + 1 + 4 + 1 + 4 + 8,
            ),
            (
                InlineSchemaFields::Type38 {
                    state: state38(None),
                },
                1 + 4 + 4 + 5 * 4 + 1 + 1 + 2 * 4 + 1,
            ),
            (
                InlineSchemaFields::Type38 {
                    state: state38(FiniteVector::new([0.5; 11])),
                },
                1 + 4 + 4 + 5 * 4 + 1 + 1 + 2 * 4 + 1 + 11 * 8,
            ),
            (
                InlineSchemaFields::Type41 {
                    reference: xmt(10),
                    numeric_values: FiniteVector::new([0.5; 11]).unwrap(),
                },
                1 + 4 + 11 * 8,
            ),
        ];
        for (fields, expected) in cases {
            crate::test_support::with_decode_context(|ctx| {
                assert_eq!(
                    fields.decode_cost(ctx, "NX schema equality cost").unwrap(),
                    expected
                );
                assert!(ctx
                    .equal(&fields, &fields, "NX schema equality cost")
                    .unwrap());
            });
            let error = crate::test_support::resource_refusal_at(
                &[],
                ResourceDimension::WorkUnits,
                "NX schema equality cost",
                |ctx| ctx.equal(&fields, &fields, "NX schema equality cost"),
            );
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == expected)
            );
        }
    }

    #[test]
    fn term_use_wire_preserves_values_and_rejects_nonfinite_construction() {
        let json = r#"{"schema":"type41","reference":86,"numeric_values":[0.5,-0.25,1.0,2.0,3.0,4.0,5.0,6.0,7.0,-0.0,9.0]}"#;
        let fields: InlineSchemaFields = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&fields).unwrap(), json);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FiniteVector::new([value; 11]).is_none());
        }
    }
}

/// Nonempty opaque revision state.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<u8>")]
pub(crate) struct BodyStateBytes(Vec<u8>);

impl Serialize for BodyStateBytes {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl TryFrom<Vec<u8>> for BodyStateBytes {
    type Error = &'static str;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if bytes.is_empty() {
            return Err("state_bytes: require a nonempty revision state");
        }
        Ok(Self(bytes))
    }
}
#[cfg(test)]
std::thread_local! {
    static BODY_STATE_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<BodyStateBytes> for Vec<u8> {
    fn from(bytes: BodyStateBytes) -> Self {
        BODY_STATE_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        bytes.0
    }
}

#[cfg(test)]
mod body_state_tests {
    use super::{BodyStateBytes, InlineBodyStateFields, BODY_STATE_INTO_WIRE_COUNT};
    #[test]
    fn body_wire_rejects_null_compact_references_and_empty_revisions() {
        for json in [
            r#"{"form":"compact","reference":9}"#,
            r#"{"form":"revision","node_id":7,"references":[8,1,2,3,4,5,6,7],"state_bytes":[170,187]}"#,
        ] {
            let state: InlineBodyStateFields = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&state).unwrap(), json);
        }
        for reference in [0, 1] {
            assert!(serde_json::from_value::<InlineBodyStateFields>(
                serde_json::json!({"form":"compact","reference":reference})
            )
            .is_err());
        }
        let error = serde_json::from_str::<InlineBodyStateFields>(
            r#"{"form":"revision","node_id":7,"references":[8,1,2,3,4,5,6,7],"state_bytes":[]}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("state_bytes"));
    }

    #[test]
    fn body_state_bytes_native_limit_refuses_before_owned_wire_conversion() {
        #[derive(serde::Serialize)]
        struct Record<'a> {
            id: &'static str,
            state_bytes: &'a BodyStateBytes,
        }
        let state_bytes = BodyStateBytes::try_from(vec![170, 187]).unwrap();
        assert_eq!(
            serde_json::to_vec(&state_bytes).unwrap(),
            serde_json::to_vec(&Vec::<u8>::from(state_bytes.clone())).unwrap()
        );
        let record = Record {
            id: "nx:deltas:body-state#0",
            state_bytes: &state_bytes,
        };
        BODY_STATE_INTO_WIRE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": "nx:deltas:body-state#0", "state_bytes": [170, 187]}),
        );
        BODY_STATE_INTO_WIRE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
