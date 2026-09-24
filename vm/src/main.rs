use std::io::Write;
use std::{env, fs, io, process};
use vm::vm::Vm;

fn main() {
    let mut args = env::args();
    args.next();

    let mut vm = Vm::new();
    if let Err(()) = match (args.next(), args.next()) {
        (None, None) => run_prompt(&mut vm),
        (Some(filename), None) => run_file(&mut vm, &filename),
        _ => Err(()),
    } {
        process::exit(1);
    };
}

fn run_prompt(vm: &mut Vm) -> ! {
    loop {
        print!("> ");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let _ = vm.run(&input, None::<&mut String>);
    }
}

fn run_file(vm: &mut Vm, filename: &str) -> Result<(), ()> {
    struct Stdout(std::io::Stdout);
    impl std::fmt::Write for Stdout {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            std::io::Write::write_fmt(&mut self.0, format_args!("{s}"))
                .map_err(|_| std::fmt::Error)
        }
    }

    match fs::read_to_string(filename) {
        Ok(source) => {
            if std::env::var("LOX_DUMP").is_ok() {
                vm.run(&source, Some(&mut Stdout(std::io::stdout())))
            } else {
                vm.run(&source, None::<&mut String>)
            }
        }
        Err(e) => {
            eprintln!("{}", e.to_string().to_lowercase());
            Err(())
        }
    }
}
