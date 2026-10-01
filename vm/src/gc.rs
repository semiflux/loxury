use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};
use std::{/*collections::HashMap,*/ mem};
use fxhash::{FxHashMap as HashMap};

use crate::chunk::{
    BoundMethod, Class, Closure, Function, Instance, Native, ObjUpvalue, Value,
};

// arena id rides in the top byte of every handle (MSB mask), so a
// handle alone identifies both the arena and the slot. nan-boxed
// `Value`s store exactly this pair as their object payload.
pub const TAG_FUNCTION: u8 = 1;
pub const TAG_UPVALUE: u8 = 2;
pub const TAG_CLOSURE: u8 = 3;
pub const TAG_CLASS: u8 = 4;
pub const TAG_INSTANCE: u8 = 5;
pub const TAG_BOUND_METHOD: u8 = 6;
pub const TAG_STRING: u8 = 7;
pub const TAG_NATIVE: u8 = 8;

const TAG_SHIFT: u32 = usize::BITS - 8;
const TAG_MASK: usize = 0xFFusize << TAG_SHIFT;
const INDEX_MASK: usize = !TAG_MASK;

#[derive(Default)]
struct Interner {
    map: HashMap<&'static str, u32>,
    vec: Vec<&'static str>,
    buf: String,
    full: Vec<String>,
}

impl Interner {
    pub fn intern(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.map.get(name) {
            return id;
        }
        let name = unsafe { self.alloc(name) };
        let id = self.map.len() as u32;
        self.map.insert(name, id);
        self.vec.push(name);
        debug_assert!(self.lookup(id) == name);
        debug_assert!(self.intern(name) == id);
        id
    }

    pub fn lookup(&self, id: u32) -> &str {
        self.vec[id as usize]
    }

    unsafe fn alloc(&mut self, name: &str) -> &'static str {
        let cap = self.buf.capacity();
        if cap < self.buf.len() + name.len() {
            let new_cap = (cap.max(name.len()) + 1).next_power_of_two();
            let new_buf = String::with_capacity(new_cap);
            let old_buf = mem::replace(&mut self.buf, new_buf);
            self.full.push(old_buf);
        }
        let interned = {
            let start = self.buf.len();
            self.buf.push_str(name);
            &self.buf[start..]
        };
        &*(interned as *const str)
    }
}

pub struct Arena<T> {
    objects: Vec<GcObject<T>>,
    live: Vec<bool>,
    recycle: Vec<usize>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            objects: vec![],
            live: vec![],
            recycle: vec![],
        }
    }
}

#[derive(Debug)]
struct GcObject<T> {
    value: T,
    marked: Cell<bool>,
}

#[derive(Eq, Debug)]
pub struct GcHandle<T> {
    idx: usize,
    _type: PhantomData<*mut T>,
}

impl<T> PartialEq for GcHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx
    }
}

impl<T> Hash for GcHandle<T> {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.idx.hash(hasher)
    }
}

impl<T> GcHandle<T> {
    pub(crate) fn tagged(tag: u8, index: usize) -> Self {
        debug_assert!(index <= INDEX_MASK, "handle index overflow");
        Self {
            idx: ((tag as usize) << TAG_SHIFT) | index,
            _type: PhantomData,
        }
    }

    pub fn tag(&self) -> u8 {
        (self.idx >> TAG_SHIFT) as u8
    }

    pub fn index(&self) -> usize {
        self.idx & INDEX_MASK
    }
}

impl<T> Clone for GcHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for GcHandle<T> {}

pub trait Allocate<T> {
    fn alloc(&mut self, value: T) -> GcHandle<T>;
}

pub trait Mark<T> {
    fn mark(&self, value: GcHandle<T>) -> bool;
}

pub trait _Allocate {
    type Item;
    // raw slot index; the `Heap` wrapper tags it on the way out
    fn alloc(&mut self, value: Self::Item) -> (usize, usize);
}

