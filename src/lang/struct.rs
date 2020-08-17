use crate::lang::table::ColumnType;
use crate::lang::table::Row;
use crate::lang::value::Value;
use crate::lang::value::ValueType;
use crate::util::identity_arc::Identity;
use crate::util::replace::Replace;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use ordered_map::OrderedMap;
use lazy_static::lazy_static;
use chrono::Duration;
use crate::lang::stream::CrushStream;
use crate::lang::errors::{CrushError, error};

lazy_static! {
    pub static ref STRUCT_STREAM_TYPE: Vec<ColumnType> = vec![
        ColumnType::new("name", ValueType::String),
        ColumnType::new("value", ValueType::Any),
    ];
}

#[derive(Clone)]
struct StructData {
    parent: Option<Struct>,
    lookup: OrderedMap<String, usize>,
    cells: Vec<Value>,
}

#[derive(Clone)]
pub struct Struct {
    data: Arc<Mutex<StructData>>,
}

impl Identity for Struct {
    fn id(&self) -> u64 {
        self.data.id()
    }
}

impl Hash for Struct {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let data = self.data.lock().unwrap();
        data.cells.iter().for_each(|value| {
            value.hash(state);
        });
        let p = data.parent.clone();
        drop(data);
        p.hash(state);
    }
}

