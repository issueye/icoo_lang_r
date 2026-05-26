use std::path::Path;
use std::{env, process};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    match icoo_lang_r::run_embedded_package() {
        Ok(Some(())) => return,
        Ok(None) => {}
        Err(err) => {
            eprintln!("{}", err);
            process::exit(70);
        }
    }

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
        "package" | "pack" => {
            let (input, output, standalone) = parse_package_args(args);
            match icoo_lang_r::package_path(icoo_lang_r::PackageOptions {
                input,
                output,
                standalone,
            }) {
                Ok(result) => {
                    if standalone {
                        println!(
                            "packaged {} files into standalone binary {}",
                            result.files,
                            result.output.display()
                        );
                    } else {
                        println!(
                            "packaged {} files into {}",
                            result.files,
                            result.output.display()
                        );
                    }
                    Ok(())
                }
                Err(err) => Err(err),
            }
        }
        "init" => {
            let (dir, init_options) = parse_init_args(args);
            match icoo_lang_r::init_project_with_options(&dir, init_options) {
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

fn parse_init_args(
    args: impl Iterator<Item = String>,
) -> (String, icoo_lang_r::InitProjectOptions) {
    let mut dir: Option<String> = None;
    let mut build_scripts = false;
    for arg in args {
        match arg.as_str() {
            "--build-scripts" | "--build-script" | "--scripts" => {
                build_scripts = true;
            }
            _ if arg.starts_with('-') => {
                eprintln!("error: unknown init option '{}'", arg);
                print_usage_to_stderr();
                process::exit(64);
            }
            _ => {
                if dir.replace(arg).is_some() {
                    eprintln!("error: unexpected extra argument");
                    print_usage_to_stderr();
                    process::exit(64);
                }
            }
        }
    }
    (
        dir.unwrap_or_else(|| ".".to_string()),
        icoo_lang_r::InitProjectOptions { build_scripts },
    )
}

fn parse_package_args(
    args: impl Iterator<Item = String>,
) -> (std::path::PathBuf, Option<std::path::PathBuf>, bool) {
    let mut input: Option<std::path::PathBuf> = None;
    let mut output: Option<std::path::PathBuf> = None;
    let mut standalone = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--standalone" | "--exe" => {
                standalone = true;
            }
            "-o" | "--output" => {
                let Some(path) = args.next() else {
                    eprintln!("error: missing output path after {}", arg);
                    print_usage_to_stderr();
                    process::exit(64);
                };
                if output.replace(std::path::PathBuf::from(path)).is_some() {
                    eprintln!("error: package output specified more than once");
                    print_usage_to_stderr();
                    process::exit(64);
                }
            }
            _ if arg.starts_with('-') => {
                eprintln!("error: unknown package option '{}'", arg);
                print_usage_to_stderr();
                process::exit(64);
            }
            _ => {
                if input.replace(std::path::PathBuf::from(arg)).is_some() {
                    eprintln!("error: package input specified more than once");
                    print_usage_to_stderr();
                    process::exit(64);
                }
            }
        }
    }
    (
        input.unwrap_or_else(|| std::path::PathBuf::from(".")),
        output,
        standalone,
    )
}

fn print_help() {
    println!(
        "icoo {VERSION}

Usage:
  icoo init [dir] [--build-scripts]
  icoo run [file.icoo|project_dir|pkg.toml]
  icoo check <file.icoo>
  icoo package [file.icoo|project_dir|pkg.toml] [-o output.zip] [--standalone]
  icoo <file.icoo|project_dir|pkg.toml>
  icoo --help
  icoo --version

Commands:
  init    Create a new Icoo project scaffold
  run     Execute an Icoo source file or project
  check   Lex, parse, resolve, and typecheck without executing
  package Create a distributable .icoo.zip package or standalone binary

Examples:
  icoo init my_app
  icoo init my_app --build-scripts
  icoo run
  icoo run hello.icoo
  icoo check hello.icoo
  icoo package my_app -o dist/my_app.icoo.zip
  icoo package my_app --standalone -o dist/my_app.exe"
    );
}

fn print_usage_to_stderr() {
    eprintln!(
        "usage: icoo init [dir] [--build-scripts] | icoo run [file.icoo|project_dir|pkg.toml] | icoo check <file.icoo> | icoo package [file.icoo|project_dir|pkg.toml] [-o output.zip] [--standalone] | icoo <file.icoo|project_dir|pkg.toml>"
    );
}
