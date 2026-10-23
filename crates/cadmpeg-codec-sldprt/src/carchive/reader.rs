//! The byte reader and object-pointer resolution behind the class rules.
//!
//! A line-for-line port of the research grammar's engine
//! (`research/sldprt-mfc-archive/grammar/gram.py`). Rules read with the
//! consuming primitives (`u16`, `f64`, `cstring`, `ptr`, ...) and look ahead
//! with the peeking ones (`eq`, `zeros`, `byte`, `pu32`, ...), which take an
//! absolute offset and follow Python's slicing: a range past the end compares
//! unequal rather than failing, while an index past the end fails.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Why a read stopped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop {
    /// The bytes do not have the shape a rule expects (`gram.Bad`).
    Bad(String),
    /// An object of a class with no rule (`gram.Unknown`).
    Unknown(Arc<str>),
    /// A read or index past the end of the stream (`IndexError`,
    /// `struct.error`).
    End(usize),
}

pub(crate) type Res<T> = Result<T, Stop>;

/// A `Stop::Bad` with `message`.
pub(crate) fn bad(message: impl Into<String>) -> Stop {
    Stop::Bad(message.into())
}

/// A `CObject*` as read: null, a back-reference to an object read earlier,
/// or an object read here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ptr {
    /// `('null',)`.
    Null,
    /// `('ref', index, class)`: the class of the object at `index`, when the
    /// stream read it.
    Ref { index: u32, class: Option<Arc<str>> },
    /// `('obj', class, value)`: the value the class's rule returned.
    Obj { class: Arc<str>, value: V },
}

impl Ptr {
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub(crate) fn is_ref(&self) -> bool {
        matches!(self, Self::Ref { .. })
    }

    pub(crate) fn is_obj(&self) -> bool {
        matches!(self, Self::Obj { .. })
    }

    /// `p[0]`: `"null"`, `"ref"` or `"obj"`.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Ref { .. } => "ref",
            Self::Obj { .. } => "obj",
        }
    }

    /// An object's class (`p[1]` of an `obj`).
    pub(crate) fn obj_class(&self) -> Option<&str> {
        match self {
            Self::Obj { class, .. } => Some(class),
            _ => None,
        }
    }

    /// A back-reference's class (`p[2]` of a `ref`).
    pub(crate) fn ref_class(&self) -> Option<&str> {
        match self {
            Self::Ref { class, .. } => class.as_deref(),
            _ => None,
        }
    }

    /// The object's class, or the referenced object's class.
    pub(crate) fn class(&self) -> Option<&str> {
        self.obj_class().or_else(|| self.ref_class())
    }

    /// An object's value (`p[2]` of an `obj`).
    pub(crate) fn value(&self) -> Option<&V> {
        match self {
            Self::Obj { value, .. } => Some(value),
            _ => None,
        }
    }
}

/// A rule's return value: the dynamic values the Python rules return.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum V {
    #[default]
    None,
    Bool(bool),
    Int(i64),
    F(f64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<V>),
    Map(BTreeMap<String, V>),
    Ptr(Box<Ptr>),
}

