//! The byte reader and object-pointer resolution behind the class rules.
//!
//! Rules read with the consuming primitives (`u16`, `f64`, `cstring`, `ptr`,
//! ...) and look ahead with the peeking ones (`eq`, `zeros`, `byte`, `pu32`,
//! ...), which take an absolute offset: a range past the end compares unequal
//! rather than failing, while an index past the end fails.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ExpandSpec, ResourceLimit, View};

/// Why a read stopped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stop {
    /// The bytes do not have the shape a rule expects.
    Bad(String),
    /// An object of a class with no rule.
    Unknown(Arc<str>),
    /// A read or index past the end of the stream.
    End(usize),
    /// The decode context refused more objects or nesting.
    Refused(ResourceLimit),
}

pub(crate) type Res<T> = Result<T, Stop>;

/// A `Stop::Bad` with `message`.
pub(crate) fn bad(message: impl Into<String>) -> Stop {
    Stop::Bad(message.into())
}

/// A stop from a decode-context refusal.
fn stopped(error: cadmpeg_core::CodecError) -> Stop {
    match error {
        cadmpeg_core::CodecError::ResourceLimit(limit) => Stop::Refused(limit),
        error => Stop::Bad(error.to_string()),
    }
}

/// A `CObject*` as read: null, a back-reference to an object read earlier,
/// or an object read here.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ptr {
    /// No object.
    Null,
    /// A back-reference to the object at `index`, whose class is known when
    /// the stream read it.
    Ref { index: u32, class: Option<Arc<str>> },
    /// An object: its class and the value the class's rule returned.
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

    /// The pointer kind: `null`, `ref` or `obj`.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Ref { .. } => "ref",
            Self::Obj { .. } => "obj",
        }
    }

    /// An object's class.
    pub(crate) fn obj_class(&self) -> Option<&str> {
        match self {
            Self::Obj { class, .. } => Some(class),
            _ => None,
        }
    }

    /// A back-reference's class.
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

    /// An object's value.
    pub(crate) fn value(&self) -> Option<&V> {
        match self {
            Self::Obj { value, .. } => Some(value),
            _ => None,
        }
    }
}

/// A rule's return value.
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
            Self::Int(value) => cadmpeg_core::convert::f64_from_i64(*value),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_list(&self) -> Option<&[V]> {
        match self {
            Self::List(value) => Some(value),
            _ => None,
        }
    }

    /// Whether the value counts as true.
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
    /// Where the value (an f64, or the integer's u32) starts.
    pub(crate) value_at: usize,
}

/// The classes whose object writes a sketch feature's `sgSketch`.
const PROFILE_FAMILY: [&str; 3] = [
    "moProfileFeature_c",
    "moOriginProfileFeature_c",
    "moSketchBlockDef_c",
];

/// A sketch's profile body: the compressed body of its `sgSketch` tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProfileBody {
    /// Where the body's nested stream starts: its u32 `B`.
    pub(crate) stream: usize,
    /// Where the tag of the innermost profile-family object that holds the
    /// body starts.
    pub(crate) owner: usize,
}

/// The reader over one stream.
pub(crate) struct Reader<'b> {
    /// The stream (`r.b`).
    pub(crate) b: &'b [u8],
    /// The decode context that charges the walk and caps its nesting.
    ctx: &'b DecodeContext<'b>,
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
    /// The file's `swVersion`.
    pub(crate) ver: Option<u32>,
    /// Rule state shared across the rule modules.
    pub(crate) st: super::rules::State,
    /// Parameter objects read.
    pub(crate) parameters: Vec<ParameterRecord>,
    /// The rule nesting depth (`r.depth`): 1 for a top-level object.
    pub(crate) depth: usize,
    /// The flag position of a favourite handle followed by a view block (`FAVFLAG`).
    pub(crate) favflag: Option<usize>,
    /// The objects being read, outermost first: class and tag position.
    pub(crate) open: Vec<(Arc<str>, usize)>,
    /// Where each profile-family object read ends, by its tag position.
    pub(crate) ends: HashMap<usize, usize>,
    /// The sketch profile bodies read.
    pub(crate) profile_bodies: Vec<ProfileBody>,
}

