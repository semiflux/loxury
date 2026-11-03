//! NaN-boxed `Value` (opt-in via the `nan-boxing` feature).
//!
//! Layout mirrors clox (`value.h`): immediates live outside the quiet-NaN
//! range, heap handles are tagged payloads under `SIGN_BIT | QNAN`.
//! The payload is `(arena_tag << 32) | slot`, where `arena_tag` is the
//! same MSB tag every `GcHandle` already carries.

use crate::chunk::{BoundMethod, Class, Closure, Function, Instance, Native};
use crate::gc::{
    GcHandle, TAG_BOUND_METHOD, TAG_CLASS, TAG_CLOSURE, TAG_FUNCTION, TAG_INSTANCE, TAG_NATIVE,
    TAG_STRING,
};

const QNAN: u64 = 0x7ffc_0000_0000_0000;
const SIGN_BIT: u64 = 0x8000_0000_0000_0000;

const TAG_NIL: u64 = 1;
const TAG_FALSE: u64 = 2;
const TAG_TRUE: u64 = 3;

const NIL_VAL: u64 = QNAN | TAG_NIL;
const FALSE_VAL: u64 = QNAN | TAG_FALSE;
const TRUE_VAL: u64 = QNAN | TAG_TRUE;

const INDEX_MASK: u64 = 0xFFFF_FFFF;

#[derive(Clone, Copy)]
pub struct Value(u64);

fn obj_val(kind: u8, index: usize) -> u64 {
    debug_assert!((index as u64) <= INDEX_MASK, "handle index overflow");
    SIGN_BIT | QNAN | ((kind as u64) << 32) | (index as u64)
}

impl Value {
    pub fn nil() -> Self {
        Self(NIL_VAL)
    }

    pub fn boolean(b: bool) -> Self {
        Self(if b { TRUE_VAL } else { FALSE_VAL })
    }

    pub fn number(n: f64) -> Self {
        Self(n.to_bits())
    }

    fn obj(kind: u8, index: usize) -> Self {
        Self(obj_val(kind, index))
    }

    #[inline(always)]
    pub fn is_number(&self) -> bool {
        self.0 & QNAN != QNAN
    }

    #[inline(always)]
    pub fn is_nil(&self) -> bool {
        self.0 == NIL_VAL
    }

    #[inline(always)]
    pub fn is_bool(&self) -> bool {
        (self.0 | 1) == TRUE_VAL
    }

    #[inline(always)]
    fn is_obj(&self) -> bool {
        self.0 & (QNAN | SIGN_BIT) == (QNAN | SIGN_BIT)
    }

    // valid only under `is_obj`; the tag space is our arena ids
    #[inline(always)]
    fn kind(&self) -> u8 {
        (self.0 >> 32) as u8
    }

    #[inline(always)]
    fn slot(&self) -> usize {
        (self.0 & INDEX_MASK) as usize
    }

    #[inline(always)]
    pub fn as_bool(&self) -> bool {
        self.0 == TRUE_VAL
    }

    #[inline(always)]
    pub fn as_number(&self) -> f64 {
        f64::from_bits(self.0)
    }

    #[inline(always)]
    pub fn values_equal(&self, other: &Self) -> bool {
        if self.is_number() && other.is_number() {
            self.as_number() == other.as_number()
        } else {
            self.0 == other.0
        }
    }
}

macro_rules! object_values {
    ($($ctor:ident => $is:ident, $try:ident, $tag:ident, $ty:ty;)*) => {
        impl Value {
            $(
                pub fn $ctor(handle: GcHandle<$ty>) -> Self {
                    Self::obj($tag, handle.index())
                }

                #[inline(always)]
                pub fn $is(&self) -> bool {
                    self.is_obj() && self.kind() == $tag
                }

                #[allow(clippy::result_unit_err)]
                pub fn $try(&self) -> Result<GcHandle<$ty>, ()> {
                    if self.$is() {
                        Ok(GcHandle::tagged($tag, self.slot()))
                    } else {
                        Err(())
                    }
                }
            )*
        }
    };
}

object_values! {
    string => is_string, try_as_string, TAG_STRING, String;
    function => is_function, try_as_function, TAG_FUNCTION, Function;
    native => is_native, try_as_native, TAG_NATIVE, Native;
    closure => is_closure, try_as_closure, TAG_CLOSURE, Closure;
    class => is_class, try_as_class, TAG_CLASS, Class;
    instance => is_instance, try_as_instance, TAG_INSTANCE, Instance;
    method => is_method, try_as_method, TAG_BOUND_METHOD, BoundMethod;
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_number() {
            write!(f, "number({})", self.as_number())
        } else if self.is_nil() {
            write!(f, "nil")
        } else if self.is_bool() {
            write!(f, "bool({})", self.as_bool())
        } else if self.is_obj() {
            write!(f, "obj(kind={}, slot={})", self.kind(), self.slot())
        } else {
            write!(f, "bad({:#x})", self.0)
        }
    }
}
