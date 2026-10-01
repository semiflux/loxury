// use std::collections::HashMap;
use fxhash::FxHashMap as HashMap;
use std::fmt::{self, Debug, Display};

// use crate::gc::{Gc, GcHandle, Manager};
use crate::{
    gc::{GcHandle, Heap},
    location::Coords,
};

#[derive(Debug, Default)]
pub struct Chunk {
    code: Vec<u8>,
    coords: Vec<Coords>,
    pub constants: Vec<Value>,
}

impl Chunk {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    // TODO: consider making unsafe, +3% boost
    pub fn byte_at(&self, index: usize) -> u8 {
        self.code[index]
    }

    pub fn at_mut(&mut self, index: usize) -> &mut u8 {
        &mut self.code[index]
    }

    pub fn write(&mut self, byte: u8, coords: Coords) {
        self.code.push(byte);
        self.coords.push(coords);
    }

    pub fn write_nowhere(&mut self, byte: u8) {
        self.code.push(byte)
    }

    pub fn add_constant(&mut self, value: Value) -> u32 {
        self.constants.push(value);
        (self.constants.len() - 1) as u32
    }

    pub fn get_constant(&self, index: u8) -> &Value {
        &self.constants[index as usize]
    }

    pub fn coords(&self, index: usize) -> Coords {
        // write_nowhere epilogue bytes carry no coords; blame the last one
        if self.coords.is_empty() {
            Coords::new(0, 0)
        } else {
            self.coords[index.min(self.coords.len() - 1)]
        }
    }

    pub fn disassemble<W: fmt::Write>(&self, objects: &Heap, out: &mut W) -> fmt::Result {
        let mut bytes = self.code.iter().enumerate();
        macro_rules! simple {
            ($op_name:expr) => {
                writeln!(out, "{}", $op_name)?;
            };
        }
        macro_rules! byte {
            ($op_name:expr) => {{
                let arg = *bytes.next().unwrap().1;
                writeln!(out, "{} {}", $op_name, arg)?;
            }};
        }
        macro_rules! constant {
            ($op_name:expr) => {{
                let index = *bytes.next().unwrap().1 as usize;
                let arg = &self.constants[index];
                write!(out, "{} {} ", $op_name, index)?;
                writeln!(out, "{:?}", ValueDisplay(arg, &objects))?;
            }};
        }
        macro_rules! jump {
            ($op_name:expr, $addr:expr, $sign:tt) => {{
                let offset = u16::from_be_bytes([*bytes.next().unwrap().1, *bytes.next().unwrap().1]);
                let destination = $addr + 3 $sign offset as usize;
                writeln!(out, "{} {} -> {}", $op_name, offset, destination)?;
            }};
        }
        macro_rules! invoke {
            ($op_name:expr) => {{
                let constant = *bytes.next().unwrap().1 as usize;
                let arg_count = *bytes.next().unwrap().1 as u8;
                write!(out, "{} ({} args) {} ", $op_name, arg_count, constant)?;
                let constant = &self.constants[constant];
                writeln!(out, "{:?}", ValueDisplay(constant, &objects))?;
            }};
        }
        while let Some((addr, &b)) = bytes.next() {
            let pos = self.coords(addr);
            write!(out, "{addr:04} {}:{} ", pos.row(), pos.col())?;
            match b.try_into().unwrap() {
                OpCode::Constant => {
                    constant!("constant");
                }
                OpCode::Add => {
                    simple!("add");
                }
                OpCode::Subtract => {
                    simple!("subtract");
                }
                OpCode::Multiply => {
                    simple!("multiply");
                }
                OpCode::Divide => {
                    simple!("divide");
                }
                OpCode::Negate => {
                    simple!("negate");
                }
                OpCode::Return => {
                    simple!("return");
                }
                OpCode::Nil => {
                    simple!("nil");
                }
                OpCode::True => {
                    simple!("true");
                }
                OpCode::False => {
                    simple!("false");
                }
                OpCode::Not => {
                    simple!("not");
                }
                OpCode::Equal => {
                    simple!("equal");
                }
                OpCode::Greater => {
                    simple!("greater");
                }
                OpCode::Less => {
                    simple!("less");
                }
                OpCode::Print => {
                    simple!("print");
                }
                OpCode::Pop => {
                    simple!("pop");
                }
                OpCode::DefineGlobal => {
                    constant!("define_global");
                }
                OpCode::GetGlobal => {
                    constant!("get_global");
                }
                OpCode::SetGlobal => {
                    constant!("set_global");
                }
                OpCode::GetLocal => {
                    byte!("get_local");
                }
                OpCode::SetLocal => {
                    byte!("set_local");
                }
                OpCode::JumpIfFalse => {
                    jump!("jump_if_false", addr, +);
                }
                OpCode::Jump => {
                    jump!("jump", addr, +);
                }
                OpCode::Loop => {
                    jump!("loop", addr, -);
                }
                OpCode::Call => {
                    byte!("call")
                }
                OpCode::Closure => {
                    let index = *bytes.next().unwrap().1 as usize;
                    let arg = &self.constants[index];
                    write!(out, "closure {} ", index)?;
                    writeln!(out, "{:?}", ValueDisplay(arg, objects))?;
                    let function = arg.try_as_function().unwrap();
                    let function = &objects[function];
                    for _ in 0..function.upvalue_count {
                        let (addr, is_local) = bytes.next().unwrap();
                        let (_, index) = bytes.next().unwrap();
                        writeln!(
                            out,
                            "{detail:04} {row}:{col}     {kind} {index}",
                            detail = addr,
                            row = pos.row(),
                            col = pos.col(),
                            kind = if *is_local == 1 { "local" } else { "upvalue" },
                        )?;
                    }
                }
                OpCode::GetUpvalue => {
                    byte!("get_upvalue")
                }
                OpCode::SetUpvalue => {
                    byte!("set_upvalue")
                }
                OpCode::CloseUpvalue => {
                    simple!("close_upvalue");
                }
                OpCode::Class => {
                    constant!("class");
                }
                OpCode::GetProperty => {
                    constant!("get_property")
                }
                OpCode::SetProperty => {
                    constant!("set_property")
                }
                OpCode::Method => {
                    constant!("method")
                }
                OpCode::Invoke => {
                    invoke!("invoke")
                }
                OpCode::Inherit => {
                    simple!("inherit");
                }
                OpCode::GetSuper => {
                    constant!("get_super")
                }
                OpCode::SuperInvoke => {
                    invoke!("super_invoke")
                }
            }
        }
        Ok(())
    }
}