/// Reader state saved before a speculative read.
pub(crate) struct Snapshot {
    p: usize,
    counter: u32,
    objs: HashMap<u32, Arc<str>>,
    known: HashMap<u32, Arc<str>>,
    trace: Vec<Trace>,
    parameters: Vec<ParameterRecord>,
    ver: Option<u32>,
    st: super::rules::State,
    favflag: Option<usize>,
    depth: usize,
    open: Vec<(Arc<str>, usize)>,
    ends: HashMap<usize, usize>,
    profile_bodies: Vec<ProfileBody>,
}

impl<'b> Reader<'b> {
    /// A reader at `pos` of `b` with the class map `known` and the map
    /// counter at `counter`.
    pub(crate) fn new(
        ctx: &'b DecodeContext<'b>,
        b: &'b [u8],
        pos: usize,
        known: HashMap<u32, Arc<str>>,
        counter: u32,
        ver: Option<u32>,
    ) -> Self {
        Self {
            b,
            ctx,
            p: pos,
            known,
            counter,
            objs: HashMap::new(),
            trace: Vec::new(),
            ver,
            st: super::rules::State::default(),
            parameters: Vec::new(),
            depth: 0,
            favflag: None,
            open: Vec::new(),
            ends: HashMap::new(),
            profile_bodies: Vec::new(),
        }
    }

