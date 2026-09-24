// TODO (speedup): clone and push the function instead of accessing it every time through the objects manager
// TODO: use infallible for `error`
use crate::chunk::{
    BoundMethod, Class, Closure, Function, FunctionDisplay, FunctionKind, Instance, ObjUpvalue,
    OpCode, Value, ValueDisplay,
};
use crate::compiler::Compiler;
use crate::gc::{Allocate, GcHandle, Heap, Mark, Trace};
use crate::lex::Lexer;
use crate::location::AtCoords;
use crate::{ArrayVec, RunError};
// use std::collections::HashMap;
use fxhash::FxHashMap as HashMap;

use std::fmt::Write as FmtWrite;
use std::io::{Stderr, Stdout, Write};
use std::time::UNIX_EPOCH;

const FRAMES_MAX: usize = 64;

struct CallFrame {
    closure: GcHandle<Closure>,
    ip: usize,
    base: usize,
}

impl CallFrame {
    fn new(closure: GcHandle<Closure>, base: usize) -> Self {
        Self {
            closure,
            base,
            ip: 0,
        }
    }
}

pub struct Vm<W = Stdout, E = Stderr> {
    frames: ArrayVec<CallFrame, FRAMES_MAX>,
    stack: ArrayVec<Value, { 64 * 256 }>,
    // name -> value
    globals: HashMap<GcHandle<String>, Value>,
    objects: Heap,
    open_upvalues: Vec<GcHandle<ObjUpvalue>>,
    init_string: GcHandle<String>,
    pub out: W,
    pub err: E,
}

fn dump_function<D: FmtWrite>(f: &Function, heap: &Heap, out: &mut D) {
    let _ = writeln!(out, "---{}---", FunctionDisplay(f, heap));
    let _ = f.chunk.disassemble(heap, out);
    for constant in &f.chunk.constants {
        if let Value::Function(h) = constant {
            dump_function(&heap[*h], heap, out);
        }
    }
}

impl<W: Write + Default, E: Write + Default> Default for Vm<W, E> {    fn default() -> Self {
        Self::with_outputs(W::default(), E::default())
    }
}

impl Vm<Stdout, Stderr> {
    pub fn new() -> Self {
        Self::with_outputs(std::io::stdout(), std::io::stderr())
    }
}

impl<W: Write, E: Write> Vm<W, E> {
    pub fn with_outputs(out: W, err: E) -> Self {
        let mut objects = Heap::default();
        let init_string = objects.alloc("init".into());
        let mut ret = Self {
            frames: ArrayVec::new(),
            stack: ArrayVec::new(),
            globals: HashMap::default(),
            objects,
            open_upvalues: Vec::new(),
            init_string,
            out,
            err,
        };
        ret.define_native("clock", 0, |_| {
            Value::Number(UNIX_EPOCH.elapsed().unwrap().as_secs_f64())
        });
        ret
    }

    pub fn define_native(&mut self, name: &str, arity: u8, f: fn(&[Value]) -> Value) {
        let name = self.alloc(name.to_owned());
        self.stack.push(Value::String(name));
        self.globals
            .insert(name, Value::NativeFunction { arity, f });
        self.stack.pop();
    }

    fn call_closure(
        &mut self,
        closure: GcHandle<Closure>,
        arg_count: u8,
    ) -> Result<(), AtCoords<RunError>> {
        // TODO: possibly pass base instead of arg_count?
        let c = &self.objects[closure];
        let arity = self.objects[c.function].arity;
        if arity != arg_count {
            self.error(RunError::WrongArity(arity, arg_count))?;
        }
        if self.frames.len() >= FRAMES_MAX {
            self.error(RunError::StackOverflow)?;
        }
        // FIXME: repeated operation
        let base = self.stack.len() - 1 - arg_count as usize;
        self.frames.push(CallFrame::new(closure, base));
        Ok(())
    }

