use crate::chunk::{BoundMethod, Class, Closure, Function, Instance, Native};
use crate::gc::GcHandle;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Nil,
    Number(f64),
    String(GcHandle<String>),
    Function(GcHandle<Function>),
    Native(GcHandle<Native>),
    Closure(GcHandle<Closure>),
    Class(GcHandle<Class>),
    Instance(GcHandle<Instance>),
    Method(GcHandle<BoundMethod>),
}

impl Value {
    pub fn nil() -> Self {
        Self::Nil
    }

    pub fn boolean(b: bool) -> Self {
        Self::Bool(b)
    }

    pub fn number(n: f64) -> Self {
        Self::Number(n)
    }

    #[inline(always)]
    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    #[inline(always)]
    pub fn is_falsey(&self) -> bool {
        matches!(self, Self::Nil | Self::Bool(false))
    }

    #[allow(clippy::result_unit_err)]
    #[inline(always)]
    pub fn try_as_bool(&self) -> Result<bool, ()> {
        if let Self::Bool(b) = self {
            Ok(*b)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    #[inline(always)]
    pub fn try_as_number(&self) -> Result<f64, ()> {
        if let Self::Number(n) = self {
            Ok(*n)
        } else {
            Err(())
        }
    }

    #[inline(always)]
    pub fn values_equal(&self, other: &Self) -> bool {
        self == other
    }
}

macro_rules! object_values {
    ($($ctor:ident => $try:ident, $variant:ident, $ty:ty;)*) => {
        impl Value {
            $(
                pub fn $ctor(handle: GcHandle<$ty>) -> Self {
                    Self::$variant(handle)
                }

                #[allow(clippy::result_unit_err)]
                pub fn $try(&self) -> Result<GcHandle<$ty>, ()> {
                    if let Self::$variant(handle) = self {
                        Ok(*handle)
                    } else {
                        Err(())
                    }
                }
            )*
        }
    };
}

object_values! {
    string => try_as_string, String, String;
    function => try_as_function, Function, Function;
    native => try_as_native, Native, Native;
    closure => try_as_closure, Closure, Closure;
    class => try_as_class, Class, Class;
    instance => try_as_instance, Instance, Instance;
    method => try_as_method, Method, BoundMethod;
}
