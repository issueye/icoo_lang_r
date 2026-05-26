use crate::error::{IcooError, IcooResult};
use crate::interpreter::Interpreter;
use std::path::{Path, PathBuf};

const DEFAULT_PKG_TOML: &str = r#"[package]
name = "icoo-app"
version = "0.1.0"

[run]
entry = "src/main.icoo"
"#;

const DEFAULT_MAIN: &str = r#"fn main() {
    print("hello from Icoo")
}
"#;

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub root: PathBuf,
    pub entry: PathBuf,
}

pub fn init_project(path: impl AsRef<Path>) -> IcooResult<()> {
    let root = path.as_ref();
    std::fs::create_dir_all(root).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to create project directory '{}': {}",
                root.display(),
                err
            ),
            None,
        )
    })?;

    let pkg_path = root.join("pkg.toml");
    if pkg_path.exists() {
        return Err(IcooError::runtime(
            format!("project config '{}' already exists", pkg_path.display()),
            None,
        ));
    }

    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to create source directory '{}': {}",
                src_dir.display(),
                err
            ),
            None,
        )
    })?;

    let main_path = src_dir.join("main.icoo");
    if main_path.exists() {
        return Err(IcooError::runtime(
            format!("entry file '{}' already exists", main_path.display()),
            None,
        ));
    }

    std::fs::write(&pkg_path, DEFAULT_PKG_TOML).map_err(|err| {
        IcooError::runtime(
            format!("failed to write '{}': {}", pkg_path.display(), err),
            None,
        )
    })?;
    std::fs::write(&main_path, DEFAULT_MAIN).map_err(|err| {
        IcooError::runtime(
            format!("failed to write '{}': {}", main_path.display(), err),
            None,
        )
    })?;

    Ok(())
}

pub fn resolve_project(path: impl AsRef<Path>) -> IcooResult<ProjectConfig> {
    let path = path.as_ref();
    let pkg_path = if path.is_dir() {
        path.join("pkg.toml")
    } else if path.file_name().and_then(|name| name.to_str()) == Some("pkg.toml") {
        path.to_path_buf()
    } else {
        path.join("pkg.toml")
    };

    let root = pkg_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let source = std::fs::read_to_string(&pkg_path).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to read project config '{}': {}",
                pkg_path.display(),
                err
            ),
            None,
        )
    })?;
    let config = parse_project_config(&source, &pkg_path)?;
    let entry = root.join(config.entry);

    Ok(ProjectConfig { root, entry })
}

pub fn run_project(path: impl AsRef<Path>) -> IcooResult<()> {
    run_project_with_output(path, |line| println!("{}", line))
}

pub fn run_project_with_output<F>(path: impl AsRef<Path>, output: F) -> IcooResult<()>
where
    F: FnMut(String) + 'static,
{
    let config = resolve_project(path)?;
    let mut interpreter = Interpreter::with_output(output);
    interpreter.interpret_entry_file(config.entry)?;
    interpreter.call_global_main()
}

fn parse_project_config(source: &str, path: &Path) -> IcooResult<ProjectConfigToml> {
    let value: toml::Value = toml::from_str(source).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to parse project config '{}': {}",
                path.display(),
                err
            ),
            None,
        )
    })?;
    let entry = value
        .get("run")
        .and_then(|run| run.get("entry"))
        .and_then(toml::Value::as_str)
        .unwrap_or("src/main.icoo");
    if entry.trim().is_empty() {
        return Err(IcooError::runtime(
            format!("project config '{}' has empty run.entry", path.display()),
            None,
        ));
    }
    Ok(ProjectConfigToml {
        entry: PathBuf::from(entry),
    })
}

#[derive(Debug)]
struct ProjectConfigToml {
    entry: PathBuf,
}
