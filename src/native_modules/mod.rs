mod env;
mod io;
mod io_fs;
mod json;
mod math;
mod net_http_client;
mod net_http_server;
mod os;
mod time;
mod toml;
mod web_ino;
mod yaml;

pub struct NativeModuleSpec {
    pub import_path: &'static str,
    pub kind: &'static str,
    pub type_name: &'static str,
    pub methods: &'static [&'static str],
}

pub const SPECS: &[NativeModuleSpec] = &[
    math::SPEC,
    time::SPEC,
    json::SPEC,
    yaml::SPEC,
    toml::SPEC,
    env::SPEC,
    io::SPEC,
    io_fs::SPEC,
    os::SPEC,
    net_http_client::SPEC,
    net_http_server::SPEC,
    web_ino::SPEC,
];

pub fn import_path(source: &str) -> Option<&'static str> {
    SPECS
        .iter()
        .find(|spec| spec.import_path == source)
        .map(|spec| spec.import_path)
}

pub fn type_name(source: &str) -> Option<&'static str> {
    SPECS
        .iter()
        .find(|spec| spec.import_path == source)
        .map(|spec| spec.type_name)
}

pub fn kind(module: &str) -> &str {
    SPECS
        .iter()
        .find(|spec| spec.import_path == module || spec.kind == module)
        .map(|spec| spec.kind)
        .unwrap_or_else(|| module.strip_prefix("std.").unwrap_or(module))
}

pub fn has_method(module: &str, name: &str) -> bool {
    let kind = kind(module);
    SPECS
        .iter()
        .find(|spec| spec.kind == kind)
        .map(|spec| spec.methods.contains(&name))
        .unwrap_or(false)
}
