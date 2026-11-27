// SPDX-License-Identifier: Apache-2.0
//! Reader for the ISO 10303-21 text of the `STEP_DATA` stream.
//!
//! The reader admits the simple entity instances `#n=NAME(arguments);` of the
//! `DATA` section, with the parameter forms the Alibre schema writes: null
//! `$`, omitted `*`, integers, reals, strings, enumerations, instance
//! references, lists, and typed parameters.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Deepest list nesting the reader admits.
const MAX_DEPTH: usize = 16;
/// Most octets of one string parameter.
const MAX_STRING_OCTETS: usize = 32_767;
/// Most parameters, list elements and typed parameters counted together, in
/// one entity instance.
const MAX_INSTANCE_PARAMETERS: usize = 65_536;

/// One parameter of an entity instance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    /// The null value `$` or the omitted value `*`.
    Null,
    /// An integer.
    Int(i64),
    /// A real number.
    Real(f64),
    /// A string without its quotes.
    Str(String),
    /// An enumeration or logical value without its dots.
    Enum(String),
    /// A reference `#n` to another instance.
    Ref(u32),
    /// A parenthesized list.
    List(Vec<Value>),
    /// A typed parameter `NAME(arguments)`.
    Typed(String, Vec<Value>),
}

impl Value {
    pub(crate) fn as_ref(&self) -> Option<u32> {
        match self {
            Self::Ref(id) => Some(*id),
            _ => None,
        }
    }

    pub(crate) fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// An integer or a real as a real.
    pub(crate) fn as_real(&self) -> Option<f64> {
        match self {
            Self::Real(value) => Some(*value),
            // The text of the integer parses to its nearest real, with no cast.
            Self::Int(value) => value.to_string().parse().ok(),
            _ => None,
        }
    }

    pub(crate) fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// A logical value `.T.` or `.F.`.
    pub(crate) fn as_logical(&self) -> Option<bool> {
        match self {
            Self::Enum(word) if word == "T" => Some(true),
            Self::Enum(word) if word == "F" => Some(false),
            _ => None,
        }
    }

    /// The references of a list value.
    pub(crate) fn refs(&self) -> Option<Vec<u32>> {
        self.as_list()?.iter().map(Self::as_ref).collect()
    }
}

/// One entity instance.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Instance {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) args: Vec<Value>,
}

impl Instance {
    pub(crate) fn arg(&self, index: usize) -> Option<&Value> {
        self.args.get(index)
    }
}

/// The instances of a `DATA` section and the schema names of the header.
#[derive(Debug, Default)]
pub(crate) struct Part21 {
    pub(crate) schemas: Vec<String>,
    pub(crate) instances: BTreeMap<u32, Instance>,
}

impl Part21 {
    pub(crate) fn get(&self, id: u32) -> Option<&Instance> {
        self.instances.get(&id)
    }

    /// The instance `id` when it is an entity named `name`.
    pub(crate) fn of(&self, id: u32, name: &str) -> Option<&Instance> {
        self.get(id).filter(|instance| instance.name == name)
    }

