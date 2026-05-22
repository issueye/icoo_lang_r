use super::{check_value_type, Interpreter};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::native_modules;
use crate::parser::ast::{ClassDecl, FieldKind};
use crate::runtime::env::{BindingKind, Environment};
use crate::runtime::value::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

impl Interpreter {
    pub(super) fn execute_class(&mut self, decl: &ClassDecl) -> IcooResult<()> {
        let superclass = if let Some(super_name) = &decl.superclass {
            match self.env.borrow().get(&super_name.name, super_name.span)? {
                Value::Class(class) => Some(class),
                _ => {
                    return Err(IcooError::runtime(
                        format!("superclass '{}' is not a class", super_name.name),
                        Some(super_name.span),
                    ))
                }
            }
        } else {
            None
        };

        let mut seen = HashSet::new();
        if let Some(superclass) = &superclass {
            for field in superclass.all_fields() {
                seen.insert(field.name);
            }
        }
        let mut fields = Vec::new();
        for field in &decl.fields {
            if !seen.insert(field.name.name.clone()) {
                return Err(IcooError::runtime(
                    format!(
                        "field '{}' is already declared in this inheritance chain",
                        field.name.name
                    ),
                    Some(field.name.span),
                ));
            }
            fields.push(FieldDef {
                name: field.name.name.clone(),
                kind: field.kind,
                type_hint: field.type_hint.clone(),
                initializer: field.initializer.clone(),
            });
        }

        let method_closure = if let Some(superclass) = &superclass {
            let env = Environment::child(self.env.clone());
            env.borrow_mut().define(
                "super".to_string(),
                Value::Class(superclass.clone()),
                true,
                BindingKind::Const,
            );
            env
        } else {
            self.env.clone()
        };

        let mut methods = HashMap::new();
        for method in &decl.methods {
            methods.insert(
                method.name.name.clone(),
                Rc::new(IcooFunction {
                    decl: method.clone(),
                    closure: method_closure.clone(),
                    bound_self: None,
                    is_initializer: method.name.name == "init",
                }),
            );
        }

        let class = Rc::new(IcooClass {
            name: decl.name.name.clone(),
            superclass,
            fields,
            methods,
        });
        self.env.borrow_mut().define(
            decl.name.name.clone(),
            Value::Class(class),
            true,
            BindingKind::Const,
        );
        Ok(())
    }

    pub(super) fn get_property(
        &mut self,
        object: Value,
        name: &str,
        span: Span,
    ) -> IcooResult<Value> {
        if let Value::Module(module) = &object {
            return module.exports.get(name).cloned().ok_or_else(|| {
                IcooError::runtime(
                    format!(
                        "module '{}' has no export '{}'",
                        module.path.display(),
                        name
                    ),
                    Some(span),
                )
            });
        }
        if let Value::NativeModule(module) = &object {
            if native_modules::has_method(&module.name, name) {
                return Ok(Value::NativeModuleMethod(Rc::new(NativeModuleMethod {
                    module: module.name.clone(),
                    name: name.to_string(),
                })));
            }
        }
        if let Value::Instance(instance) = &object {
            let field = instance.borrow().fields.get(name).cloned();
            if let Some(field) = field {
                if !field.initialized {
                    return Err(IcooError::runtime(
                        format!("field '{}' is not initialized", name),
                        Some(span),
                    ));
                }
                return Ok(field.value);
            }
            if let Some(method) = instance.borrow().class.find_method(name) {
                let bound = method.bind(Value::Instance(instance.clone()));
                return Ok(Value::Function(Rc::new(bound)));
            }
        }
        if self.has_native_method(&object, name) {
            return Ok(Value::NativeMethod(Rc::new(NativeMethod {
                name: name.to_string(),
                receiver: object,
            })));
        }
        Err(IcooError::runtime(
            format!(
                "type '{}' has no property or method '{}'",
                object.type_name(),
                name
            ),
            Some(span),
        ))
    }