    fn call_value(&mut self, arg_count: u8) -> Result<(), AtCoords<RunError>> {
        let base = self.stack.len() - 1 - arg_count as usize;
        match self.stack[base] {
            Value::NativeFunction { arity, f } => {
                if arity != arg_count {
                    self.error(RunError::WrongArity(arity, arg_count))?;
                }
                let result = f(&self.stack[base + 1..]);
                // TODO: use `Vec::truncate`
                for _ in 0..arg_count + 1 {
                    self.stack.pop();
                }
                self.stack.push(result);
                Ok(())
            }
            Value::Closure(c) => self.call_closure(c, arg_count),
            Value::Class(c) => {
                let instance = self.alloc(Instance::new(c));
                let base = self.stack.len() - 1 - arg_count as usize;
                self.stack[base] = Value::Instance(instance);
                let class = &self.objects[c];
                if let Some(initializer) = class.methods.get(&self.init_string) {
                    self.call_closure(initializer.try_as_closure().unwrap(), arg_count)
                } else if arg_count != 0 {
                    self.error(RunError::WrongArity(0, arg_count))
                } else {
                    Ok(())
                }
            }
            Value::Method(m) => {
                let i = self.objects[m].receiver;
                let c = self.objects[m].method;
                let base = self.stack.len() - 1 - arg_count as usize;
                self.stack[base] = i;
                self.call_closure(c, arg_count)
            }
            _ => self.error(RunError::NotCallable),
        }
    }

    fn invoke_from_class(&mut self, class: GcHandle<Class>, name: GcHandle<String>, arg_count: u8) -> Result<(), AtCoords<RunError>> {
        let class = &self.objects[class];
        match class.methods.get(&name) {
            Some(m) => Ok(self.call_closure(m.try_as_closure().unwrap(), arg_count)?),
            None => {
                let name = &self.objects[name];
                self.error(RunError::UndefinedProperty(name.into()))
            }
        }
    }

    fn bind_method(
        &mut self,
        class: GcHandle<Class>,
        name: GcHandle<String>,
    ) -> Result<(), AtCoords<RunError>> {
        let c = &self.objects[class];
        if let Some(method) = c.methods.get(&name) {
            let bound = BoundMethod::new(
                self.stack[self.stack.len() - 1],
                method.try_as_closure().unwrap(),
            );
            let bound = self.alloc(bound);
            self.stack.pop();
            self.stack.push(Value::Method(bound));
            Ok(())
        } else {
            let name = &self.objects[name];
            self.error(RunError::UndefinedProperty(name.into()))
        }
    }

    // unit Err: errors are already reported with locations
    #[allow(clippy::result_unit_err)]
    pub fn run<D: FmtWrite>(&mut self, source: &str, disasm: Option<&mut D>) -> Result<(), ()> {
        // clear previous junk
        // FIXME: check if needs optimization
        self.stack = ArrayVec::new();
        self.frames = ArrayVec::new();

        let mut lexer = Lexer::new(source).peekable();
        let function = {
            let mut compiler = Compiler::new(
                &mut lexer,
                &mut self.objects,
                FunctionKind::Script,
                &mut self.err,
            );
            match compiler.compile() {
                Ok(function) => function,
                Err(()) => return Err(()),
            }
        };
        if let Some(out) = disasm {
            dump_function(&function, &self.objects, out);
        }
        let function = self.alloc(function);
        let closure = self.alloc(Closure::new(function));

        self.frames.push(CallFrame::new(closure, 0));
        self.stack.push(Value::Closure(closure));
        self.execute().map_err(|e| {
            let _ = writeln!(self.err, "{e}");
        })
    }

    fn current_frame(&mut self) -> &mut CallFrame {
        self.frames.last_mut().unwrap()
    }

    fn error(&mut self, error: RunError) -> Result<(), AtCoords<RunError>> {
        // frame.ip is one past the failing instruction; back up onto it
        let ip = self.current_frame().ip.saturating_sub(1);
        let closure = self.current_frame().closure;
        let function = &self.objects[closure].function;
        let function = &self.objects[*function];
        Err(function.chunk.coords(ip).locate(error))
    }

    fn is_falsey(value: Value) -> bool {
        match value {
           Value::Nil => true,
            Value::Bool(b) => !b,
            _ => false,
        }
    }