    /// Whether the header names the Alibre schema.
    pub(crate) fn is_alibre(&self) -> bool {
        self.schemas.iter().any(|name| name == "ALIBRE_SCHEMA")
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    /// The parameters read since the last instance began.
    parameters: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn bump(&mut self) {
        self.at += 1;
    }

    fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(byte) if byte.is_ascii_whitespace() => self.bump(),
                Some(b'/') if self.bytes.get(self.at + 1) == Some(&b'*') => {
                    self.at += 2;
                    while self.at < self.bytes.len()
                        && !(self.bytes[self.at] == b'*'
                            && self.bytes.get(self.at + 1) == Some(&b'/'))
                    {
                        self.at += 1;
                    }
                    self.at = (self.at + 2).min(self.bytes.len());
                }
                _ => break,
            }
        }
    }

    fn word(&mut self) -> &'a str {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("")
    }

    fn require(&mut self, byte: u8, what: &str) -> Result<(), CodecError> {
        self.skip_space();
        if self.peek() == Some(byte) {
            self.bump();
            Ok(())
        } else {
            Err(CodecError::malformed(format_args!(
                "STEP_DATA expects {what} at byte {}",
                self.at
            )))
        }
    }

    fn string(&mut self) -> Result<String, CodecError> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return Err(CodecError::malformed("STEP_DATA string is not closed")),
                Some(b'\'') => {
                    self.bump();
                    if self.peek() == Some(b'\'') {
                        out.push(b'\'');
                        self.bump();
                    } else {
                        break;
                    }
                }
                Some(byte) => {
                    if out.len() >= MAX_STRING_OCTETS {
                        return Err(CodecError::malformed(format_args!(
                            "STEP_DATA string exceeds {MAX_STRING_OCTETS} octets (MAX_STRING_OCTETS)"
                        )));
                    }
                    out.push(byte);
                    self.bump();
                }
            }
        }
        Ok(out.iter().map(|byte| char::from(*byte)).collect())
    }

    fn number(&mut self) -> Result<Value, CodecError> {
        let start = self.at;
        while self.peek().is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'E' | b'e')
        }) {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).unwrap_or("");
        if text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'+')
        {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Value::Int(value));
            }
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(Value::Real(value)),
            _ => Err(CodecError::malformed(format_args!(
                "STEP_DATA number is not valid at byte {start}"
            ))),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, CodecError> {
        if depth > MAX_DEPTH {
            return Err(CodecError::malformed("STEP_DATA nests lists too deeply"));
        }
        self.parameters += 1;
        if self.parameters > MAX_INSTANCE_PARAMETERS {
            return Err(CodecError::malformed(format_args!(
                "STEP_DATA instance holds more than {MAX_INSTANCE_PARAMETERS} parameters (MAX_INSTANCE_PARAMETERS)"
            )));
        }
        self.skip_space();
        match self.peek() {
            Some(b'$' | b'*') => {
                self.bump();
                Ok(Value::Null)
            }
            Some(b'\'') => {
                self.bump();
                Ok(Value::Str(self.string()?))
            }
            Some(b'.') => {
                self.bump();
                let word = self.word().to_owned();
                self.require(b'.', "the closing dot of an enumeration")?;
                Ok(Value::Enum(word))
            }
            Some(b'#') => {
                self.bump();
                let word = self.word();
                word.parse::<u32>().map(Value::Ref).map_err(|_| {
                    CodecError::malformed("STEP_DATA reference is not an instance number")
                })
            }
            Some(b'(') => {
                self.bump();
                Ok(Value::List(self.list(depth)?))
            }
            Some(byte) if byte.is_ascii_digit() || matches!(byte, b'-' | b'+') => self.number(),
            Some(byte) if byte.is_ascii_alphabetic() => {
                let name = self.word().to_owned();
                self.require(b'(', "the arguments of a typed parameter")?;
                Ok(Value::Typed(name, self.list(depth)?))
            }
            _ => Err(CodecError::malformed(format_args!(
                "STEP_DATA parameter is not valid at byte {}",
                self.at
            ))),
        }
    }

    /// The parameters after an opening parenthesis, through the closing one.
    fn list(&mut self, depth: usize) -> Result<Vec<Value>, CodecError> {
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b')') {
            self.bump();
            return Ok(items);
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.bump(),
                Some(b')') => {
                    self.bump();
                    return Ok(items);
                }
                _ => {
                    return Err(CodecError::malformed(format_args!(
                        "STEP_DATA list is not closed at byte {}",
                        self.at
                    )))
                }
            }
        }
    }
}

fn find(bytes: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + from)
}

fn header_schemas(header: &[u8]) -> Vec<String> {
    let Some(at) = find(header, b"FILE_SCHEMA", 0) else {
        return Vec::new();
    };
    let mut cursor = Cursor {
        bytes: header,
        at,
        parameters: 0,
    };
    let mut names = Vec::new();
    cursor.at += b"FILE_SCHEMA".len();
    while let Some(byte) = cursor.peek() {
        match byte {
            b'\'' => {
                cursor.bump();
                if let Ok(name) = cursor.string() {
                    names.push(name);
                }
            }
            b';' => break,
            _ => cursor.bump(),
        }
    }
    names
}

