// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub struct ScopedReservation;
pub struct JsonParserAdmission<'a> {
    text: &'a str,
    reservation: ScopedReservation,
}
pub struct TypedJsonAdmission<T, S> {
    source: S,
    reservation: ScopedReservation,
    target: std::marker::PhantomData<fn() -> T>,
}
impl DecodeContext {
    pub fn json_parser_admission<'a>(&self, text: &'a str) -> Result<JsonParserAdmission<'a>, ()> {
        Ok(JsonParserAdmission { text, reservation: ScopedReservation })
    }
    pub fn json_validation_admission<'a, T>(&self, text: &'a str) -> Result<TypedJsonAdmission<T, &'a str>, ()> {
        Ok(TypedJsonAdmission { source: text, reservation: ScopedReservation, target: std::marker::PhantomData })
    }
    pub fn json_conversion_admission<T>(&self, source: serde_json::Value) -> Result<TypedJsonAdmission<T, serde_json::Value>, ()> {
        Ok(TypedJsonAdmission { source, reservation: ScopedReservation, target: std::marker::PhantomData })
    }
    pub fn parse_json<T: serde::de::DeserializeOwned>(&self, text: &str, _operation: &str) -> Result<T, ()> {
        serde_json::from_str(text).map_err(|_| ()) // finding: unproven_decode_charge
    }
}
struct PlainJson(serde_json::Value);
impl<'de> serde::Deserialize<'de> for PlainJson {
    fn deserialize<D: serde::Deserializer<'de>>(parser: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(parser).map(Self)
    }
}
#[derive(serde::Deserialize)]
struct Record { items: Vec<String> }
pub fn value_tree(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_parser_admission(text)?;
    drop(serde_json::from_str::<serde_json::Value>(admission.text));
    Ok(())
}
pub fn plain_tree(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_parser_admission(text)?;
    drop(serde_json::from_str::<PlainJson>(admission.text));
    Ok(())
}
pub fn wrong_tree_target(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_parser_admission(text)?;
    drop(serde_json::from_str::<Record>(admission.text)); // finding: unproven_decode_charge
    Ok(())
}
pub fn derived_validation(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_validation_admission::<Record>(text)?;
    drop(serde_json::from_str::<Record>(admission.source));
    Ok(())
}
pub fn wrong_validation_target(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_validation_admission::<Record>(text)?;
    drop(serde_json::from_str::<Vec<Record>>(admission.source)); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_validation_source(ctx: &DecodeContext, text: &str, other: &str) -> Result<(), ()> {
    let _admission = ctx.json_validation_admission::<Record>(text)?;
    drop(serde_json::from_str::<Record>(other)); // finding: unproven_decode_charge
    Ok(())
}
pub fn conversion(ctx: &DecodeContext, source: serde_json::Value) -> Result<(), ()> {
    let admission = ctx.json_conversion_admission::<Record>(source)?;
    drop(serde_json::from_value::<Record>(admission.source));
    Ok(())
}
pub fn wrong_conversion_target(ctx: &DecodeContext, source: serde_json::Value) -> Result<(), ()> {
    let admission = ctx.json_conversion_admission::<Record>(source)?;
    drop(serde_json::from_value::<Vec<Record>>(admission.source)); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_conversion_phase(ctx: &DecodeContext, source: serde_json::Value, text: &str) -> Result<(), ()> {
    let _admission = ctx.json_conversion_admission::<Record>(source)?;
    drop(serde_json::from_str::<Record>(text)); // finding: unproven_decode_charge
    Ok(())
}
pub fn released_json_storage(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_parser_admission(text)?;
    drop(admission.reservation);
    drop(serde_json::from_str::<serde_json::Value>(admission.text)); // finding: unproven_decode_charge
    Ok(())
}
pub fn validation_reuse(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_validation_admission::<Record>(text)?;
    drop(serde_json::from_str::<Record>(admission.source));
    drop(serde_json::from_str::<Record>(admission.source)); // finding: unproven_decode_charge
    Ok(())
}
pub fn released_validation_storage(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    let admission = ctx.json_validation_admission::<Record>(text)?;
    drop(admission.reservation);
    drop(serde_json::from_str::<Record>(admission.source)); // finding: unproven_decode_charge
    Ok(())
}