    /// Saves the state a speculative read may change.
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            p: self.p,
            counter: self.counter,
            objs: self.objs.clone(),
            known: self.known.clone(),
            trace: self.trace.clone(),
            parameters: self.parameters.clone(),
            ver: self.ver,
            st: self.st.clone(),
            favflag: self.favflag,
            depth: self.depth,
            open: self.open.clone(),
            ends: self.ends.clone(),
            profile_bodies: self.profile_bodies.clone(),
        }
    }

    /// Restores the state saved by [`Reader::snapshot`].
    pub(crate) fn restore(&mut self, snapshot: Snapshot) {
        self.p = snapshot.p;
        self.counter = snapshot.counter;
        self.objs = snapshot.objs;
        self.known = snapshot.known;
        self.trace = snapshot.trace;
        self.parameters = snapshot.parameters;
        self.ver = snapshot.ver;
        self.st = snapshot.st;
        self.favflag = snapshot.favflag;
        self.depth = snapshot.depth;
        self.open = snapshot.open;
        self.ends = snapshot.ends;
        self.profile_bodies = snapshot.profile_bodies;
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
        let value = View::u16_le_at(self.b, self.p).ok_or(Stop::End(self.p))?;
        self.p += 2;
        Ok(value)
    }

    pub(crate) fn u32(&mut self) -> Res<u32> {
        let value = View::u32_le_at(self.b, self.p).ok_or(Stop::End(self.p))?;
        self.p += 4;
        Ok(value)
    }

    pub(crate) fn u64(&mut self) -> Res<u64> {
        let value = View::u64_le_at(self.b, self.p).ok_or(Stop::End(self.p))?;
        self.p += 8;
        Ok(value)
    }

    pub(crate) fn f64(&mut self) -> Res<f64> {
        let value = View::f64_le_at(self.b, self.p).ok_or(Stop::End(self.p))?;
        self.p += 8;
        Ok(value)
    }

    /// Moves `n` bytes without a bounds check. A position past the end makes
    /// the next read stop with `Stop::End`.
    pub(crate) fn skip(&mut self, n: usize) {
        self.p = self.p.checked_add(n).unwrap_or(self.b.len());
    }

    /// Inflates `compressed` to exactly `declared` bytes, charging the output
    /// against the expansion allowance before it is retained.
    pub(crate) fn zlib_exact(&mut self, compressed: &[u8], declared: u32) -> Res<Vec<u8>> {
        use std::io::Read;
        let mut expand = self
            .ctx
            .begin_expand(ExpandSpec::Exact(u64::from(declared)))
            .map_err(stopped)?;
        let mut decoder = flate2::read::ZlibDecoder::new(compressed);
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let read = decoder.read(&mut chunk).map_err(|_| bad("blob zlib"))?;
            if read == 0 {
                break;
            }
            expand.write(&chunk[..read]).map_err(stopped)?;
        }
        expand.finalize_owned().map_err(stopped)
    }

    /// Advances the map counter, refusing the wrap.
    pub(crate) fn bump_counter(&mut self) -> Res<()> {
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| bad("archive map counter overflows"))?;
        Ok(())
    }

    /// Compares `bytes` and consumes them, or stops with `what`.
    pub(crate) fn expect_bytes(&mut self, bytes: &[u8], what: &str) -> Res<()> {
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

    /// Compares `n` zero bytes and consumes them, or stops with `what`.
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
        let end = start.checked_add(2 * n).ok_or(Stop::End(start))?;
        let bytes = self.b.get(start..end).ok_or(Stop::End(start))?;
        let mut units = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            units.push(View::u16_le_at(bytes, at).ok_or(Stop::End(start))?);
            at += 2;
        }
        self.p = end;
        let value = String::from_utf16_lossy(&units);
        self.ctx
            .retain_bytes(u64_from_index(value.len()), "retain SLDPRT archive CString")
            .map_err(stopped)?;
        Ok(value)
    }

    /// A `CObject*`. An object's body is read by its class's
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
            let end = start.checked_add(n).ok_or(Stop::End(start))?;
            let bytes = &self.b[start.min(self.b.len())..end.min(self.b.len())];
            let class: Arc<str> = bytes
                .iter()
                .map(|&byte| char::from(byte))
                .collect::<String>()
                .into();
            self.p = end;
            self.known.insert(self.counter, class.clone());
            self.bump_counter()?;
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
        self.ctx
            .retain_bytes(
                u64_from_index(class.len()),
                "retain SLDPRT archive class name",
            )
            .map_err(stopped)?;
        self.objs.insert(self.counter, class.clone());
        self.bump_counter()?;
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
        self.ctx
            .charge_entities(1, "walk SLDPRT CArchive object")
            .map_err(stopped)?;
        let guard = self
            .ctx
            .enter_nested_limit("walk SLDPRT CArchive object")
            .map_err(Stop::Refused)?;
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| bad("archive object depth overflows"))?;
        self.open.push((class.clone(), at));
        let value = rule(self);
        self.open.pop();
        drop(guard);
        self.depth -= 1;
        let value = value?;
        if PROFILE_FAMILY.contains(&&*class) {
            self.ends.insert(at, self.p);
        }
        let retained = retained_value_bytes(&value)?;
        self.ctx
            .retain_bytes(retained, "retain SLDPRT CArchive object value")
            .map_err(stopped)?;
        Ok(Ptr::Obj { class, value })
    }

    /// A pointer that must be an object read here.
    pub(crate) fn obj(&mut self, want: Option<&[&str]>) -> Res<Ptr> {
        let v = self.ptr(want)?;
        if !v.is_obj() {
            return Err(bad(format!("{} where an object was expected", v.kind())));
        }
        Ok(v)
    }

    /// Record the profile body whose nested stream starts at `stream`, held by
    /// the innermost open profile-family object.
    pub(crate) fn note_profile_body(&mut self, stream: usize) {
        if let Some(&(_, owner)) = self
            .open
            .iter()
            .rev()
            .find(|(class, _)| PROFILE_FAMILY.contains(&&**class))
        {
            self.profile_bodies.push(ProfileBody { stream, owner });
        }
    }

    /// The class of the most recently opened object.
    pub(crate) fn last_class(&self) -> Option<Arc<str>> {
        self.trace.last().map(|trace| trace.class.clone())
    }

    // --- Peeking at absolute offsets, with slice semantics. ---

    /// Whether the bytes at `at` equal `bytes`.
    pub(crate) fn eq(&self, at: usize, bytes: &[u8]) -> bool {
        at.checked_add(bytes.len())
            .and_then(|end| self.b.get(at..end))
            .is_some_and(|window| window == bytes)
            || (bytes.is_empty())
    }

    /// Whether the `n` bytes at `at` are zero.
    pub(crate) fn zeros(&self, at: usize, n: usize) -> bool {
        // An empty slice equals `bytes(0)` wherever it starts.
        n == 0
            || at
                .checked_add(n)
                .and_then(|end| self.b.get(at..end))
                .is_some_and(|window| window.iter().all(|&byte| byte == 0))
    }

    /// Whether the `n` bytes at `at` are all `ff`.
    pub(crate) fn ffs(&self, at: usize, n: usize) -> bool {
        at.checked_add(n)
            .and_then(|end| self.b.get(at..end))
            .is_some_and(|window| window.iter().all(|&byte| byte == 0xff))
    }

    /// `n` bytes from `at`, clipped to the stream.
    pub(crate) fn slice(&self, at: usize, n: usize) -> &'b [u8] {
        let start = at.min(self.b.len());
        let end = at
            .checked_add(n)
            .map_or(self.b.len(), |end| end.min(self.b.len()));
        &self.b[start..end]
    }

    /// The byte at `at`.
    pub(crate) fn byte(&self, at: usize) -> Res<u8> {
        self.b.get(at).copied().ok_or(Stop::End(at))
    }

    /// The little-endian u16 at `at`.
    pub(crate) fn pu16(&self, at: usize) -> Res<u16> {
        View::u16_le_at(self.b, at).ok_or(Stop::End(at))
    }

    /// The little-endian u32 at `at`.
    pub(crate) fn pu32(&self, at: usize) -> Res<u32> {
        View::u32_le_at(self.b, at).ok_or(Stop::End(at))
    }

    /// The little-endian f64 at `at`.
    pub(crate) fn pf64(&self, at: usize) -> Res<f64> {
        View::f64_le_at(self.b, at).ok_or(Stop::End(at))
    }

    /// The first occurrence of `pattern` in `start..end`, or `None`.
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

    /// The stream length.
    pub(crate) fn len(&self) -> usize {
        self.b.len()
    }
}