/// Reads the schema names and the entity instances of a Part 21 stream.
pub(crate) fn parse(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Part21, CodecError> {
    let data = find(bytes, b"DATA;", 0)
        .ok_or_else(|| CodecError::malformed("STEP_DATA has no DATA section"))?;
    let mut part = Part21 {
        schemas: header_schemas(&bytes[..data]),
        instances: BTreeMap::new(),
    };
    let mut cursor = Cursor {
        bytes,
        at: data + b"DATA;".len(),
        parameters: 0,
    };
    loop {
        cursor.skip_space();
        match cursor.peek() {
            Some(b'#') => {
                cursor.bump();
                let id = cursor
                    .word()
                    .parse::<u32>()
                    .map_err(|_| CodecError::malformed("STEP_DATA instance has no number"))?;
                cursor.require(b'=', "'=' after an instance number")?;
                cursor.skip_space();
                let name = cursor.word().to_owned();
                cursor.require(b'(', "the arguments of an instance")?;
                cursor.parameters = 0;
                let args = cursor.list(0)?;
                cursor.require(b';', "';' after an instance")?;
                ctx.charge_entities(1, "admit STEP_DATA instance")?;
                ctx.charge_entities(
                    u64::try_from(cursor.parameters).map_err(|_| {
                        CodecError::malformed("STEP_DATA parameter count does not fit 64 bits")
                    })?,
                    "admit STEP_DATA parameters",
                )?;
                if part
                    .instances
                    .insert(id, Instance { id, name, args })
                    .is_some()
                {
                    return Err(CodecError::malformed(format_args!(
                        "STEP_DATA repeats instance #{id}"
                    )));
                }
            }
            _ => break,
        }
    }
    // A stream cut at an instance boundary has no ENDSEC and is refused, not
    // taken for a complete section.
    if cursor.word() != "ENDSEC" {
        return Err(CodecError::malformed(
            "STEP_DATA ends without the ENDSEC of the DATA section",
        ));
    }
    cursor.require(b';', "';' after ENDSEC")?;
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    fn with_ctx<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).unwrap();
        run(&ctx)
    }

    #[test]
    fn reads_instances_and_parameter_forms() {
        let part = with_ctx(|ctx| {
            parse(
                ctx,
                b"HEADER;\nFILE_SCHEMA (('ALIBRE_SCHEMA','X'));\nENDSEC;\nDATA;\n\
                  #1=A('it''s',.T.,$,(#2,#3),-1.5E1,7,B(2.));\n#2=C();\nENDSEC;",
            )
            .unwrap()
        });
        assert!(part.is_alibre());
        let first = part.get(1).unwrap();
        assert_eq!(first.name, "A");
        assert_eq!(first.arg(0), Some(&Value::Str("it's".into())));
        assert_eq!(first.arg(1).and_then(Value::as_logical), Some(true));
        assert_eq!(first.arg(2), Some(&Value::Null));
        assert_eq!(first.arg(3).and_then(Value::refs), Some(vec![2, 3]));
        assert_eq!(first.arg(4).and_then(Value::as_real), Some(-15.0));
        assert_eq!(first.arg(5).and_then(Value::as_int), Some(7));
        assert_eq!(
            first.arg(6),
            Some(&Value::Typed("B".into(), vec![Value::Real(2.0)]))
        );
        assert_eq!(part.get(2).unwrap().args.len(), 0);
    }

    #[test]
    fn refuses_a_repeated_instance_and_deep_nesting() {
        with_ctx(|ctx| {
            assert!(parse(ctx, b"DATA;#1=A();#1=A();ENDSEC;").is_err());
            let deep = format!("DATA;#1=A({}{});", "(".repeat(40), ")".repeat(40));
            assert!(parse(ctx, deep.as_bytes()).is_err());
        });
    }

    #[test]
    fn refuses_a_section_without_endsec() {
        with_ctx(|ctx| {
            assert!(parse(ctx, b"DATA;#1=A();").is_err());
            assert!(parse(ctx, b"DATA;#1=A();#2=B();ENDSEC").is_err());
            assert!(parse(ctx, b"DATA;#1=A();\nENDSEC;\n").is_ok());
        });
    }

    #[test]
    fn bounds_the_parameters_of_an_instance_and_the_octets_of_a_string() {
        with_ctx(|ctx| {
            let many = format!(
                "DATA;#1=A({});ENDSEC;",
                vec!["1"; MAX_INSTANCE_PARAMETERS + 1].join(",")
            );
            assert!(parse(ctx, many.as_bytes()).is_err());
            let fits = format!(
                "DATA;#1=A({});ENDSEC;",
                vec!["1"; MAX_INSTANCE_PARAMETERS - 1].join(",")
            );
            assert!(parse(ctx, fits.as_bytes()).is_ok());
            let long = format!("DATA;#1=A('{}');ENDSEC;", "x".repeat(MAX_STRING_OCTETS + 1));
            assert!(parse(ctx, long.as_bytes()).is_err());
            let exact = format!("DATA;#1=A('{}');ENDSEC;", "x".repeat(MAX_STRING_OCTETS));
            assert!(parse(ctx, exact.as_bytes()).is_ok());
        });
    }

    #[test]
    fn an_integer_beyond_i32_reads_as_a_real() {
        assert_eq!(Value::Int(10_000_000_000).as_real(), Some(1e10));
    }
}