    fn execute(&mut self) -> Result<(), AtCoords<RunError>> {
        loop {
            macro_rules! read_byte {
                () => {{
                    let frame = self.frames.last_mut().unwrap();
                    let closure = &self.objects[frame.closure];
                    let function = &self.objects[closure.function];
                    let ip = frame.ip;
                    let ret = function.chunk.byte_at(ip);
                    frame.ip += 1;
                    ret
                }};
            }
            macro_rules! read_wide {
                () => {
                    u16::from_be_bytes([read_byte!(), read_byte!()])
                };
            }
            macro_rules! read_constant {
                () => {{
                    let frame = self.frames.last_mut().unwrap();
                    let closure = &self.objects[frame.closure];
                    let function = &self.objects[closure.function];
                    // let index = read_byte!();
                    function.chunk.get_constant(read_byte!())
                }};
            }
            // TODO: add macro for binary expressions
            match read_byte!().try_into().unwrap() {
                OpCode::Return => {
                    let result = self.stack.pop().unwrap();
                    let frame = self.frames.pop().unwrap();
                    self.close_upvalues(frame.base);
                    if self.frames.is_empty() {
                        self.stack.pop();
                        return Ok(());
                    }

                    for _ in frame.base..self.stack.len() {
                        self.stack.pop();
                    }
                    self.stack.push(result);
                }
                OpCode::Constant => {
                    let constant = read_constant!();
                    self.stack.push(*constant);
                }
                OpCode::Add => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => {
                            self.stack.push(Value::Number(a + b))
                        }
                        (Value::String(a), Value::String(b)) => {
                            let value = self.objects[a].to_owned() + &self.objects[b];
                            let value = self.alloc(value);
                            self.stack.push(Value::String(value));
                        }
                        _ => return self.error(RunError::ExpectedNumbersOrStrings),
                    }
                }
                OpCode::Subtract => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => {
                            self.stack.push(Value::Number(a - b))
                        }
                        _ => return self.error(RunError::ExpectedNumbers),
                    }
                }
                OpCode::Multiply => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => {
                            self.stack.push(Value::Number(a * b))
                        }
                        _ => return self.error(RunError::ExpectedNumbers),
                    }
                }
                OpCode::Divide => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => {
                            self.stack.push(Value::Number(a / b))
                        }
                        _ => return self.error(RunError::ExpectedNumbers),
                    }
                }
                OpCode::Negate => match self.stack.pop().unwrap() {
                    Value::Number(n) => self.stack.push(Value::Number(-n)),
                    _ => self.error(RunError::ExpectedNumber)?,
                },
                OpCode::Nil => {
                    self.stack.push(Value::Nil);
                }
                OpCode::True => {
                    self.stack.push(Value::Bool(true));
                }
                OpCode::False => {
                    self.stack.push(Value::Bool(false));
                }
                OpCode::Not => {
                    let value = Self::is_falsey(self.stack.pop().unwrap());
                    self.stack.push(Value::Bool(value));
                }
                OpCode::Equal => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    // TODO: implement separate function
                    self.stack.push(Value::Bool(match (a, b) {
                        (Value::Bool(a), Value::Bool(b)) => a == b,
                        (Value::Nil, Value::Nil) => true,
                        (Value::Number(a), Value::Number(b)) => a == b,
                        (Value::String(a), Value::String(b)) => a == b,
                        (Value::Function(a), Value::Function(b)) => a == b,
                        (Value::Closure(a), Value::Closure(b)) => a == b,
                        (Value::Class(a), Value::Class(b)) => a == b,
                        (Value::Instance(a), Value::Instance(b)) => a == b,
                        (Value::Method(a), Value::Method(b)) => a == b,
                        (
                            Value::NativeFunction { arity: arity_a, f: f_a },
                            Value::NativeFunction { arity: arity_b, f: f_b },
                        ) => {
                            arity_a == arity_b
                                && std::ptr::fn_addr_eq(f_a, f_b)
                        }
                        _ => false,
                    }));
                }
                OpCode::Greater => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => self.stack.push(Value::Bool(a > b)),
                        _ => return self.error(RunError::ExpectedNumbers),
                    }
                }
                OpCode::Less => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    match (a, b) {
                        (Value::Number(a), Value::Number(b)) => self.stack.push(Value::Bool(a < b)),
                        _ => return self.error(RunError::ExpectedNumbers),
                    }
                }
                OpCode::Print => {
                    let value = *self.stack.last().unwrap();
                    let _ = writeln!(self.out, "{}", ValueDisplay(&value, &self.objects));
                    self.stack.pop();
                }
                OpCode::Pop => {
                    self.stack.pop();
                }
                OpCode::DefineGlobal => {
                    let name = read_constant!().try_as_string().unwrap();
                    let value = self.stack.pop().unwrap();
                    self.globals.insert(name, value);
                }
                OpCode::GetGlobal => {
                    let name = read_constant!().try_as_string().unwrap();
                    if let Some(value) = self.globals.get(&name) {
                        self.stack.push(*value);
                    } else {
                        let name = self.objects[name].into();
                        return self.error(RunError::UndefinedVariable(name));
                    }
                }
                OpCode::SetGlobal => {
                    let name = read_constant!().try_as_string().unwrap();
                    if let Some(value) = self.globals.get_mut(&name) {
                        let new_value = self.stack.last().unwrap();
                        *value = *new_value;
                    } else {
                        let name = self.objects[name].into();
                        return self.error(RunError::UndefinedVariable(name));
                    }
                }
                OpCode::GetLocal => {
                    let slot = read_byte!();
                    let slot = self.current_frame().base + slot as usize;
                    let value = self.stack[slot];
                    self.stack.push(value);
                }
                OpCode::SetLocal => {
                    let slot = read_byte!();
                    let slot = self.current_frame().base + slot as usize;
                    let value = self.stack.last().unwrap();
                    self.stack[slot] = *value;
                }
                OpCode::JumpIfFalse => {
                    let offset = u16::from_be_bytes([read_byte!(), read_byte!()]);
                    if Self::is_falsey(*self.stack.last().unwrap()) {
                        self.frames.last_mut().unwrap().ip += offset as usize;
                    }
                }
                OpCode::Jump => {
                    let offset = read_wide!();
                    self.current_frame().ip += offset as usize;
                }
                OpCode::Loop => {
                    let offset = read_wide!();
                    self.current_frame().ip -= offset as usize;
                }
                OpCode::Call => {
                    let arg_count = read_byte!();
                    self.call_value(arg_count)?
                }
                OpCode::Closure => {
                    let function = read_constant!().try_as_function().unwrap();
                    let closure = Closure::new(function);
                    let closure_obj = self.alloc(closure);
                    self.stack.push(Value::Closure(closure_obj));

                    let closure = &self.objects[closure_obj];
                    let upvalue_count = self.objects[closure.function].upvalue_count;
                    for _ in 0..upvalue_count {
                        let is_local = read_byte!();
                        let index = read_byte!();
                        if is_local == 1 {
                            let slot = self.current_frame().base + index as usize;
                            let upvalue = self.capture_upvalue(slot);
                            let closure = &mut self.objects[closure_obj];
                            closure.upvalues.push(upvalue);
                        } else {
                            let current_closure_obj = self.current_frame().closure;
                            let current_closure = &self.objects[current_closure_obj];
                            let upvalue = current_closure.upvalues[index as usize];
                            let closure = &mut self.objects[closure_obj];
                            closure.upvalues.push(upvalue);
                        }
                    }
                }
                OpCode::GetUpvalue => {
                    let slot = read_byte!();
                    let closure = self.current_frame().closure;
                    let closure = &self.objects[closure];
                    let value = self.get_upvalue(closure.upvalues[slot as usize]);
                    self.stack.push(value);
                }
                OpCode::SetUpvalue => {
                    let slot = read_byte!();
                    let closure = self.current_frame().closure;
                    let closure = &self.objects[closure];
                    let upvalue = closure.upvalues[slot as usize];
                    self.set_upvalue(upvalue);
                }
                OpCode::CloseUpvalue => {
                    let top = self.stack.len() - 1;
                    self.close_upvalues(top);
                    self.stack.pop();
                }
                OpCode::Class => {
                    let name = read_constant!().try_as_string().unwrap();
                    let class = self.alloc(Class::new(name));
                    self.stack.push(Value::Class(class));
                }
                OpCode::GetProperty => {
                    let value = self.stack.last().unwrap();
                    let Ok(instance) = value.try_as_instance() else {
                        let v = ValueDisplay(value, &self.objects).to_string();
                        return self.error(RunError::NotAnInstance(v));
                    };
                    let instance = &self.objects[instance];
                    let name = read_constant!().try_as_string().unwrap();
                    if let Some(value) = instance.fields.get(&name) {
                        self.stack.pop();
                        self.stack.push(*value);
                    } else {
                        self.bind_method(instance.class, name)?
                    }
                }
                OpCode::SetProperty => {
                    let i = self.stack.len() - 2;
                    let value = self.stack[i];
                    let Ok(instance) = value.try_as_instance() else {
                        let v = ValueDisplay(&value, &self.objects).to_string();
                        return self.error(RunError::NotAnInstance(v));
                    };
                    let name = read_constant!().try_as_string().unwrap();
                    let instance = &mut self.objects[instance];
                    let value = self.stack.pop().unwrap();
                    instance.fields.insert(name, value);
                    // removing the instance
                    self.stack.pop();
                    self.stack.push(value);
                }
                OpCode::Method => {
                    let name = read_constant!().try_as_string().unwrap();
                    self.define_method(name);
                }
                OpCode::Invoke => {
                    let method = read_constant!().try_as_string().unwrap();
                    let arg_count = read_byte!() as usize;
                    let base = self.stack.len() - 1 - arg_count;
                    let receiver = &self.stack[base];
                    let Value::Instance(instance) = receiver else {
                        let v = ValueDisplay(receiver, &self.objects).to_string();
                        return self.error(RunError::NotAnInstance(v))
                    };
                    let instance = &self.objects[*instance];
                    if let Some(m) = instance.fields.get(&method) {
                        self.stack[base] = *m;
                        self.call_value(arg_count as u8)
                    } else {
                        self.invoke_from_class(instance.class, method, arg_count as u8)
                    }?;
                }
                OpCode::Inherit => {
                    let top = self.stack.len() - 1;
                    let Ok(superclass) = self.stack[top - 1].try_as_class() else {
                        return self.error(RunError::UninheritableValue);
                    };
                    let superclass = &self.objects[superclass];
                    let it = superclass.methods.clone();
                    let subclass = &self.stack[top].try_as_class().unwrap();
                    let subclass = &mut self.objects[*subclass];
                    subclass.methods.extend(it);

                    self.stack.pop();
                }
                OpCode::GetSuper => {
                    let name = read_constant!().try_as_string().unwrap();
                    let superclass = self.stack.pop().unwrap().try_as_class().unwrap();
                    self.bind_method(superclass, name)?
                }
                OpCode::SuperInvoke => {
                    let method = read_constant!().try_as_string().unwrap();
                    let arg_count = read_byte!();
                    let superclass = self.stack.pop().unwrap().try_as_class().unwrap();
                    self.invoke_from_class(superclass, method, arg_count)?;

                }
            }
        }
    }

    fn close_upvalues(&mut self, last: usize) {
        // FIXME: ugly
        let mut top = self.open_upvalues.len();
        for upvalue in self.open_upvalues.iter().rev() {
            let upvalue = &mut self.objects[*upvalue];
            let slot = upvalue.as_open().unwrap();
            if slot < last {
                break;
            }
            let value = self.stack[slot];
            *upvalue = ObjUpvalue::Closed(value);
            top -= 1;
        }
        self.open_upvalues.truncate(top);
    }

    fn define_method(&mut self, name: GcHandle<String>) {
        let method = self.stack.pop().unwrap();
        let class = self.stack[self.stack.len() - 1].try_as_class().unwrap();
        let class = &mut self.objects[class];
        class.methods.insert(name, method);
    }

    // FIXME: ugly
    fn capture_upvalue(&mut self, slot: usize) -> GcHandle<ObjUpvalue> {
        for (i, upvalue_obj) in self.open_upvalues.iter().enumerate().rev() {
            let upvalue = &self.objects[*upvalue_obj].as_open().unwrap();
            if *upvalue == slot {
                return *upvalue_obj;
            } else if *upvalue < slot {
                let upvalue = ObjUpvalue::Open(slot);
                let upvalue = self.alloc(upvalue);
                self.open_upvalues.insert(i + 1, upvalue);
                return upvalue;
            }
        }
        let upvalue = ObjUpvalue::Open(slot);
        let upvalue = self.alloc(upvalue);
        self.open_upvalues.push(upvalue);
        upvalue
    }

    fn get_upvalue(&self, upvalue: GcHandle<ObjUpvalue>) -> Value {
        let upvalue = &self.objects[upvalue];
        match upvalue {
            ObjUpvalue::Open(slot) => self.stack[*slot],
            ObjUpvalue::Closed(value) => *value,
        }
    }

    fn set_upvalue(&mut self, upvalue: GcHandle<ObjUpvalue>) {
        let upvalue = &mut self.objects[upvalue];
        let update = self.stack.last().unwrap();
        match upvalue {
            ObjUpvalue::Open(slot) => self.stack[*slot] = *update,
            ObjUpvalue::Closed(ref mut value) => *value = *update,
        }
    }

    fn mark_roots(&self) {
        for slot in &*self.stack {
            slot.trace(&self.objects);
        }
        for frame in &*self.frames {
            let _ = self.objects.mark(frame.closure);
        }
        for upvalue in &*self.open_upvalues {
            self.objects.mark(*upvalue);
        }
        for (k, v) in &self.globals {
            self.objects.mark(*k);
            v.trace(&self.objects);
        }
        self.objects.mark(self.init_string);

        // don't care about compiler's temporary object, only collect at runtime
    }

    fn alloc<T>(&mut self, value: T) -> GcHandle<T>
    where
        Heap: Allocate<T>,
    {
        if self.objects.should_sweep() {
            self.mark_roots();
            self.objects.sweep();
        }
        self.objects.alloc(value)
    }
}
