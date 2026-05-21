use std::{env, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: icoo <file.icoo>");
        process::exit(64);
    }

    if let Err(err) = icoo_lang_r::run_file(&args[1]) {
        eprintln!("{}", err);
        process::exit(70);
    }
}