/// Retained bytes of a rule's value: one node per value, the entries of its
/// lists and maps, and its map keys. A pointer's target was charged when the
/// object opened, and a string when it was decoded.
fn retained_value_bytes(value: &V) -> Res<u64> {
    let overflow = || bad("archive value size overflows");
    let mut total = u64_from_index(std::mem::size_of::<V>());
    match value {
        V::List(items) => {
            for item in items {
                total = total
                    .checked_add(retained_value_bytes(item)?)
                    .ok_or_else(overflow)?;
            }
        }
        V::Map(map) => {
            for (key, value) in map {
                let key_bytes = u64_from_index(key.len());
                let value_bytes = retained_value_bytes(value)?;
                total = total
                    .checked_add(key_bytes)
                    .and_then(|total| total.checked_add(value_bytes))
                    .ok_or_else(overflow)?;
            }
        }
        V::Ptr(_) => {
            total = total
                .checked_add(u64_from_index(std::mem::size_of::<Ptr>()))
                .ok_or_else(overflow)?;
        }
        V::Bytes(bytes) => {
            total = total
                .checked_add(u64_from_index(bytes.len()))
                .ok_or_else(overflow)?;
        }
        V::None | V::Bool(_) | V::Int(_) | V::F(_) | V::Str(_) => {}
    }
    Ok(total)
}

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }
    out
}
