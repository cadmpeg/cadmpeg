// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_retained(&self, _bytes: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
fn construct<T: Into<String>>(text: T) -> String {
    text.into()
}
fn forward<T: Into<String>>(text: T) -> String {
    construct(text)
}
struct Tagged {
    tag: Option<String>,
}
impl Tagged {
    fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }
}
pub fn charged(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _direct = text.to_owned();
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _from = String::from(text);
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _into: String = text.into();
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "owned")?;
    let _nested = forward(text);
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "tag")?;
    let _tagged = Tagged { tag: None }.with_tag(text);
    Ok(())
}
pub fn wrong(ctx: &DecodeContext, text: &str, other: &str, yes: bool) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(other.len()).map_err(|_| ())?, "wrong")?;
    let _wrong = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    if yes {
        ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "branch")?;
    }
    let _conditional = text.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _drop = ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "drop");
    let _dropped = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "once")?;
    let _once = forward(text);
    let _reuse = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
pub fn changed<'a>(ctx: &DecodeContext, mut text: &'a str, other: &'a str) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "before change")?;
    text = other;
    let _changed = forward(text); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
fn duplicate(text: &str) -> (String, String) {
    (text.to_owned(), text.to_owned()) // finding: uncharged_decode_allocation, uncharged_decode_work, uncharged_decode_allocation, uncharged_decode_work
}
pub fn duplicated(ctx: &DecodeContext, text: &str) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "one")?;
    let _copies = duplicate(text);
    Ok(())
}

fn repeated_conversion<T: Into<String> + Copy>(text: T, count: usize) {
    for _ in 0..count {
        // finding: uncharged_decode_work
        std::hint::black_box(construct(text));
    }
}
pub fn repeated(ctx: &DecodeContext, text: &str, count: usize) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "one")?;
    repeated_conversion(text, count); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}
fn overwrite<T>(value: &mut T, replacement: T) {
    *value = replacement;
}
fn changed_conversion<T: Into<String>>(text: T, other: T) -> String {
    let mut text = text;
    overwrite(&mut text, other);
    construct(text)
}
pub fn mutated_helper(ctx: &DecodeContext, text: &str, other: &str) -> Result<(), ()> {
    ctx.charge_retained(
        u64::try_from(text.len()).map_err(|_| ())?,
        "before mutation",
    )?;
    let _changed = changed_conversion(text, other); // finding: uncharged_decode_allocation, uncharged_decode_work
    Ok(())
}

pub fn repeated_raw(ctx: &DecodeContext, text: &str, count: usize) -> Result<(), ()> {
    ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "once")?;
    for _ in 0..count {
        // finding: uncharged_decode_work
        let _copy = text.to_owned(); // finding: uncharged_decode_allocation, uncharged_decode_work
    }
    for _ in 0..count {
        // finding: uncharged_decode_work
        ctx.charge_retained(u64::try_from(text.len()).map_err(|_| ())?, "each")?;
        let _copy = text.to_owned();
    }
    Ok(())
}

fn into_string<T: Into<String>>(value: T) -> String {
    value.into()
}

pub fn reflexive_string_move(_ctx: &DecodeContext, value: String) -> String {
    into_string(value)
}

#[derive(Clone, Copy)]
enum CopyAdmission {
    Standard,
}

pub fn reflexive_copy_move(_ctx: &DecodeContext, value: CopyAdmission) -> CopyAdmission {
    value.into()
}

pub mod fixed_admission {
    pub struct DecodeArena {
        records: Vec<String>,
    }

    pub struct DecodeContext<'arena> {
        arena: &'arena DecodeArena,
    }

    pub struct WorkBudget {
        remaining: usize,
    }

    #[derive(Clone, Copy)]
    enum WorkSlicePolicy<'ctx, 'arena> {
        Independent(&'ctx WorkBudget),
        Session {
            context: &'ctx DecodeContext<'arena>,
            work: &'ctx WorkBudget,
        },
    }

    #[derive(Clone, Copy)]
    struct EvaluationWorkSlice<'ctx, 'arena> {
        policy: WorkSlicePolicy<'ctx, 'arena>,
    }

    #[derive(Clone, Copy)]
    enum EvaluationAdmission<'ctx, 'arena> {
        Decode(&'ctx DecodeContext<'arena>),
        Standard,
        WorkSlice(EvaluationWorkSlice<'ctx, 'arena>),
    }

    impl<'ctx, 'arena> From<&'ctx DecodeContext<'arena>> for EvaluationAdmission<'ctx, 'arena> {
        fn from(context: &'ctx DecodeContext<'arena>) -> Self {
            Self::Decode(context)
        }
    }

    struct Scratch<'ctx, 'arena> {
        admission: EvaluationAdmission<'ctx, 'arena>,
    }

    impl<'ctx, 'arena> Scratch<'ctx, 'arena> {
        fn new(admission: impl Into<EvaluationAdmission<'ctx, 'arena>>) -> Self {
            Self {
                admission: admission.into(),
            }
        }
    }

    pub fn scratch_for_admissions<'ctx, 'arena>(
        context: &'ctx DecodeContext<'arena>,
        budget: &'ctx WorkBudget,
    ) {
        let _decode = Scratch::new(context);
        let _standard = Scratch::new(EvaluationAdmission::Standard);
        let work_slice = EvaluationAdmission::WorkSlice(EvaluationWorkSlice {
            policy: WorkSlicePolicy::Session {
                context,
                work: budget,
            },
        });
        let _slice = Scratch::new(work_slice);
    }
}

#[derive(Clone, Copy)]
struct CopyTarget(u8);

impl From<&str> for CopyTarget {
    fn from(value: &str) -> Self {
        let mut checksum = 0;
        for byte in value.bytes() {
            // finding: uncharged_decode_work
            checksum ^= byte;
        }
        Self(checksum)
    }
}

pub fn nonreflexive_copy_conversion(_ctx: &DecodeContext, value: &str) -> CopyTarget {
    value.into()
}
