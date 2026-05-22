use std::{env, process};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage_to_stderr();
        process::exit(64);
    };

    let result = match command.as_str() {
        "--help" | "-h" => {
            if args.next().is_some() {
                eprintln!("error: --help does not accept arguments");
                print_usage_to_stderr();
                process::exit(64);
            }
            print_help();
            return;
        }
        "--version" | "-V" => {
            if args.next().is_some() {
                eprintln!("error: --version does not accept arguments");
                print_usage_to_stderr();
                process::exit(64);
            }
            println!("icoo {}", VERSION);
            return;
        }
        "run" => {
            let file = expect_single_file(args);
            icoo_lang_r::run_file(file)
        }
        "check" => {
            let file = expect_single_file(args);
            icoo_lang_r::check_file(file)
        }
        file => {
            if args.next().is_some() {
                eprintln!("error: unexpected extra argument");
                print_usage_to_stderr();
                process::exit(64);
            }
            icoo_lang_r::run_file(file)
        }
    };

    if let Err(err) = result {
        eprintln!("{}", err);
        process::exit(70);
    }
}

fn expect_single_file(mut args: impl Iterator<Item = String>) -> String {
    let Some(file) = args.next() else {
        eprintln!("error: missing <file.icoo>");
        print_usage_to_stderr();
        process::exit(64);
    };
    if args.next().is_some() {
        eprintln!("error: unexpected extra argument");
        print_usage_to_stderr();
        process::exit(64);
    }
    file
}

fn print_help() {
    println!(
        "icoo {VERSION}

Usage:
  icoo run <file.icoo>
  icoo check <file.icoo>
  icoo <file.icoo>
  icoo --help
  icoo --version

Commands:
  run     Execute an Icoo source file
  check   Lex, parse, resolve, and typecheck without executing

Examples:
  icoo run hello.icoo
  icoo check hello.icoo"
    );
}

fn print_usage_to_stderr() {
    eprintln!("usage: icoo run <file.icoo> | icoo check <file.icoo> | icoo <file.icoo>");
}
