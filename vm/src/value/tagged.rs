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
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    #[inline(always)]
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    #[inline(always)]
    pub fn as_bool(&self) -> bool {
        if let Self::Bool(b) = self {
            *b
        } else {
            panic!("not a bool")
        }
    }

    #[inline(always)]
    pub fn as_number(&self) -> f64 {
        if let Self::Number(n) = self {
            *n
        } else {
            panic!("not a number")
        }
    }

    #[inline(always)]
    pub fn values_equal(&self, other: &Self) -> bool {
        self == other
    }
}

macro_rules! object_values {
    ($($ctor:ident => $is:ident, $try:ident, $variant:ident, $ty:ty;)*) => {
        impl Value {
            $(
                pub fn $ctor(handle: GcHandle<$ty>) -> Self {
                    Self::$variant(handle)
                }

                #[inline(always)]
                pub fn $is(&self) -> bool {
                    matches!(self, Self::$variant(_))
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
    string => is_string, try_as_string, String, String;
    function => is_function, try_as_function, Function, Function;
    native => is_native, try_as_native, Native, Native;
    closure => is_closure, try_as_closure, Closure, Closure;
    class => is_class, try_as_class, Class, Class;
    instance => is_instance, try_as_instance, Instance, Instance;
    method => is_method, try_as_method, Method, BoundMethod;
}
