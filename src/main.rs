use std::path::Path;
use std::{env, process};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        if Path::new("pkg.toml").exists() {
            if let Err(err) = icoo_lang_r::run_project(".") {
                eprintln!("{}", err);
                process::exit(70);
            }
            return;
        }
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
            let path = expect_optional_path(args, ".");
            icoo_lang_r::run_path(path)
        }
        "check" => {
            let file = expect_single_file(args);
            icoo_lang_r::check_file(file)
        }
        "init" => {
            let dir = expect_optional_path(args, ".");
            match icoo_lang_r::init_project(&dir) {
                Ok(()) => {
                    println!("created Icoo project at {}", dir);
                    Ok(())
                }
                Err(err) => Err(err),
            }
        }
        file => {
            if args.next().is_some() {
                eprintln!("error: unexpected extra argument");
                print_usage_to_stderr();
                process::exit(64);
            }
            icoo_lang_r::run_path(file)
        }
    };

    if let Err(err) = result {
        eprintln!("{}", err);
        process::exit(70);
    }
}

fn expect_optional_path(mut args: impl Iterator<Item = String>, default: &str) -> String {
    let path = args.next().unwrap_or_else(|| default.to_string());
    if args.next().is_some() {
        eprintln!("error: unexpected extra argument");
        print_usage_to_stderr();
        process::exit(64);
    }
    path
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
  icoo init [dir]
  icoo run [file.icoo|project_dir|pkg.toml]
  icoo check <file.icoo>
  icoo <file.icoo|project_dir|pkg.toml>
  icoo --help
  icoo --version

Commands:
  init    Create a new Icoo project scaffold
  run     Execute an Icoo source file or project
  check   Lex, parse, resolve, and typecheck without executing

Examples:
  icoo init my_app
  icoo run
  icoo run hello.icoo
  icoo check hello.icoo"
    );
}

fn print_usage_to_stderr() {
    eprintln!(
        "usage: icoo init [dir] | icoo run [file.icoo|project_dir|pkg.toml] | icoo check <file.icoo> | icoo <file.icoo|project_dir|pkg.toml>"
    );
}
