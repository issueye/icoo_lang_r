use crate::error::{IcooError, IcooResult};
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;

const EMBED_MAGIC: &[u8; 16] = b"ICOO_STANDALONE\0";
const EMBED_FOOTER_LEN: u64 = 24;

#[derive(Debug, Clone)]
pub struct PackageOptions {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub standalone: bool,
}

#[derive(Debug, Clone)]
pub struct PackageResult {
    pub output: PathBuf,
    pub files: usize,
}

#[derive(Debug, Clone)]
struct PackageManifest {
    name: String,
    version: String,
    entry: String,
}

pub fn package_path(options: PackageOptions) -> IcooResult<PackageResult> {
    let input = normalize_input(&options.input)?;
    let output_override = options.output.clone();
    let standalone = options.standalone;
    let (payload, manifest, files) = package_payload(&input, output_override.as_deref())?;
    let output = match output_override {
        Some(output) => output,
        None if standalone => default_standalone_output_path(&manifest)?,
        None => default_archive_output_path(&manifest),
    };
    ensure_output_parent(&output)?;

    if standalone {
        write_standalone_binary(&output, &payload)?;
    } else {
        fs::write(&output, &payload).map_err(|err| {
            IcooError::runtime(
                format!("failed to write package '{}': {}", output.display(), err),
                None,
            )
        })?;
    }

    Ok(PackageResult {
        output,
        files: files + 2,
    })
}

pub fn run_embedded_package() -> IcooResult<Option<()>> {
    let exe = std::env::current_exe().map_err(|err| {
        IcooError::runtime(
            format!("failed to locate current executable: {}", err),
            None,
        )
    })?;
    let Some(payload) = read_embedded_payload(&exe)? else {
        return Ok(None);
    };
    let root = extract_payload_to_temp(&payload)?;
    if root.join("pkg.toml").exists() {
        crate::run_project(root)?;
    } else {
        let manifest = read_embedded_manifest(&root)?;
        crate::run_file(root.join(manifest.entry))?;
    }
    Ok(Some(()))
}

fn package_payload(
    input: &Path,
    output: Option<&Path>,
) -> IcooResult<(Vec<u8>, PackageManifest, usize)> {
    let (root, manifest, explicit_files) =
        if input.file_name().and_then(|name| name.to_str()) == Some("pkg.toml") {
            let (root, manifest) = package_project_manifest(&input)?;
            (root, manifest, None)
        } else if input.is_file() {
            let (root, manifest) = package_single_file_manifest(&input)?;
            (root, manifest, Some(vec![input.to_path_buf()]))
        } else {
            let (root, manifest) = package_project_manifest(&input)?;
            (root, manifest, None)
        };

    let mut files = Vec::new();
    if let Some(explicit_files) = explicit_files {
        files.extend(explicit_files);
    } else {
        collect_files(&root, output, &mut files)?;
    }
    files.sort();

    let mut buffer = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut buffer);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    write_manifest(&mut zip, &manifest, options)?;
    write_readme(&mut zip, &manifest, options)?;

    for path in &files {
        let relative = path.strip_prefix(&root).map_err(|err| {
            IcooError::runtime(
                format!(
                    "failed to compute package path for '{}': {}",
                    path.display(),
                    err
                ),
                None,
            )
        })?;
        let name = zip_path(relative);
        zip.start_file(name, options).map_err(zip_err)?;
        let mut source = File::open(path).map_err(|err| {
            IcooError::runtime(
                format!("failed to read '{}': {}", path.display(), err),
                None,
            )
        })?;
        let mut buffer = Vec::new();
        source.read_to_end(&mut buffer).map_err(|err| {
            IcooError::runtime(
                format!("failed to read '{}': {}", path.display(), err),
                None,
            )
        })?;
        zip.write_all(&buffer).map_err(zip_err)?;
    }

    zip.finish().map_err(zip_err)?;
    drop(zip);
    Ok((buffer.into_inner(), manifest, files.len()))
}

fn normalize_input(input: &Path) -> IcooResult<PathBuf> {
    let path = if input.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        input.to_path_buf()
    };
    if path.exists() {
        Ok(path)
    } else {
        Err(IcooError::runtime(
            format!("package input '{}' does not exist", path.display()),
            None,
        ))
    }
}

fn package_single_file_manifest(input: &Path) -> IcooResult<(PathBuf, PackageManifest)> {
    if input.extension().and_then(|ext| ext.to_str()) != Some("icoo") {
        return Err(IcooError::runtime(
            format!("package input '{}' is not an .icoo file", input.display()),
            None,
        ));
    }
    let root = input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| IcooError::runtime("package input has invalid file name", None))?;
    let name = input
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("icoo-script")
        .to_string();
    Ok((
        root,
        PackageManifest {
            name: sanitize_package_name(&name),
            version: "0.1.0".to_string(),
            entry: file_name.to_string(),
        },
    ))
}

