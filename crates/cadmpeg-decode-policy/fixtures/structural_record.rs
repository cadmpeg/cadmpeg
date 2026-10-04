// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::structural::project;
use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
use std::sync::LazyLock;

pub struct FixedRecord {
    major: u32,
    minor: Option<u8>,
}

impl Serialize for FixedRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("FixedRecord", 2)?;
        record.serialize_field("major", &self.major)?;
        record.serialize_field("minor", &self.minor)?;
        record.end()
    }
}

pub fn fixed_record_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &FixedRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "fixed structural record")?);
    Ok(())
}

pub fn ordered_major_radius_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::OrderedMajorRadius,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "ordered major radius record")?);
    Ok(())
}

pub struct ScanningRecord {
    text: String,
}

impl Serialize for ScanningRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let length = self.text.chars().count();
        let mut record = serializer.serialize_struct("ScanningRecord", 1)?;
        record.serialize_field("length", &length)?;
        record.end()
    }
}

pub fn scan_inside_record_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &ScanningRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "scanning structural record")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct AllocatingRecord {
    text: String,
}

impl Serialize for AllocatingRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let owned = self.text.clone();
        let mut record = serializer.serialize_struct("AllocatingRecord", 1)?;
        record.serialize_field("text", &owned)?;
        record.end()
    }
}

pub fn allocation_inside_record_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &AllocatingRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "allocating structural record")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DynamicCountRecord {
    count: usize,
    value: u8,
}

impl Serialize for DynamicCountRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("DynamicCountRecord", self.count)?;
        record.serialize_field("value", &self.value)?;
        record.end()
    }
}

pub fn dynamic_field_count_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &DynamicCountRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "dynamic record count")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct DynamicKeyRecord {
    key: &'static str,
    value: u8,
}

impl Serialize for DynamicKeyRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("DynamicKeyRecord", 1)?;
        record.serialize_field(self.key, &self.value)?;
        record.end()
    }
}

pub fn dynamic_field_name_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &DynamicKeyRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "dynamic record field name")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct TwoStateRecord {
    value: u8,
}

impl Serialize for TwoStateRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // This fixture is compiled without executing the duplicated state.
        let alternate = unsafe { std::ptr::read(&serializer) };
        let mut first = serializer.serialize_struct("TwoStateRecord", 1)?;
        let second = alternate.serialize_struct("OtherState", 1)?;
        first.serialize_field("value", &self.value)?;
        second.end()
    }
}

pub fn two_serializer_states_are_unproven(
    ctx: &DecodeContext<'_>,
    source: &TwoStateRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "two serializer states")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct SwallowedFieldError {
    value: u8,
}

impl Serialize for SwallowedFieldError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("SwallowedFieldError", 1)?;
        let _field_result = record.serialize_field("value", &self.value);
        record.end()
    }
}

pub fn swallowed_field_error_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &SwallowedFieldError,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "swallowed record field error")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct ChangedEndResult {
    value: u8,
}

impl Serialize for ChangedEndResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("ChangedEndResult", 1)?;
        record.serialize_field("value", &self.value)?;
        record.end().map_err(|_error| <S::Error as serde::ser::Error>::custom("changed end error"))
    }
}

pub fn changed_end_result_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &ChangedEndResult,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "changed record end result")?); // finding: unproven_decode_charge
    Ok(())
}

static EXTERNAL_TEXT: LazyLock<String> = LazyLock::new(|| "runtime external text".to_owned());

pub struct ExternalRecord;

impl Serialize for ExternalRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("ExternalRecord", 1)?;
        record.serialize_field("external", EXTERNAL_TEXT.as_str())?;
        record.end()
    }
}

pub fn external_lazy_value_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &ExternalRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "external lazy record field")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct CallbackRecord {
    value: Option<u8>,
    offset: u8,
}

impl Serialize for CallbackRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let adjusted = self.value.map(|value| value.wrapping_add(self.offset));
        let mut record = serializer.serialize_struct("CallbackRecord", 1)?;
        record.serialize_field("value", &adjusted)?;
        record.end()
    }
}

pub fn callback_field_is_unproven(
    ctx: &DecodeContext<'_>,
    source: &CallbackRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "callback structural field")?); // finding: unproven_decode_charge
    Ok(())
}

pub struct GenericRecord<T> {
    value: T,
}

impl<T: Serialize> Serialize for GenericRecord<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut record = serializer.serialize_struct("GenericRecord", 1)?;
        record.serialize_field("value", &self.value)?;
        record.end()
    }
}

pub fn concrete_generic_record_is_bounded(
    ctx: &DecodeContext<'_>,
    source: &GenericRecord<Vec<String>>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "concrete generic record")?);
    Ok(())
}

pub fn generic_record_keeps_child_obligations(
    ctx: &DecodeContext<'_>,
    source: &GenericRecord<ScanningRecord>,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "generic record scanning child")?); // finding: unproven_decode_charge
    Ok(())
}