pub enum OpCode {
    Constant,
    Nil,
    True,
    False,
    Pop,
    GetLocal,
    SetLocal,
    GetGlobal,
    DefineGlobal,
    SetGlobal,
    GetUpvalue,
    SetUpvalue,
    GetProperty,
    SetProperty,
    GetSuper,
    Equal,
    Greater,
    Less,
    Add,
    Subtract,
    Multiply,
    Divide,
    Not,
    Negate,
    Print,
    Jump,
    JumpIfFalse,
    Loop,
    Call,
    Invoke,
    SuperInvoke,
    Closure,
    CloseUpvalue,
    Return,
    Class,
    Inherit,
    Method,
}

impl TryFrom<u8> for OpCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Constant),
            1 => Ok(Self::Nil),
            2 => Ok(Self::True),
            3 => Ok(Self::False),
            4 => Ok(Self::Pop),
            5 => Ok(Self::GetLocal),
            6 => Ok(Self::SetLocal),
            7 => Ok(Self::GetGlobal),
            8 => Ok(Self::DefineGlobal),
            9 => Ok(Self::SetGlobal),
            10 => Ok(Self::GetUpvalue),
            11 => Ok(Self::SetUpvalue),
            12 => Ok(Self::GetProperty),
            13 => Ok(Self::SetProperty),
            14 => Ok(Self::GetSuper),
            15 => Ok(Self::Equal),
            16 => Ok(Self::Greater),
            17 => Ok(Self::Less),
            18 => Ok(Self::Add),
            19 => Ok(Self::Subtract),
            20 => Ok(Self::Multiply),
            21 => Ok(Self::Divide),
            22 => Ok(Self::Not),
            23 => Ok(Self::Negate),
            24 => Ok(Self::Print),
            25 => Ok(Self::Jump),
            26 => Ok(Self::JumpIfFalse),
            27 => Ok(Self::Loop),
            28 => Ok(Self::Call),
            29 => Ok(Self::Invoke),
            30 => Ok(Self::SuperInvoke),
            31 => Ok(Self::Closure),
            32 => Ok(Self::CloseUpvalue),
            33 => Ok(Self::Return),
            34 => Ok(Self::Class),
            35 => Ok(Self::Inherit),
            36 => Ok(Self::Method),
            _ => Err(()),
        }
    }
}

