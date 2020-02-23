use std::collections::HashMap;
use crate::{
    errors::error,
    lang::Value,
};
use std::sync::{Mutex, Arc};
use crate::errors::CrushResult;
use crate::lang::ValueType;

#[derive(Debug)]
pub struct ScopeData {
    /** This is the parent scope used to perform variable name resolution. If a variable lookup
     fails in the current scope, it proceeds to this scope.*/
    pub parent_scope: Option<Arc<Mutex<ScopeData>>>,
    /** This is the scope in which the current scope was called. Since a closure can be called
     from inside any scope, it need not be the same as the parent scope. This scope is the one used
     for break/continue loop control. */
    pub calling_scope: Option<Arc<Mutex<ScopeData>>>,

    /** This is a list of scopes that are imported into the current scope. Anything directly inside one
    of these scopes is also considered part of this scope. */
    pub uses: Vec<Arc<Mutex<ScopeData>>>,

    /** The actual data of this scope. */
    pub data: HashMap<String, Value>,

    /** True if this scope is a loop. */
    pub is_loop: bool,

    /** True if this scope should stop execution, i.e. if the continue or break commands have been called.  */
    pub is_stopped: bool,

    pub is_readonly: bool,
}

impl ScopeData {
    pub fn new(parent_scope: Option<Arc<Mutex<ScopeData>>>, caller: Option<Arc<Mutex<ScopeData>>>, is_loop: bool) -> ScopeData {
        return ScopeData {
            parent_scope,
            calling_scope: caller,
            is_loop,
            uses: Vec::new(),
            data: HashMap::new(),
            is_stopped: false,
            is_readonly: false,
        };
    }

    pub fn readonly(&mut self) {
        self.is_readonly = true;
    }

    pub fn do_break(&mut self) -> bool {
        if self.is_readonly {
            return false;
        } else if self.is_loop {
            self.is_stopped = true;
            true
        } else {
            let ok = self.calling_scope.as_deref()
                .map(|p| p.lock().unwrap().do_break())
                .unwrap_or(false);
            if !ok {
                false
            } else {
                self.is_stopped = true;
                true
            }
        }
    }

    pub fn do_continue(&mut self) -> bool {
        if self.is_readonly {
            return false;
        } else if self.is_loop {
            true
        } else {
            let ok = self.calling_scope.as_deref()
                .map(|p| p.lock().unwrap().do_continue())
                .unwrap_or(false);
            if !ok {
                false
            } else {
                self.is_stopped = true;
                true
            }
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.is_stopped
    }

    pub fn set(&mut self, name: &str, value: Value) -> CrushResult<()> {
        if !self.data.contains_key(name) {
            match &self.parent_scope {
                Some(p) => {
                    return p.lock().unwrap().set(name, value);
                }
                None => return error(format!("Unknown variable ${{{}}}", name).as_str()),
            }
        }
        if self.is_readonly {
            return error("Scope is read only");
        }

        if self.data[name].value_type() != value.value_type() {
            return error(format!("Type mismatch when reassigning variable ${{{}}}. Use `unset ${{{}}}` to remove old variable.", name, name).as_str());
        }
        self.data.insert(name.to_string(), value);
        return Ok(());
    }

    pub fn dump(&self, map: &mut HashMap<String, ValueType>) {
        match &self.parent_scope {
            Some(p) => p.lock().unwrap().dump(map),
            None => {}
        }
        for (k, v) in self.data.iter() {
            map.insert(k.clone(), v.value_type());
        }
    }


    pub fn remove(&mut self, name: &str) -> Option<Value> {
        if !self.data.contains_key(name) {
            match &self.parent_scope {
                Some(p) =>
                    p.lock().unwrap().remove(name),
                None => None,
            }
        } else {
            if self.is_readonly {
                return None;
            }
            self.data.remove(name)
        }
    }

    pub fn uses(&mut self, other: &Arc<Mutex<ScopeData>>) {
        self.uses.push(other.clone());
    }

}
