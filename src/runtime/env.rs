use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type EnvRef = Rc<RefCell<Environment>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Mutable,
    Const,
    Final,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub value: Value,
    pub initialized: bool,
    pub kind: BindingKind,
}

#[derive(Debug)]
pub struct Environment {
    values: HashMap<String, Binding>,
    parent: Option<EnvRef>,
}

impl Environment {
    pub fn new() -> EnvRef {
        Rc::new(RefCell::new(Self {
            values: HashMap::new(),
            parent: None,
        }))
    }

    pub fn child(parent: EnvRef) -> EnvRef {
        Rc::new(RefCell::new(Self {
            values: HashMap::new(),
            parent: Some(parent),
        }))
    }

    pub fn define(&mut self, name: String, value: Value, initialized: bool, kind: BindingKind) {
        self.values.insert(
            name,
            Binding {
                value,
                initialized,
                kind,
            },
        );
    }

    pub fn get(&self, name: &str, span: Span) -> IcooResult<Value> {
        if let Some(binding) = self.values.get(name) {
            if !binding.initialized {
                return Err(IcooError::runtime(
                    format!("binding '{}' is not initialized", name),
                    Some(span),
                ));
            }
            return Ok(binding.value.clone());
        }
        if let Some(parent) = &self.parent {
            return parent.borrow().get(name, span);
        }
        Err(IcooError::runtime(
            format!("undefined variable '{}'", name),
            Some(span),
        ))
    }

    pub fn assign(&mut self, name: &str, value: Value, span: Span) -> IcooResult<()> {
        if let Some(binding) = self.values.get_mut(name) {
            match binding.kind {
                BindingKind::Mutable => {
                    binding.value = value;
                    binding.initialized = true;
                    Ok(())
                }
                BindingKind::Const => Err(IcooError::runtime(
                    format!("cannot assign to const binding '{}'", name),
                    Some(span),
                )),
                BindingKind::Final if !binding.initialized => {
                    binding.value = value;
                    binding.initialized = true;
                    Ok(())
                }
                BindingKind::Final => Err(IcooError::runtime(
                    format!("final binding '{}' can only be assigned once", name),
                    Some(span),
                )),
            }
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().assign(name, value, span)
        } else {
            Err(IcooError::runtime(
                format!("undefined variable '{}'", name),
                Some(span),
            ))
        }
    }
}
