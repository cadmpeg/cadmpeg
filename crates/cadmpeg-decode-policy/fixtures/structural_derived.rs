// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::check::Finding;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::PositiveI64;
use cadmpeg_ir::schema::structural::project;
use serde::ser::{SerializeSeq, Serializer};
use serde::Serialize;

pub fn imported_unit_enum_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &Severity,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "imported enum derive")?);
    Ok(())
}

pub fn imported_skipped_field_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &Finding,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "imported field derive")?);
    Ok(())
}

pub fn imported_fixed_into_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &PositiveI64,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "imported fixed Into derive")?);
    Ok(())
}

#[derive(Serialize)]
pub struct LocalFixedSource<'a> {
    label: &'a str,
    level: Severity,
}

pub fn local_derived_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &LocalFixedSource<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "local fixed derive")?);
    Ok(())
}

#[derive(Serialize)]
pub struct TupleStructSource(pub u8, pub u16);

#[derive(Serialize)]
pub enum TupleVariantSource {
    Pair(u8, u16),
}

#[derive(Serialize)]
pub struct GenericNewtypeSource<T>(pub T);

pub fn tuple_and_generic_newtype_derives_are_bounded(
    ctx: &DecodeContext<'_>,
    tuple: &TupleStructSource,
    variant: &TupleVariantSource,
    newtype: &GenericNewtypeSource<Option<u8>>,
) -> Result<(), CodecError> {
    drop(project(ctx, tuple, "tuple struct derive")?);
    drop(project(ctx, variant, "tuple variant derive")?);
    drop(project(ctx, newtype, "generic newtype derive")?);
    Ok(())
}

#[derive(Serialize)]
pub struct TaggedStructPayload {
    label: &'static str,
    level: Severity,
}

#[derive(Serialize)]
pub enum TaggedExternalPayload {
    Level(Severity),
    Unit,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
pub enum TaggedNewtypeSource {
    Struct(TaggedStructPayload),
    External(TaggedExternalPayload),
}

pub fn tagged_newtype_and_payload_are_bounded(
    ctx: &DecodeContext<'_>,
    source: &TaggedNewtypeSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "tagged newtype derive")?);
    Ok(())
}

pub struct MarkedScanningSource {
    text: String,
}

#[automatically_derived]
impl Serialize for MarkedScanningSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.text.len()))?;
        for character in self.text.chars() {
            sequence.serialize_element(&character)?;
        }
        sequence.end()
    }
}

pub fn marked_scan_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &MarkedScanningSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "marked source scan")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct MarkedAllocatingSource {
    text: String,
}

#[automatically_derived]
impl Serialize for MarkedAllocatingSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let owned = self.text.clone();
        owned.serialize(serializer)
    }
}

pub fn marked_allocation_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &MarkedAllocatingSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "marked source allocation")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct MarkedSwallowedRefusalSource {
    level: Severity,
}

#[automatically_derived]
impl Serialize for MarkedSwallowedRefusalSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // This fixture is compiled without executing the duplicated serializer.
        let alternate = unsafe { std::ptr::read(&serializer) };
        let _ = serializer.serialize_some(&self.level);
        alternate.serialize_unit()
    }
}

pub fn marked_swallowed_refusal_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &MarkedSwallowedRefusalSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "marked swallowed refusal")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct MarkedExternalSource {
    text: std::sync::LazyLock<String>,
}

#[automatically_derived]
impl Serialize for MarkedExternalSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.text.as_str().serialize(serializer)
    }
}

pub fn marked_external_value_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &MarkedExternalSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "marked external value")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct MarkedZeroEmissionSource;

#[automatically_derived]
impl Serialize for MarkedZeroEmissionSource {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialize(serializer)
    }
}

pub fn marked_zero_emission_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &MarkedZeroEmissionSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "marked zero emission")?); // finding: unproven_decode_charge
    Ok(())
}

#[derive(Serialize)]
#[serde(transparent)]
pub struct TransparentRecursiveSource<'a>(&'a TransparentRecursiveSource<'a>);

pub fn zero_emission_cycle_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &TransparentRecursiveSource<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "zero emission cycle")?); // finding: unproven_decode_charge
    Ok(())
}

#[derive(Serialize)]
pub struct FlattenedSiblingCycle<'a> {
    sibling: u8,
    #[serde(flatten)]
    recursive: &'a FlattenedSiblingCycle<'a>,
}

pub fn flattened_sibling_does_not_credit_recursive_child(
    ctx: &DecodeContext<'_>,
    source: &FlattenedSiblingCycle<'_>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "flattened sibling cycle")?); // finding: unproven_decode_charge
    Ok(())
}

#[derive(Serialize)]
pub enum TaggedTuplePayload {
    Pair(String, String),
}

#[derive(Serialize)]
#[serde(tag = "kind")]
pub enum TaggedTupleSource {
    Payload(TaggedTuplePayload),
}

pub fn tagged_tuple_variant_buffer_remains_unproven(
    ctx: &DecodeContext<'_>,
    source: &TaggedTupleSource,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "tagged tuple buffer")?); // finding: unproven_decode_charge
    Ok(())
}