fn package_project_manifest(input: &Path) -> IcooResult<(PathBuf, PackageManifest)> {
    let pkg_path = if input.file_name().and_then(|name| name.to_str()) == Some("pkg.toml") {
        input.to_path_buf()
    } else {
        input.join("pkg.toml")
    };
    let root = pkg_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let source = fs::read_to_string(&pkg_path).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to read project config '{}': {}",
                pkg_path.display(),
                err
            ),
            None,
        )
    })?;
    let value: toml::Value = toml::from_str(&source).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to parse project config '{}': {}",
                pkg_path.display(),
                err
            ),
            None,
        )
    })?;
    let name = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or("icoo-app");
    let version = value
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .unwrap_or("0.1.0");
    let entry = value
        .get("run")
        .and_then(|run| run.get("entry"))
        .and_then(toml::Value::as_str)
        .unwrap_or("src/main.icoo");
    Ok((
        root,
        PackageManifest {
            name: sanitize_package_name(name),
            version: sanitize_package_name(version),
            entry: entry.to_string(),
        },
    ))
}

fn default_archive_output_path(manifest: &PackageManifest) -> PathBuf {
    PathBuf::from("target").join(format!("{}-{}.icoo.zip", manifest.name, manifest.version))
}

fn default_standalone_output_path(manifest: &PackageManifest) -> IcooResult<PathBuf> {
    let exe = std::env::current_exe().map_err(|err| {
        IcooError::runtime(
            format!("failed to locate current executable: {}", err),
            None,
        )
    })?;
    let extension = exe.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let name = format!("{}-{}", manifest.name, manifest.version);
    let mut output = PathBuf::from("target").join(name);
    if !extension.is_empty() {
        output.set_extension(extension);
    }
    Ok(output)
}

fn ensure_output_parent(output: &Path) -> IcooResult<()> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                IcooError::runtime(
                    format!(
                        "failed to create package output directory '{}': {}",
                        parent.display(),
                        err
                    ),
                    None,
                )
            })?;
        }
    }
    Ok(())
}

fn collect_files(root: &Path, output: Option<&Path>, files: &mut Vec<PathBuf>) -> IcooResult<()> {
    for entry in fs::read_dir(root).map_err(|err| {
        IcooError::runtime(
            format!("failed to list package root '{}': {}", root.display(), err),
            None,
        )
    })? {
        let entry = entry.map_err(|err| {
            IcooError::runtime(
                format!("failed to read package directory entry: {}", err),
                None,
            )
        })?;
        let path = entry.path();
        if should_skip(&path, output) {
            continue;
        }
        let metadata = entry.metadata().map_err(|err| {
            IcooError::runtime(
                format!("failed to inspect '{}': {}", path.display(), err),
                None,
            )
        })?;
        if metadata.is_dir() {
            collect_files(&path, output, files)?;
        } else if metadata.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn should_skip(path: &Path, output: Option<&Path>) -> bool {
    if output.is_some_and(|output| same_path(path, output)) {
        return true;
    }
    match path.file_name().and_then(|name| name.to_str()) {
        Some(".git" | ".vscode" | "node_modules" | "target") => true,
        Some(name)
            if name.ends_with(".icoo.zip")
                || name.ends_with(".vsix")
                || name.ends_with(".standalone") =>
        {
            true
        }
        _ => false,
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).ok();
    let right = fs::canonicalize(right).ok();
    left.is_some() && left == right
}

fn write_manifest<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    manifest: &PackageManifest,
    options: FileOptions,
) -> IcooResult<()> {
    zip.start_file("ICOOPACKAGE.toml", options)
        .map_err(zip_err)?;
    writeln!(
        zip,
        "[package]\nname = \"{}\"\nversion = \"{}\"\nentry = \"{}\"\n",
        escape_toml(&manifest.name),
        escape_toml(&manifest.version),
        escape_toml(&manifest.entry)
    )
    .map_err(zip_err)
}

fn write_readme<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    manifest: &PackageManifest,
    options: FileOptions,
) -> IcooResult<()> {
    zip.start_file("README.package.txt", options)
        .map_err(zip_err)?;
    writeln!(
        zip,
        "Icoo package: {}\nVersion: {}\nEntry: {}\n\nRun after extracting:\n  icoo run .\n",
        manifest.name, manifest.version, manifest.entry
    )
    .map_err(zip_err)
}

fn sanitize_package_name(value: &str) -> String {
    let result: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let result = result.trim_matches('-');
    if result.is_empty() {
        "icoo-package".to_string()
    } else {
        result.to_string()
    }
}

fn escape_toml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn zip_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn zip_err(err: impl std::fmt::Display) -> IcooError {
    IcooError::runtime(format!("failed to write package: {}", err), None)
}