#[cfg(not(feature = "nan-boxing"))]
#[derive(Clone, Copy, Debug)]
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

#[cfg(feature = "nan-boxing")]
pub use crate::nanbox::Value;

#[derive(Debug)]
pub struct Native {
    pub arity: u8,
    pub f: fn(&[Value]) -> Value,
}

impl Native {
    pub fn new(arity: u8, f: fn(&[Value]) -> Value) -> Self {
        Self { arity, f }
    }
}

#[cfg(not(feature = "nan-boxing"))]
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

    pub fn string(h: GcHandle<String>) -> Self {
        Self::String(h)
    }

    pub fn function(h: GcHandle<Function>) -> Self {
        Self::Function(h)
    }

    pub fn native(h: GcHandle<Native>) -> Self {
        Self::Native(h)
    }

    pub fn closure(h: GcHandle<Closure>) -> Self {
        Self::Closure(h)
    }

    pub fn class(h: GcHandle<Class>) -> Self {
        Self::Class(h)
    }

    pub fn instance(h: GcHandle<Instance>) -> Self {
        Self::Instance(h)
    }

    pub fn method(h: GcHandle<BoundMethod>) -> Self {
        Self::Method(h)
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_function(&self) -> bool {
        matches!(self, Self::Function(_))
    }

    pub fn is_native(&self) -> bool {
        matches!(self, Self::Native(_))
    }

    pub fn is_closure(&self) -> bool {
        matches!(self, Self::Closure(_))
    }

    pub fn is_class(&self) -> bool {
        matches!(self, Self::Class(_))
    }

    pub fn is_instance(&self) -> bool {
        matches!(self, Self::Instance(_))
    }

    pub fn is_method(&self) -> bool {
        matches!(self, Self::Method(_))
    }

    pub fn as_bool(&self) -> bool {
        if let Self::Bool(b) = self {
            *b
        } else {
            panic!("not a bool")
        }
    }

    pub fn as_number(&self) -> f64 {
        if let Self::Number(n) = self {
            *n
        } else {
            panic!("not a number")
        }
    }

    pub fn values_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Nil, Self::Nil) => true,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Function(a), Self::Function(b)) => a == b,
            (Self::Native(a), Self::Native(b)) => a == b,
            (Self::Closure(a), Self::Closure(b)) => a == b,
            (Self::Class(a), Self::Class(b)) => a == b,
            (Self::Instance(a), Self::Instance(b)) => a == b,
            (Self::Method(a), Self::Method(b)) => a == b,
            _ => false,
        }
    }

    // failure is reported at the call site; nothing to carry
    #[allow(clippy::result_unit_err)]
    pub fn try_as_string(&self) -> Result<GcHandle<String>, ()> {
        if let Self::String(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_function(&self) -> Result<GcHandle<Function>, ()> {
        if let Self::Function(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_native(&self) -> Result<GcHandle<Native>, ()> {
        if let Self::Native(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_instance(&self) -> Result<GcHandle<Instance>, ()> {
        if let Self::Instance(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_class(&self) -> Result<GcHandle<Class>, ()> {
        if let Self::Class(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_closure(&self) -> Result<GcHandle<Closure>, ()> {
        if let Self::Closure(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_as_method(&self) -> Result<GcHandle<BoundMethod>, ()> {
        if let Self::Method(v) = self {
            Ok(*v)
        } else {
            Err(())
        }
    }
}

pub struct ValueDisplay<'a>(pub &'a Value, pub &'a Heap);

// TODO: why did we need this??
pub struct FunctionDisplay<'a>(pub &'a Function, pub &'a Heap);

impl<'a> Display for FunctionDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(v, o) = self;
        match v.name {
            Some(name) => {
                let name = &o[name];
                write!(f, "<fn {name}>")
            }
            None => write!(f, "<script>"),
        }
    }
}

impl<'a> Display for ValueDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(&v, o) = self;
        if v.is_nil() {
            write!(f, "nil")
        } else if v.is_bool() {
            write!(f, "{}", v.as_bool())
        } else if v.is_number() {
            write!(f, "{}", v.as_number())
        } else if let Ok(v) = v.try_as_string() {
            write!(f, "{}", &o[v])
        } else if let Ok(v) = v.try_as_function() {
            write!(f, "{}", FunctionDisplay(&o[v], o))
        } else if v.is_native() {
            write!(f, "<native fn>")
        } else if let Ok(v) = v.try_as_closure() {
            let function = o[v].function;
            write!(f, "{}", FunctionDisplay(&o[function], o))
        } else if let Ok(v) = v.try_as_class() {
            write!(f, "{}", &o[o[v].name])
        } else if let Ok(v) = v.try_as_instance() {
            let class = o[v].class;
            write!(f, "{} instance", &o[o[class].name])
        } else if let Ok(v) = v.try_as_method() {
            let method = o[v].method;
            let function = o[method].function;
            write!(f, "{}", FunctionDisplay(&o[function], o))
        } else {
            write!(f, "<unknown>")
        }
    }
}

impl<'a> Debug for ValueDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let Self(value, heap) = self;
        if value.is_bool() {
            write!(f, "bool({})", value.as_bool())
        } else if value.is_nil() {
            write!(f, "nil")
        } else if value.is_number() {
            write!(f, "number({})", value.as_number())
        } else if let Ok(h) = value.try_as_string() {
            write!(f, "string(\"{}\")", &heap[h])
        } else if let Ok(h) = value.try_as_function() {
            write!(f, "function({})", FunctionDisplay(&heap[h], heap))
        } else if value.is_native() {
            write!(f, "function(<native fn>)")
        } else if let Ok(h) = value.try_as_closure() {
            write!(f, "closure({})", FunctionDisplay(&heap[heap[h].function], heap))
        } else if let Ok(h) = value.try_as_class() {
            write!(f, "class(\"{}\")", &heap[heap[h].name])
        } else if let Ok(h) = value.try_as_instance() {
            let class = heap[h].class;
            write!(f, "instance(of: \"{}\")", &heap[heap[class].name])
        } else if let Ok(h) = value.try_as_method() {
            let closure = heap[h].method;
            let func = heap[closure].function;
            write!(f, "method({})", FunctionDisplay(&heap[func], heap))
        } else {
            write!(f, "<unknown>")
        }
    }
}

#[derive(Debug)]
pub struct Function {
    pub arity: u8,
    pub chunk: Chunk,
    pub name: Option<GcHandle<String>>,
    pub kind: FunctionKind,
    // FIXME: u8?
    pub upvalue_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum FunctionKind {
    Function,
    Initializer,
    Script,
    Method,
}

impl Function {
    pub fn new(kind: FunctionKind) -> Self {
        Self {
            arity: 0,
            chunk: Chunk::new(),
            name: None,
            upvalue_count: 0,
            kind,
        }
    }
}

#[derive(Debug)]
pub struct Closure {
    pub function: GcHandle<Function>,
    pub upvalues: Vec<GcHandle<ObjUpvalue>>,
}

impl Closure {
    pub fn new(function: GcHandle<Function>) -> Self {
        Self {
            function,
            upvalues: Vec::new(),
        }
    }
}

#[derive(Ord, PartialOrd, PartialEq, Eq)]
pub struct Upvalue {
    pub index: u8,
    pub is_local: bool,
}

impl Upvalue {
    pub fn new(index: u8, is_local: bool) -> Self {
        Self { index, is_local }
    }
}

#[derive(Debug, Clone)]
pub enum ObjUpvalue {
    Open(usize),
    Closed(Value),
}

impl ObjUpvalue {
    #[allow(clippy::result_unit_err)]
    pub fn as_open(&self) -> Result<usize, ()> {
        if let Self::Open(slot) = self {
            Ok(*slot)
        } else {
            Err(())
        }
    }
}

#[derive(Debug)]
pub struct Class {
    pub name: GcHandle<String>,
    pub methods: HashMap<GcHandle<String>, Value>,
}

impl Class {
    pub fn new(name: GcHandle<String>) -> Self {
        Self {
            name,
            methods: HashMap::default(),
        }
    }
}

#[derive(Debug)]
pub struct Instance {
    pub class: GcHandle<Class>,
    pub fields: HashMap<GcHandle<String>, Value>,
}

impl Instance {
    pub fn new(class: GcHandle<Class>) -> Self {
        Self {
            class,
            fields: HashMap::default(),
        }
    }
}

#[derive(Debug)]
pub struct BoundMethod {
    pub receiver: Value,
    pub method: GcHandle<Closure>,
}

impl BoundMethod {
    pub fn new(receiver: Value, method: GcHandle<Closure>) -> Self {
        Self { receiver, method }
    }
}
