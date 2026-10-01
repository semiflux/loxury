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

    pub fn string(h: GcHandle<String>) -> Self {
        Self::obj(TAG_STRING, h.index())
    }

    pub fn function(h: GcHandle<Function>) -> Self {
        Self::obj(TAG_FUNCTION, h.index())
    }

    pub fn native(h: GcHandle<Native>) -> Self {
        Self::obj(TAG_NATIVE, h.index())
    }

    pub fn closure(h: GcHandle<Closure>) -> Self {
        Self::obj(TAG_CLOSURE, h.index())
    }

    pub fn class(h: GcHandle<Class>) -> Self {
        Self::obj(TAG_CLASS, h.index())
    }

    pub fn instance(h: GcHandle<Instance>) -> Self {
        Self::obj(TAG_INSTANCE, h.index())
    }

    pub fn method(h: GcHandle<BoundMethod>) -> Self {
        Self::obj(TAG_BOUND_METHOD, h.index())
    }

    pub fn is_number(&self) -> bool {
        self.0 & QNAN != QNAN
    }

    pub fn is_nil(&self) -> bool {
        self.0 == NIL_VAL
    }

    pub fn is_bool(&self) -> bool {
        (self.0 | 1) == TRUE_VAL
    }

    fn is_obj(&self) -> bool {
        self.0 & (QNAN | SIGN_BIT) == (QNAN | SIGN_BIT)
    }

    // valid only under `is_obj`; the tag space is our arena ids
    fn kind(&self) -> u8 {
        (self.0 >> 32) as u8
    }

    fn slot(&self) -> usize {
        (self.0 & INDEX_MASK) as usize
    }

    pub fn is_string(&self) -> bool {
        self.is_obj() && self.kind() == TAG_STRING
    }

    pub fn is_function(&self) -> bool {
        self.is_obj() && self.kind() == TAG_FUNCTION
    }

    pub fn is_native(&self) -> bool {
        self.is_obj() && self.kind() == TAG_NATIVE
    }

    pub fn is_closure(&self) -> bool {
        self.is_obj() && self.kind() == TAG_CLOSURE
    }

    pub fn is_class(&self) -> bool {
        self.is_obj() && self.kind() == TAG_CLASS
    }

    pub fn is_instance(&self) -> bool {
        self.is_obj() && self.kind() == TAG_INSTANCE
    }

    pub fn is_method(&self) -> bool {
        self.is_obj() && self.kind() == TAG_BOUND_METHOD
    }

    pub fn as_bool(&self) -> bool {
        self.0 == TRUE_VAL
    }

    pub fn as_number(&self) -> f64 {
        f64::from_bits(self.0)
    }

    pub fn values_equal(&self, other: &Self) -> bool {
        if self.is_number() && other.is_number() {
            self.as_number() == other.as_number()
        } else {
            self.0 == other.0
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_string(&self) -> Result<GcHandle<String>, ()> {
        if self.is_string() {
            Ok(GcHandle::tagged(TAG_STRING, self.slot()))
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_function(&self) -> Result<GcHandle<Function>, ()> {
        if self.is_function() {
            Ok(GcHandle::tagged(TAG_FUNCTION, self.slot()))
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_native(&self) -> Result<GcHandle<Native>, ()> {
        if self.is_native() {
            Ok(GcHandle::tagged(TAG_NATIVE, self.slot()))
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_closure(&self) -> Result<GcHandle<Closure>, ()> {
        if self.is_closure() {
            Ok(GcHandle::tagged(TAG_CLOSURE, self.slot()))
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_class(&self) -> Result<GcHandle<Class>, ()> {
        if self.is_class() {
            Ok(GcHandle::tagged(TAG_CLASS, self.slot()))
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_instance(&self) -> Result<GcHandle<Instance>, ()> {
        if self.is_instance() {
            Ok(GcHandle::tagged(TAG_INSTANCE, self.slot()))
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_method(&self) -> Result<GcHandle<BoundMethod>, ()> {
        if self.is_method() {
            Ok(GcHandle::tagged(TAG_BOUND_METHOD, self.slot()))
        } else {
            Err(())
        }
    }
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
