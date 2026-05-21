use std::{env, fs, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: icoo <file.icoo>");
        process::exit(64);
    }

    let path = &args[1];
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("failed to read '{}': {}", path, err);
            process::exit(66);
        }
    };

    if let Err(err) = icoo_lang_r::run_source(&source) {
        eprintln!("{}", err);
        process::exit(70);
    }
}