impl V {
    /// A map from `(key, value)` pairs.
    pub(crate) fn map<const N: usize>(pairs: [(&str, V); N]) -> Self {
        Self::Map(
            pairs
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    /// `v.get(key)`, `V::None` when absent or not a map.
    pub(crate) fn get(&self, key: &str) -> &V {
        static NONE: V = V::None;
        match self {
            Self::Map(map) => map.get(key).unwrap_or(&NONE),
            _ => &NONE,
        }
    }

    /// Insert into a map (`v[key] = value`); no effect on other values.
    pub(crate) fn set(&mut self, key: &str, value: V) {
        if let Self::Map(map) = self {
            map.insert(key.to_owned(), value);
        }
    }

    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub(crate) fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Bool(value) => Some(i64::from(*value)),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F(value) => Some(*value),
            #[allow(clippy::cast_precision_loss)]
            Self::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_ptr(&self) -> Option<&Ptr> {
        match self {
            Self::Ptr(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_list(&self) -> Option<&[V]> {
        match self {
            Self::List(value) => Some(value),
            _ => None,
        }
    }

    /// Python truthiness.
    pub(crate) fn truthy(&self) -> bool {
        match self {
            Self::None => false,
            Self::Bool(value) => *value,
            Self::Int(value) => *value != 0,
            Self::F(value) => *value != 0.0,
            Self::Str(value) => !value.is_empty(),
            Self::Bytes(value) => !value.is_empty(),
            Self::List(value) => !value.is_empty(),
            Self::Map(value) => !value.is_empty(),
            Self::Ptr(_) => true,
        }
    }
}

impl From<Ptr> for V {
    fn from(value: Ptr) -> Self {
        Self::Ptr(Box::new(value))
    }
}

impl From<i64> for V {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<u32> for V {
    fn from(value: u32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<u16> for V {
    fn from(value: u16) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<u8> for V {
    fn from(value: u8) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<f64> for V {
    fn from(value: f64) -> Self {
        Self::F(value)
    }
}

impl From<bool> for V {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for V {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<&str> for V {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl<T: Into<V>> From<Vec<T>> for V {
    fn from(value: Vec<T>) -> Self {
        Self::List(value.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<V>> From<Option<T>> for V {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::None, Into::into)
    }
}

/// One object the reader opened: its class, where its body starts and where
/// its tag or declaration starts (`r.trace`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Trace {
    pub(crate) class: Arc<str>,
    pub(crate) body: usize,
    pub(crate) at: usize,
}

/// A named parameter object read by its rule.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParameterRecord {
    /// `moLengthParameter_c`, `moAngleParameter_c`, `moScalerParameter_c`
    /// or `moIntegerParameter_c`.
    pub(crate) class: Arc<str>,
    pub(crate) name: V,
    /// Metres, radians, a plain real, or (an integer parameter) a count.
    pub(crate) value: f64,
    /// Where the object's tag or declaration starts.
    pub(crate) at: usize,
}

/// A named object read by the named-object header rule.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NamedRecord {
    pub(crate) class: Option<Arc<str>>,
    pub(crate) name: V,
    pub(crate) oid: u32,
}

/// The reader over one stream.
pub(crate) struct Reader<'b> {
    /// The stream (`r.b`).
    pub(crate) b: &'b [u8],
    /// The read position (`r.p`).
    pub(crate) p: usize,
    /// Map index to class name (`r.known`).
    pub(crate) known: HashMap<u32, Arc<str>>,
    /// The map counter (`r.counter`).
    pub(crate) counter: u32,
    /// Map index to the class of the object read there (`r.objs`).
    pub(crate) objs: HashMap<u32, Arc<str>>,
    /// Every object opened, in order (`r.trace`).
    pub(crate) trace: Vec<Trace>,
    /// The file's `swVersion` (`gram.CTX['ver']`).
    pub(crate) ver: Option<u32>,
    /// Rule state shared across the rule modules.
    pub(crate) st: super::rules::State,
    /// Parameter objects read.
    pub(crate) parameters: Vec<ParameterRecord>,
    /// Named objects read.
    pub(crate) named: Vec<NamedRecord>,
}

impl<'b> Reader<'b> {
    /// A reader at `pos` of `b` with the class map `known` and the map
    /// counter at `counter`.
    pub(crate) fn new(
        b: &'b [u8],
        pos: usize,
        known: HashMap<u32, Arc<str>>,
        counter: u32,
        ver: Option<u32>,
    ) -> Self {
        Self {
            b,
            p: pos,
            known,
            counter,
            objs: HashMap::new(),
            trace: Vec::new(),
            ver,
            st: super::rules::State::default(),
            parameters: Vec::new(),
            named: Vec::new(),
        }
    }

    fn take<const N: usize>(&mut self) -> Res<[u8; N]> {
        let bytes = self
            .p
            .checked_add(N)
            .and_then(|end| self.b.get(self.p..end))
            .ok_or(Stop::End(self.p))?;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        self.p += N;
        Ok(out)
    }

    pub(crate) fn u8(&mut self) -> Res<u8> {
        Ok(self.take::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Res<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    pub(crate) fn i16(&mut self) -> Res<i16> {
        Ok(i16::from_le_bytes(self.take()?))
    }

    pub(crate) fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    pub(crate) fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_le_bytes(self.take()?))
    }

    pub(crate) fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.take()?))
    }

    pub(crate) fn f32(&mut self) -> Res<f32> {
        Ok(f32::from_le_bytes(self.take()?))
    }

    pub(crate) fn f64(&mut self) -> Res<f64> {
        Ok(f64::from_le_bytes(self.take()?))
    }

    /// `r.skip(n)`: moves without a bounds check, as the Python does.
    pub(crate) fn skip(&mut self, n: usize) {
        self.p = self.p.saturating_add(n);
    }

    /// `r.expect(bs, what)`.
    pub(crate) fn expect(&mut self, bytes: &[u8], what: &str) -> Res<()> {
        if !self.eq(self.p, bytes) {
            return Err(bad(format!(
                "expected {} {what} at +{}",
                hex(bytes),
                self.p
            )));
        }
        self.p += bytes.len();
        Ok(())
    }

    /// `r.expect(bytes(n), what)`.
    pub(crate) fn expect_zeros(&mut self, n: usize, what: &str) -> Res<()> {
        if !self.zeros(self.p, n) {
            return Err(bad(format!(
                "expected {n} zero bytes {what} at +{}",
                self.p
            )));
        }
        self.p += n;
        Ok(())
    }

    /// `r.cstring()`: an MFC `CString` (`ff fe ff`, a u8 or `ff` and a u16
    /// length, UTF-16LE).
    pub(crate) fn cstring(&mut self) -> Res<String> {
        if !self.eq(self.p, &[0xff, 0xfe, 0xff]) {
            return Err(bad(format!("no CString at {}", self.p)));
        }
        self.p += 3;
        let mut n = usize::from(self.u8()?);
        if n == 0xff {
            n = usize::from(self.u16()?);
        }
        let start = self.p;
        let end = start.saturating_add(2 * n);
        // A short slice decodes what is there, as Python's slicing does.
        let bytes = &self.b[start.min(self.b.len())..end.min(self.b.len())];
        let units = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], *pair.get(1).unwrap_or(&0)]))
            .collect::<Vec<_>>();
        self.p = end;
        Ok(String::from_utf16_lossy(&units))
    }

    /// `r.ptr(want)`: a `CObject*`. An object's body is read by its class's
    /// rule; `want` names the classes an object here may have.
    pub(crate) fn ptr(&mut self, want: Option<&[&str]>) -> Res<Ptr> {
        let w = self.u16()?;
        if w == 0 {
            return Ok(Ptr::Null);
        }
        let at = self.p - 2;
        let class: Arc<str> = if w == 0xffff {
            self.u16()?;
            let n = usize::from(self.u16()?);
            let start = self.p;
            let end = start.saturating_add(n);
            let bytes = &self.b[start.min(self.b.len())..end.min(self.b.len())];
            let class: Arc<str> = bytes
                .iter()
                .map(|&byte| char::from(byte))
                .collect::<String>()
                .into();
            self.p = end;
            self.known.insert(self.counter, class.clone());
            self.counter += 1;
            class
        } else if w == 0x7fff {
            let v = self.u32()?;
            if v & 0x8000_0000 == 0 {
                return Ok(Ptr::Ref {
                    index: v,
                    class: self.objs.get(&v).cloned(),
                });
            }
            self.known
                .get(&(v & 0x7fff_ffff))
                .cloned()
                .ok_or_else(|| bad(format!("unknown long class index {}", v & 0x7fff_ffff)))?
        } else if w & 0x8000 != 0 {
            self.known
                .get(&u32::from(w & 0x7fff))
                .cloned()
                .ok_or_else(|| {
                    bad(format!(
                        "unknown class index {} at {}",
                        w & 0x7fff,
                        self.p - 2
                    ))
                })?
        } else {
            let index = u32::from(w);
            return Ok(Ptr::Ref {
                index,
                class: self.objs.get(&index).cloned(),
            });
        };
        self.objs.insert(self.counter, class.clone());
        self.counter += 1;
        if let Some(want) = want {
            if !want.contains(&&*class) {
                return Err(bad(format!("{class} where {} expected", want.join("/"))));
            }
        }
        let Some(rule) = super::rules::rule(&class) else {
            return Err(Stop::Unknown(class));
        };
        self.trace.push(Trace {
            class: class.clone(),
            body: self.p,
            at,
        });
        let value = rule(self)?;
        Ok(Ptr::Obj { class, value })
    }

    /// `r.obj(want)`: a pointer that must be an object read here.
    pub(crate) fn obj(&mut self, want: Option<&[&str]>) -> Res<Ptr> {
        let v = self.ptr(want)?;
        if !v.is_obj() {
            return Err(bad(format!("{} where an object was expected", v.kind())));
        }
        Ok(v)
    }

    /// The class of the most recently opened object (`r.trace[-1][0]`).
    pub(crate) fn last_class(&self) -> Option<Arc<str>> {
        self.trace.last().map(|trace| trace.class.clone())
    }

    // --- Peeking at absolute offsets, with Python's slice semantics. ---

    /// `r.b[at:at+len(bytes)] == bytes`.
    pub(crate) fn eq(&self, at: usize, bytes: &[u8]) -> bool {
        at.checked_add(bytes.len())
            .and_then(|end| self.b.get(at..end))
            .is_some_and(|window| window == bytes)
            || (bytes.is_empty())
    }

    /// `r.b[at:at+n] == bytes(n)`.
    pub(crate) fn zeros(&self, at: usize, n: usize) -> bool {
        // An empty slice equals `bytes(0)` wherever it starts.
        n == 0
            || at
                .checked_add(n)
                .and_then(|end| self.b.get(at..end))
                .is_some_and(|window| window.iter().all(|&byte| byte == 0))
    }

    /// `r.b[at:at+n] == b'\xff' * n`.
    pub(crate) fn ffs(&self, at: usize, n: usize) -> bool {
        at.checked_add(n)
            .and_then(|end| self.b.get(at..end))
            .is_some_and(|window| window.iter().all(|&byte| byte == 0xff))
    }

    /// `r.b[a:a+n]`, clipped to the stream as Python slices are.
    pub(crate) fn slice(&self, at: usize, n: usize) -> &'b [u8] {
        let start = at.min(self.b.len());
        let end = at.saturating_add(n).min(self.b.len());
        &self.b[start..end]
    }

    /// `r.b[at]`.
    pub(crate) fn byte(&self, at: usize) -> Res<u8> {
        self.b.get(at).copied().ok_or(Stop::End(at))
    }

    fn peek<const N: usize>(&self, at: usize) -> Res<[u8; N]> {
        let bytes = at
            .checked_add(N)
            .and_then(|end| self.b.get(at..end))
            .ok_or(Stop::End(at))?;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    /// `struct.unpack_from('<H', r.b, at)[0]`.
    pub(crate) fn pu16(&self, at: usize) -> Res<u16> {
        Ok(u16::from_le_bytes(self.peek(at)?))
    }

    /// `struct.unpack_from('<h', r.b, at)[0]`.
    pub(crate) fn pi16(&self, at: usize) -> Res<i16> {
        Ok(i16::from_le_bytes(self.peek(at)?))
    }

    /// `struct.unpack_from('<I', r.b, at)[0]`.
    pub(crate) fn pu32(&self, at: usize) -> Res<u32> {
        Ok(u32::from_le_bytes(self.peek(at)?))
    }

    /// `struct.unpack_from('<i', r.b, at)[0]`.
    pub(crate) fn pi32(&self, at: usize) -> Res<i32> {
        Ok(i32::from_le_bytes(self.peek(at)?))
    }

    /// `struct.unpack_from('<d', r.b, at)[0]`.
    pub(crate) fn pf64(&self, at: usize) -> Res<f64> {
        Ok(f64::from_le_bytes(self.peek(at)?))
    }

    /// `r.b.find(pattern, start, end)`, `None` for -1.
    pub(crate) fn find(&self, pattern: &[u8], start: usize, end: usize) -> Option<usize> {
        let end = end.min(self.b.len());
        if start > end || pattern.len() > end - start {
            return if pattern.is_empty() && start <= end {
                Some(start)
            } else {
                None
            };
        }
        self.b[start..end]
            .windows(pattern.len().max(1))
            .position(|window| pattern.is_empty() || window == pattern)
            .map(|index| start + index)
    }

    /// `len(r.b)`.
    pub(crate) fn len(&self) -> usize {
        self.b.len()
    }
}

/// Lower-case hex of `bytes`, as Python's `bytes.hex()`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }
    out
}
