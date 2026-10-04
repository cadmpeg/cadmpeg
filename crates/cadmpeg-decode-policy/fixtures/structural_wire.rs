// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::structural::project;
use serde::ser::Serializer;
use serde::Serialize;

#[derive(Serialize)]
struct BorrowedWire<'a> {
    label: &'a str,
    optional: Option<&'a str>,
}

pub struct BorrowedSource {
    label: String,
    optional: Option<String>,
}

impl Serialize for BorrowedSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BorrowedWire {
            label: self.label.as_str(),
            optional: self.optional.as_ref().map(String::as_str),
        }
        .serialize(serializer)
    }
}

pub fn borrowed_wire_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &BorrowedSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "structural Wire source")?);
    Ok(())
}

pub struct CapturedSource {
    label: Option<String>,
    fallback: String,
}

impl Serialize for CapturedSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fallback = self.fallback.as_str();
        let wire = BorrowedWire {
            label: fallback,
            optional: self.label.as_ref().map(|value| {
                if value.is_empty() {
                    fallback
                } else {
                    value.as_str()
                }
            }),
        };
        wire.serialize(serializer)
    }
}

pub fn captured_callback_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &CapturedSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "captured structural callback")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DetachedSource;

impl Serialize for DetachedSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BorrowedWire {
            label: "fixed external label",
            optional: None,
        }
        .serialize(serializer)
    }
}

pub fn detached_value_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &DetachedSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "fixed structural value")?);
    Ok(())
}

pub struct ProviderSource {
    external: std::sync::LazyLock<String>,
}

impl Serialize for ProviderSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BorrowedWire {
            label: self.external.as_str(),
            optional: None,
        }
        .serialize(serializer)
    }
}

pub fn external_provider_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &ProviderSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "external structural provider")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DiscardedResult {
    label: String,
}

impl Serialize for DiscardedResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire_result = BorrowedWire {
            label: self.label.as_str(),
            optional: None,
        }
        .serialize(serializer)?;
        drop(wire_result);
        Err(<S::Error as serde::ser::Error>::custom(
            "discarded serializer success value",
        ))
    }
}

pub fn discarded_result_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &DiscardedResult,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "discarded structural result")?); // finding: unproven_decode_charge
    Ok(())
}