macro_rules! define_heap {
    ($name:ident { $($arena:ident: $ty:ty = $tag:expr),* $(,)? }) => {
        // #[derive(Default)]
        pub struct $name {
            $($arena: $ty,)*
            bytes_allocated: usize,
            next_gc: usize,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    $($arena: <$ty>::default(),)*
                    bytes_allocated: 0,
                    next_gc: 1024 * 1024,
                }
            }
        }

        $(
            impl Mark<<$ty as _Allocate>::Item> for $name {
                fn mark(&self, handle: GcHandle<<$ty as _Allocate>::Item>) -> bool {
                    if !self.$arena.mark(handle) {
                        let obj = &self.$arena[handle];
                        obj.trace(&self);
                    }
                    true
                }
            }

            impl Allocate<<$ty as _Allocate>::Item> for $name {
                fn alloc(&mut self, value: <$ty as _Allocate>::Item) -> GcHandle<<$ty as _Allocate>::Item> {
                    let (index, bytes) = self.$arena.alloc(value);
                    self.bytes_allocated += bytes;
                    GcHandle::tagged($tag, index)
                }
            }

            impl Index<GcHandle<<$ty as _Allocate>::Item>> for $name {
                type Output = <$ty as Index<GcHandle<<$ty as _Allocate>::Item>>>::Output;
                fn index(&self, idx: GcHandle<<$ty as _Allocate>::Item>) -> &Self::Output {
                    debug_assert_eq!(idx.tag(), $tag, "handle/arena mismatch");
                    &self.$arena[idx]
                }
            }

            impl IndexMut<GcHandle<<$ty as _Allocate>::Item>> for $name {
                fn index_mut(&mut self, idx: GcHandle<<$ty as _Allocate>::Item>) -> &mut Self::Output {
                    debug_assert_eq!(idx.tag(), $tag, "handle/arena mismatch");
                    &mut self.$arena[idx]
                }
            }
        )*
    };
}

define_heap!(Heap {
    arena_function: Arena<Function> = TAG_FUNCTION,
    arena_upvalue:  Arena<ObjUpvalue> = TAG_UPVALUE,
    arena_closure:  Arena<Closure> = TAG_CLOSURE,
    arena_class: Arena<Class> = TAG_CLASS,
    arena_instance: Arena<Instance> = TAG_INSTANCE,
    arena_bound_method: Arena<BoundMethod> = TAG_BOUND_METHOD,
    arena_string:   StringArena = TAG_STRING,
    arena_native: Arena<Native> = TAG_NATIVE,
});

impl Heap {
    // TODO: move these inside macro
    pub fn sweep(&mut self) {
        let mut freed = 0;
        freed += self.arena_function.sweep();
        freed += self.arena_upvalue.sweep();
        freed += self.arena_closure.sweep();
        freed += self.arena_class.sweep();
        freed += self.arena_instance.sweep();
        freed += self.arena_bound_method.sweep();
        freed += self.arena_native.sweep();
        self.bytes_allocated -= freed;
        self.next_gc = (2 * self.bytes_allocated).max(1024 * 256);
    }

    pub fn should_sweep(&self) -> bool {
        self.bytes_allocated > self.next_gc
    }
}

impl<T> _Allocate for Arena<T> {
    type Item = T;

    fn alloc(&mut self, value: T) -> (usize, usize) {
        let pos = if let Some(pos) = self.recycle.pop() {
            self.objects[pos] = GcObject {
                value,
                marked: false.into(),
            };
            self.live[pos] = true;
            pos
        } else {
            let pos = self.objects.len();
            self.objects.push(GcObject {
                value,
                marked: false.into(),
            });
            self.live.push(true);
            pos
        };
        (pos, mem::size_of::<T>())
    }
}

impl<T> Arena<T> {
    fn sweep(&mut self) -> usize {
        let mut freed = 0;
        for i in 0..self.live.len() {
            if !self.live[i] {
                continue;
            }
            if self.objects[i].marked.get() {
                self.objects[i].marked.set(false);
            } else {
                self.live[i] = false;
                self.recycle.push(i);
                freed += mem::size_of::<T>();
            }
        }
        freed
    }
}

