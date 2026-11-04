#![feature(test)]

extern crate test;

use test::Bencher;
use vm::vm::Vm;

macro_rules! program {
    ($name:ident, $file:literal) => {
        #[bench]
        fn $name(b: &mut Bencher) {
            let source = include_str!(concat!("programs/", $file));
            b.iter(|| {
                let mut vm = Vm::new();
                test::black_box(vm.run(source, None::<&mut String>).is_ok())
            });
        }
    };
}

// correctness smoke tests: every benchmark must run clean.
// (string_equality is excluded: it fails the constant limit by design,
// and zoo_batch needs a 10s wall clock, which benches can't scale.)
macro_rules! runs_clean {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            let mut vm = Vm::new();
            assert!(vm.run(include_str!(concat!("programs/", $file)), None::<&mut String>).is_ok());
        }
    };
}

program!(binary_trees, "binary_trees.lox");
program!(equality, "equality.lox");
program!(fib, "fib.lox");
program!(instantiation, "instantiation.lox");
program!(invocation, "invocation.lox");
program!(method_call, "method_call.lox");
program!(properties, "properties.lox");
program!(trees, "trees.lox");
program!(zoo, "zoo.lox");

runs_clean!(binary_trees_ok, "binary_trees.lox");
runs_clean!(equality_ok, "equality.lox");
runs_clean!(fib_ok, "fib.lox");
runs_clean!(instantiation_ok, "instantiation.lox");
runs_clean!(invocation_ok, "invocation.lox");
runs_clean!(method_call_ok, "method_call.lox");
runs_clean!(properties_ok, "properties.lox");
runs_clean!(trees_ok, "trees.lox");
runs_clean!(zoo_ok, "zoo.lox");