fn write_standalone_binary(output: &Path, payload: &[u8]) -> IcooResult<()> {
    let launcher = std::env::current_exe().map_err(|err| {
        IcooError::runtime(
            format!("failed to locate current executable: {}", err),
            None,
        )
    })?;
    let mut launcher_file = File::open(&launcher).map_err(|err| {
        IcooError::runtime(
            format!("failed to read launcher '{}': {}", launcher.display(), err),
            None,
        )
    })?;
    let mut output_file = File::create(output).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to create standalone binary '{}': {}",
                output.display(),
                err
            ),
            None,
        )
    })?;
    std::io::copy(&mut launcher_file, &mut output_file).map_err(|err| {
        IcooError::runtime(
            format!("failed to copy launcher '{}': {}", launcher.display(), err),
            None,
        )
    })?;
    output_file.write_all(payload).map_err(|err| {
        IcooError::runtime(format!("failed to append package payload: {}", err), None)
    })?;
    output_file.write_all(EMBED_MAGIC).map_err(|err| {
        IcooError::runtime(format!("failed to append package marker: {}", err), None)
    })?;
    output_file
        .write_all(&(payload.len() as u64).to_le_bytes())
        .map_err(|err| {
            IcooError::runtime(
                format!("failed to append package payload length: {}", err),
                None,
            )
        })?;
    Ok(())
}

fn read_embedded_payload(exe: &Path) -> IcooResult<Option<Vec<u8>>> {
    let mut file = File::open(exe).map_err(|err| {
        IcooError::runtime(
            format!("failed to read executable '{}': {}", exe.display(), err),
            None,
        )
    })?;
    let len = file
        .metadata()
        .map_err(|err| {
            IcooError::runtime(
                format!("failed to inspect executable '{}': {}", exe.display(), err),
                None,
            )
        })?
        .len();
    if len < EMBED_FOOTER_LEN {
        return Ok(None);
    }
    file.seek(SeekFrom::End(-(EMBED_FOOTER_LEN as i64)))
        .map_err(|err| IcooError::runtime(format!("failed to seek executable: {}", err), None))?;
    let mut magic = [0_u8; 16];
    file.read_exact(&mut magic).map_err(|err| {
        IcooError::runtime(format!("failed to read package marker: {}", err), None)
    })?;
    if &magic != EMBED_MAGIC {
        return Ok(None);
    }
    let mut size = [0_u8; 8];
    file.read_exact(&mut size).map_err(|err| {
        IcooError::runtime(
            format!("failed to read package payload length: {}", err),
            None,
        )
    })?;
    let payload_len = u64::from_le_bytes(size);
    if payload_len > len - EMBED_FOOTER_LEN {
        return Err(IcooError::runtime(
            "embedded package payload is invalid",
            None,
        ));
    }
    file.seek(SeekFrom::Start(len - EMBED_FOOTER_LEN - payload_len))
        .map_err(|err| {
            IcooError::runtime(format!("failed to seek package payload: {}", err), None)
        })?;
    let mut payload = vec![0_u8; payload_len as usize];
    file.read_exact(&mut payload).map_err(|err| {
        IcooError::runtime(format!("failed to read package payload: {}", err), None)
    })?;
    Ok(Some(payload))
}

fn extract_payload_to_temp(payload: &[u8]) -> IcooResult<PathBuf> {
    let root = std::env::temp_dir().join(format!(
        "icoo_embedded_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&root).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to create embedded package directory '{}': {}",
                root.display(),
                err
            ),
            None,
        )
    })?;
    let reader = Cursor::new(payload);
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_err)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_err)?;
        let Some(path) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let output = root.join(path);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|err| {
                IcooError::runtime(
                    format!("failed to create directory '{}': {}", output.display(), err),
                    None,
                )
            })?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                IcooError::runtime(
                    format!("failed to create directory '{}': {}", parent.display(), err),
                    None,
                )
            })?;
        }
        let mut file = File::create(&output).map_err(|err| {
            IcooError::runtime(
                format!("failed to extract '{}': {}", output.display(), err),
                None,
            )
        })?;
        std::io::copy(&mut entry, &mut file).map_err(|err| {
            IcooError::runtime(
                format!("failed to extract '{}': {}", output.display(), err),
                None,
            )
        })?;
    }
    Ok(root)
}

fn read_embedded_manifest(root: &Path) -> IcooResult<PackageManifest> {
    let path = root.join("ICOOPACKAGE.toml");
    let source = fs::read_to_string(&path).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to read embedded package manifest '{}': {}",
                path.display(),
                err
            ),
            None,
        )
    })?;
    let value: toml::Value = toml::from_str(&source).map_err(|err| {
        IcooError::runtime(
            format!(
                "failed to parse embedded package manifest '{}': {}",
                path.display(),
                err
            ),
            None,
        )
    })?;
    let package = value.get("package");
    let name = package
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or("icoo-package");
    let version = package
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .unwrap_or("0.1.0");
    let entry = package
        .and_then(|package| package.get("entry"))
        .and_then(toml::Value::as_str)
        .unwrap_or("main.icoo");
    Ok(PackageManifest {
        name: name.to_string(),
        version: version.to_string(),
        entry: entry.to_string(),
    })
}