impl<T> Mark<T> for Arena<T> {
    fn mark(&self, idx: GcHandle<T>) -> bool {
        let idx = idx.index();
        debug_assert!(self.live[idx], "attempted to mark a freed slot");
        if self.objects[idx].marked.get() {
            return true;
        }
        self.objects[idx].marked.set(true);
        false
    }
}

impl<T> Index<GcHandle<T>> for Arena<T> {
    type Output = T;
    fn index(&self, idx: GcHandle<T>) -> &T {
        let idx = idx.index();
        debug_assert!(self.live[idx], "use-after-free");
        &self.objects[idx].value
    }
}

impl<T> IndexMut<GcHandle<T>> for Arena<T> {
    fn index_mut(&mut self, idx: GcHandle<T>) -> &mut T {
        let idx = idx.index();
        debug_assert!(self.live[idx], "use-after-free");
        &mut self.objects[idx].value
    }
}

#[derive(Default)]
pub struct StringArena {
    interner: Interner,
}

impl _Allocate for StringArena {
    type Item = String;
    fn alloc(&mut self, value: Self::Item) -> (usize, usize) {
        let index = self.interner.intern(&value);
        (index as usize, value.len())
    }
}

impl Index<GcHandle<String>> for StringArena {
    type Output = str;
    fn index(&self, index: GcHandle<String>) -> &Self::Output {
        self.interner.lookup(index.index() as u32)
    }
}

impl IndexMut<GcHandle<String>> for StringArena {
    fn index_mut(&mut self, _: GcHandle<String>) -> &mut Self::Output {
        panic!()
    }
}

impl Mark<String> for StringArena {
    fn mark(&self, _: GcHandle<String>) -> bool {
        true
    }
}

pub trait Trace {
    fn trace(&self, _heap: &Heap) {}
}

impl Trace for &str {}

impl Trace for Function {
    fn trace(&self, heap: &Heap) {
        if let Some(name) = self.name {
            heap.mark(name);
        }
        for v in &self.chunk.constants {
            v.trace(heap);
        }
    }
}

impl Trace for ObjUpvalue {
    fn trace(&self, heap: &Heap) {
        if let Self::Closed(v) = self {
            v.trace(heap);
        }
    }
}

impl Trace for Closure {
    fn trace(&self, heap: &Heap) {
        heap.mark(self.function);
        for v in &self.upvalues {
            heap.mark(*v);
        }
    }
}

impl Trace for Class {
    fn trace(&self, heap: &Heap) {
        heap.mark(self.name);
        for (k, v) in self.methods.iter() {
            heap.mark(*k);
            v.trace(heap);
        }
    }
}

impl Trace for Instance {
    fn trace(&self, heap: &Heap) {
        heap.mark(self.class);
        for (k, v) in self.fields.iter() {
            heap.mark(*k);
            v.trace(heap);
        }
    }
}

impl Trace for Value {
    // shared by both representations; every object kind marks its arena
    fn trace(&self, heap: &Heap) {
        if let Ok(v) = self.try_as_string() {
            heap.mark(v);
        } else if let Ok(v) = self.try_as_function() {
            heap.mark(v);
        } else if let Ok(v) = self.try_as_native() {
            heap.mark(v);
        } else if let Ok(v) = self.try_as_closure() {
            heap.mark(v);
        } else if let Ok(v) = self.try_as_class() {
            heap.mark(v);
        } else if let Ok(v) = self.try_as_instance() {
            heap.mark(v);
        } else if let Ok(v) = self.try_as_method() {
            heap.mark(v);
        }
    }
}

impl Trace for Native {
    fn trace(&self, _heap: &Heap) {}
}

impl Trace for BoundMethod {
    fn trace(&self, heap: &Heap) {
        self.receiver.trace(heap);
        heap.mark(self.method);
    }
}