    pub(super) fn set_property(
        &mut self,
        object: Value,
        name: &str,
        value: Value,
        span: Span,
    ) -> IcooResult<()> {
        let Value::Instance(instance) = object else {
            return Err(IcooError::runtime("only instances have fields", Some(span)));
        };
        let mut instance_ref = instance.borrow_mut();
        let class_name = instance_ref.class.name.clone();
        let Some(field) = instance_ref.fields.get_mut(name) else {
            return Err(IcooError::runtime(
                format!(
                    "cannot assign undeclared field '{}' on class '{}'",
                    name, class_name
                ),
                Some(span),
            ));
        };
        match field.kind {
            FieldKind::Mutable => {
                check_value_type(&value, &field.type_hint, &format!("field '{}'", name), span)?;
                field.value = value;
                field.initialized = true;
                Ok(())
            }
            FieldKind::Const => Err(IcooError::runtime(
                format!("cannot assign const field '{}'", name),
                Some(span),
            )),
            FieldKind::Final if !field.initialized => {
                check_value_type(&value, &field.type_hint, &format!("field '{}'", name), span)?;
                field.value = value;
                field.initialized = true;
                Ok(())
            }
            FieldKind::Final => Err(IcooError::runtime(
                format!("final field '{}' can only be assigned once", name),
                Some(span),
            )),
        }
    }

    pub(super) fn eval_super_get(&mut self, name: &str, span: Span) -> IcooResult<Value> {
        let superclass = match self.env.borrow().get("super", span)? {
            Value::Class(class) => class,
            _ => return Err(IcooError::runtime("'super' is not a class", Some(span))),
        };
        let receiver = self.env.borrow().get("self", span)?;
        let Some(method) = superclass.find_method(name) else {
            return Err(IcooError::runtime(
                format!("undefined superclass method '{}'", name),
                Some(span),
            ));
        };
        Ok(Value::Function(Rc::new(method.bind(receiver))))
    }

    pub(super) fn call_class(
        &mut self,
        class: Rc<IcooClass>,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        let fields = class.all_fields();
        let mut instance_fields = HashMap::new();
        for field in &fields {
            instance_fields.insert(
                field.name.clone(),
                FieldValue {
                    value: Value::Nil,
                    initialized: false,
                    kind: field.kind,
                    type_hint: field.type_hint.clone(),
                },
            );
        }
        let instance = Rc::new(RefCell::new(Instance {
            class: class.clone(),
            fields: instance_fields,
        }));
        let receiver = Value::Instance(instance.clone());

        for field in &fields {
            if let Some(initializer) = &field.initializer {
                let value = self.eval(initializer)?;
                check_value_type(
                    &value,
                    &field.type_hint,
                    &format!("field '{}'", field.name),
                    span,
                )?;
                let mut instance_ref = instance.borrow_mut();
                let Some(slot) = instance_ref.fields.get_mut(&field.name) else {
                    return Err(IcooError::runtime(
                        format!("internal error: missing field '{}'", field.name),
                        Some(span),
                    ));
                };
                slot.value = value;
                slot.initialized = true;
            }
        }

        if let Some(init) = class.find_method("init") {
            let bound = init.bind(receiver.clone());
            self.call_function(Rc::new(bound), args, span)?;
        } else if !args.is_empty() {
            return Err(IcooError::runtime(
                format!(
                    "class '{}' expected 0 arguments but got {}",
                    class.name,
                    args.len()
                ),
                Some(span),
            ));
        }

        let missing: Vec<String> = instance
            .borrow()
            .fields
            .iter()
            .filter(|(_, field)| !field.initialized)
            .map(|(name, _)| name.clone())
            .collect();
        if !missing.is_empty() {
            return Err(IcooError::runtime(
                format!(
                    "class '{}' did not initialize required fields: {}",
                    class.name,
                    missing.join(", ")
                ),
                Some(span),
            ));
        }
        Ok(receiver)
    }
}