impl PartialEq for Struct {
    fn eq(&self, other: &Self) -> bool {
        let us = self.data.lock().unwrap().clone();
        let them = other.data.lock().unwrap().clone();
        if us.cells.len() != them.cells.len() {
            return false;
        }
        for (v1, v2) in us.cells.iter().zip(them.cells.iter()) {
            if !v1.eq(v2) {
                return false;
            }
        }
        for (name, idx) in us.lookup.iter() {
            match them.lookup.get(name) {
                None => return false,
                Some(idx2) => {
                    if !idx.eq(idx2) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl PartialOrd for Struct {
    fn partial_cmp(&self, _other: &Self) -> Option<Ordering> {
        None
    }
}

impl Struct {
    pub fn new(mut vec: Vec<(String, Value)>, parent: Option<Struct>) -> Struct {
        let mut lookup = OrderedMap::new();
        let mut cells = Vec::new();
        vec.drain(..).for_each(|(key, value)| {
            lookup.insert(key, cells.len());
            cells.push(value);
        });
        Struct {
            data: Arc::new(Mutex::new(StructData {
                parent,
                cells,
                lookup,
            })),
        }
    }

    pub fn from_vec(mut values: Vec<Value>, types: Vec<ColumnType>) -> Struct {
        let mut lookup = OrderedMap::new();
        let mut cells = Vec::new();

        values.drain(..).zip(types).for_each(|(value, column)| {
            lookup.insert(column.name, cells.len());
            cells.push(value);
        });
        Struct {
            data: Arc::new(Mutex::new(StructData {
                parent: None,
                lookup,
                cells,
            })),
        }
    }

    pub fn local_signature(&self) -> Vec<ColumnType> {
        let mut res = Vec::new();
        let data = self.data.lock().unwrap();
        let mut reverse_lookup = OrderedMap::new();
        for (key, value) in &data.lookup {
            reverse_lookup.insert(value.clone(), key);
        }
        for (idx, value) in data.cells.iter().enumerate() {
            res.push(ColumnType::new(
                reverse_lookup.get(&idx).unwrap(),
                value.value_type(),
            ));
        }
        res
    }

    pub fn local_elements(&self) -> Vec<(String, Value)> {
        let mut reverse_lookup = OrderedMap::new();
        let data = self.data.lock().unwrap();
        for (key, value) in &data.lookup {
            reverse_lookup.insert(value.clone(), key);
        }
        data.cells
            .iter()
            .enumerate()
            .map(|(idx, v)| (reverse_lookup[&idx].to_string(), v.clone()))
            .collect()
    }

    pub fn to_row(&self) -> Row {
        Row::new(self.data.lock().unwrap().cells.clone())
    }

    pub fn to_vec(&self) -> Vec<Value> {
        self.data.lock().unwrap().cells.clone()
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        let data = self.data.lock().unwrap();
        match data.lookup.get(name) {
            None => {
                let p = data.parent.clone();
                drop(data);
                match p {
                    None => None,
                    Some(parent) => parent.get(name),
                }
            }
            Some(idx) => Some(data.cells[*idx].clone()),
        }
    }

    pub fn keys(&self) -> Vec<String> {
        let mut fields = HashSet::new();
        self.fill_keys(&mut fields);
        fields.drain().collect()
    }

    fn fill_keys(&self, dest: &mut HashSet<String>) {
        let data = self.data.lock().unwrap();
        data.lookup.keys().for_each(|name| {
            dest.insert(name.clone());
        });
        let parent = data.parent.clone();
        drop(data);
        if let Some(p) = parent {
            p.fill_keys(dest);
        }
    }

    pub fn map(&self) -> OrderedMap<String, Value> {
        let mut map = OrderedMap::new();
        self.fill_map(&mut map);
        map
    }

    fn fill_map(&self, dest: &mut OrderedMap<String, Value>) {
        let data = self.data.lock().unwrap();
        data.lookup.iter().for_each(|(name, idx)| {
            if !dest.contains_key(name) {
                dest.insert(name.clone(), data.cells[*idx].clone());
            }
        });
        let parent = data.parent.clone();
        drop(data);
        if let Some(p) = parent {
            p.fill_map(dest);
        }
    }

    pub fn set(&self, name: &str, value: Value) -> Option<Value> {
        let mut data = self.data.lock().unwrap();
        match data.lookup.get(name).cloned() {
            None => {
                let idx = data.lookup.len();
                data.lookup.insert(name.to_string(), idx);
                data.cells.push(value);
                None
            }
            Some(idx) => Some(data.cells.replace(idx, value)),
        }
    }

    pub fn materialize(&self) -> Struct {
        let data = self.data.lock().unwrap();
        Struct {
            data: Arc::new(Mutex::new(StructData {
                parent: data.parent.clone(),
                lookup: data.lookup.clone(),
                cells: data
                    .cells
                    .iter()
                    .map(|value| value.clone().materialize())
                    .collect(),
            })),
        }
    }

    pub fn set_parent(&self, parent: Option<Struct>) {
        self.data.lock().unwrap().parent = parent;
    }
}

impl ToString for Struct {
    fn to_string(&self) -> String {
        let elements = self.local_elements();
        let data = self.data.lock().unwrap();
        let parent = data.parent.clone();
        drop(data);
        format!(
            "data{} {}",
            parent
                .map(|p| format!(" parent=({})", p.to_string()))
                .unwrap_or_else(|| "".to_string()),
            elements
                .iter()
                .map(|(c, t)| format!("{}=({})", c, t.to_string()))
                .collect::<Vec<String>>()
                .join(", ")
        )
    }
}

pub struct StructReader {
    idx: usize,
    rows: Vec<(String, Value)>,
}

impl StructReader {
    pub fn new(s: Struct) -> StructReader {
        StructReader {
            idx: 0,
            rows: s.map().drain().collect(),
        }
    }
}

impl CrushStream for StructReader {
    fn read(&mut self) -> Result<Row, CrushError> {
        if self.idx >= self.rows.len() {
            return error("EOF");
        }
        self.idx += 1;
        let (k, v) = self
            .rows
            .replace(self.idx - 1, ("".to_string(), Value::Empty()));
        Ok(Row::new(vec![Value::String(k), v]))
    }

    fn read_timeout(
        &mut self,
        _timeout: Duration,
    ) -> Result<Row, crate::lang::stream::RecvTimeoutError> {
        match self.read() {
            Ok(r) => Ok(r),
            Err(_) => Err(crate::lang::stream::RecvTimeoutError::Disconnected),
        }
    }

    fn types(&self) -> &[ColumnType] {
        &STRUCT_STREAM_TYPE
    }
}
