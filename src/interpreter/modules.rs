use super::{install_natives_into, Interpreter};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::native_modules;
use crate::parser::ast::{Program, Stmt};
use crate::runtime::env::{EnvRef, Environment};
use crate::runtime::value::{IcooModule, NativeModule, NativeModuleMethod, Value};
use crate::{lexer, parser, resolver, typechecker};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

impl Interpreter {
    pub fn interpret_file(&mut self, path: impl AsRef<Path>) -> IcooResult<()> {
        let path = canonical_module_path(path.as_ref()).map_err(|message| {
            IcooError::runtime(format!("module load error: {}", message), None)
        })?;
        self.load_module(&path)?;
        Ok(())
    }

    pub(super) fn load_import_value(&mut self, source: &str, span: Span) -> IcooResult<Value> {
        if let Some(module_name) = native_modules::import_path(source) {
            return Ok(Value::NativeModule(Rc::new(NativeModule {
                name: module_name.to_string(),
            })));
        }
        self.load_relative_module(source, span).map(Value::Module)
    }

    fn load_relative_module(&mut self, source: &str, span: Span) -> IcooResult<Rc<IcooModule>> {
        if !source.ends_with(".icoo") {
            return Err(IcooError::runtime(
                "module path must end with '.icoo'",
                Some(span),
            ));
        }
        if !(source.starts_with("./") || source.starts_with("../")) {
            return Err(IcooError::runtime(
                "module path must start with './' or '../'",
                Some(span),
            ));
        }
        let base_dir = self
            .current_module_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let path = canonical_module_path(&base_dir.join(source)).map_err(|message| {
            IcooError::runtime(format!("module load error: {}", message), Some(span))
        })?;
        self.load_module(&path)
    }

    fn load_module(&mut self, path: &Path) -> IcooResult<Rc<IcooModule>> {
        if let Some(module) = self.module_cache.get(path) {
            return Ok(module.clone());
        }
        if let Some(index) = self
            .loading_modules
            .iter()
            .position(|loading| loading == path)
        {
            let mut cycle = self.loading_modules[index..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(path.display().to_string());
            return Err(IcooError::runtime(
                format!("module cycle detected: {}", cycle.join(" -> ")),
                None,
            ));
        }

        let source = std::fs::read_to_string(path).map_err(|err| {
            IcooError::runtime(
                format!("failed to read module '{}': {}", path.display(), err),
                None,
            )
        })?;
        let tokens = lexer::lex(&source)?;
        let program = parser::parse(tokens)?;
        resolver::resolve(&program)?;
        typechecker::check(&program)?;

        self.loading_modules.push(path.to_path_buf());
        let previous_env = self.env.clone();
        let previous_dir = self.current_module_dir.clone();
        let module_env = Environment::new();
        self.env = module_env.clone();
        install_natives_into(&self.env);
        self.current_module_dir = path.parent().map(Path::to_path_buf);

        let execution = (|| {
            for stmt in &program.statements {
                self.execute(stmt)?;
            }
            collect_exports(&program, &module_env)
        })();

        self.env = previous_env;
        self.current_module_dir = previous_dir;
        self.loading_modules.pop();

        let exports = execution?;
        let module = Rc::new(IcooModule {
            path: path.to_path_buf(),
            exports,
        });
        self.module_cache.insert(path.to_path_buf(), module.clone());
        Ok(module)
    }
}

fn collect_exports(program: &Program, module_env: &EnvRef) -> IcooResult<HashMap<String, Value>> {
    let mut exports = HashMap::new();
    for stmt in &program.statements {
        if let Stmt::ExportDecl(inner) = stmt {
            let (name, span) = export_name(inner).ok_or_else(|| {
                IcooError::runtime("exported statement has no binding name", None)
            })?;
            if exports.contains_key(&name) {
                return Err(IcooError::runtime(
                    format!("duplicate export '{}'", name),
                    Some(span),
                ));
            }
            let value = module_env.borrow().get(&name, span)?;
            exports.insert(name, value);
        }
    }
    Ok(exports)
}

fn canonical_module_path(path: &Path) -> Result<PathBuf, String> {
    let path = path
        .canonicalize()
        .map_err(|err| format!("failed to resolve '{}': {}", path.display(), err))?;
    if path.extension().and_then(|ext| ext.to_str()) != Some("icoo") {
        return Err(format!(
            "module path '{}' must end with .icoo",
            path.display()
        ));
    }
    Ok(path)
}

pub(super) fn imported_member(module: &Value, name: &str, span: Span) -> IcooResult<Value> {
    match module {
        Value::Module(module) => module.exports.get(name).cloned().ok_or_else(|| {
            IcooError::runtime(
                format!(
                    "module '{}' has no export '{}'",
                    module.path.display(),
                    name
                ),
                Some(span),
            )
        }),
        Value::NativeModule(module) if native_modules::has_method(&module.name, name) => {
            Ok(Value::NativeModuleMethod(Rc::new(NativeModuleMethod {
                module: module.name.clone(),
                name: name.to_string(),
            })))
        }
        Value::NativeModule(module) => Err(IcooError::runtime(
            format!("module '{}' has no export '{}'", module.name, name),
            Some(span),
        )),
        _ => Err(IcooError::runtime("value is not a module", Some(span))),
    }
}

fn export_name(stmt: &Stmt) -> Option<(String, Span)> {
    match stmt {
        Stmt::Let(decl) | Stmt::Const(decl) | Stmt::Final(decl) => {
            Some((decl.name.name.clone(), decl.name.span))
        }
        Stmt::Function(decl) => Some((decl.name.name.clone(), decl.name.span)),
        Stmt::Class(decl) => Some((decl.name.name.clone(), decl.name.span)),
        _ => None,
    }
}
