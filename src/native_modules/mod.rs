pub(crate) mod env;
pub(crate) mod io;
pub(crate) mod io_fs;
pub(crate) mod json;
pub(crate) mod math;
pub(crate) mod net_http_client;
pub(crate) mod net_http_server;
pub(crate) mod os;
pub(crate) mod time;
pub(crate) mod toml;
pub(crate) mod web_ino;
pub(crate) mod yaml;

use crate::error::IcooResult;
use crate::interpreter::Interpreter;
use crate::lexer::token::Span;
use crate::runtime::value::Value;

pub struct NativeModuleSpec {
    pub import_path: &'static str,
    pub kind: &'static str,
    pub type_name: &'static str,
    pub methods: &'static [NativeMethodSpec],
}

pub struct NativeMethodSpec {
    pub name: &'static str,
    pub arity: NativeAritySpec,
    pub params: &'static [&'static str],
    pub variadic: Option<&'static str>,
    pub return_type: &'static str,
}

#[derive(Clone, Copy)]
pub enum NativeAritySpec {
    Exact(usize),
    Range { min: usize, max: usize },
    AtLeast(usize),
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
    method_spec(module, name).is_some()
}

pub fn method_spec(module: &str, name: &str) -> Option<&'static NativeMethodSpec> {
    let kind = kind(module);
    SPECS
        .iter()
        .find(|spec| spec.kind == kind)
        .and_then(|spec| spec.methods.iter().find(|method| method.name == name))
}

pub fn method_spec_for_type(type_name: &str, name: &str) -> Option<&'static NativeMethodSpec> {
    SPECS
        .iter()
        .find(|spec| spec.type_name == type_name)
        .and_then(|spec| spec.methods.iter().find(|method| method.name == name))
}

pub(crate) fn call(
    runtime: &mut Interpreter,
    kind: &str,
    name: &str,
    args: Vec<Value>,
    span: Span,
) -> Option<IcooResult<Value>> {
    match kind {
        "math" => math::call(name, args, span),
        "time" => time::call(name, args, span),
        "json" => json::call(name, args, span),
        "yaml" => yaml::call(name, args, span),
        "toml" => toml::call(name, args, span),
        "env" => env::call(runtime, name, args, span),
        "io" => io::call(runtime, name, args, span),
        "io.fs" => io_fs::call(runtime, name, args, span),
        "os" => os::call(runtime, name, args, span),
        "net.http.client" => net_http_client::call(runtime, name, args, span),
        "net.http.server" => net_http_server::call(runtime, name, args, span),
        "web.ino" => web_ino::call(name, args, span),
        _ => None,
    }
}
