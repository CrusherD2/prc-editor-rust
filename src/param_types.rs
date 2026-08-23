use serde::{Deserialize, Serialize};
use crate::hash_labels::HashLabels;
use indexmap::IndexMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamValue {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    F32(f32),
    Hash(u64),
    String(String),
    List(ParamList),
    Struct(ParamStruct),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamStruct {
    pub type_hash: u64,
    pub fields: IndexMap<u64, ParamValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamList {
    pub values: Vec<ParamValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamNode {
    pub name: String,
    pub hash: u64,
    pub value: ParamValue,
    pub children: Vec<ParamNode>,
    #[serde(skip)]
    pub children_built: bool, // Track if children have been built yet (for lazy loading)
}

impl ParamNode {
    pub fn new(name: String, hash: u64, value: ParamValue) -> Self {
        Self {
            name,
            hash,
            value,
            children: Vec::new(),
            children_built: false,
        }
    }
    
    /// Update the key name and hash of this node
    #[allow(dead_code)]
    pub fn update_key(&mut self, new_name: String, new_hash: u64) {
        self.name = new_name;
        self.hash = new_hash;
    }
    
    /// Update the value of this node
    #[allow(dead_code)]
    pub fn update_value(&mut self, new_value: ParamValue) {
        self.value = new_value;
        // Rebuild children if it's a struct or list
        self.children.clear();
        match &self.value {
            ParamValue::Struct(_s) => {
                // Would need hash_labels reference to rebuild children properly
                // For now, just clear children - they'll be rebuilt when needed
            }
            ParamValue::List(_l) => {
                // Similar issue - would need to rebuild children
            }
            _ => {}
        }
    }
    
    /// Get mutable reference to child at path
    #[allow(dead_code)]
    pub fn get_child_mut(&mut self, path: &[usize]) -> Option<&mut ParamNode> {
        if path.is_empty() {
            return Some(self);
        }
        
        let first = path[0];
        if first >= self.children.len() {
            return None;
        }
        
        if path.len() == 1 {
            Some(&mut self.children[first])
        } else {
            self.children[first].get_child_mut(&path[1..])
        }
    }
    
    pub fn from_value(hash: u64, value: ParamValue, hash_labels: &HashLabels) -> Self {
        let name = hash_labels.hash_to_string(hash);
        let node = Self::new(name, hash, value.clone());
        
        // Don't build children immediately for performance - they'll be built lazily when expanded
        // This dramatically improves loading time for large files
        
        node
    }

    /// Build children for this node if they haven't been built yet (lazy loading)
    pub fn ensure_children_built(&mut self, hash_labels: &HashLabels) {
        if self.children_built {
            return;
        }
        
        match &self.value {
            ParamValue::Struct(s) => {
                for (field_hash, field_value) in &s.fields {
                    let child = Self::from_value(*field_hash, field_value.clone(), hash_labels);
                    self.children.push(child);
                }
            },
            ParamValue::List(l) => {
                for (index, item_value) in l.values.iter().enumerate() {
                    let mut child = Self::from_value(index as u64, item_value.clone(), hash_labels);
                    child.name = format!("[{}]", index); // Override name for list items
                    self.children.push(child);
                }
            },
            _ => {}
        }
        
        self.children_built = true;
    }

    pub fn is_expandable(&self) -> bool {
        matches!(self.value, ParamValue::Struct(_) | ParamValue::List(_))
    }

    pub fn get_type_name(&self) -> &'static str {
        match &self.value {
            ParamValue::Bool(_) => "Bool",
            ParamValue::I8(_) => "SByte",
            ParamValue::U8(_) => "Byte", 
            ParamValue::I16(_) => "Short",
            ParamValue::U16(_) => "UShort",
            ParamValue::I32(_) => "Int",
            ParamValue::U32(_) => "UInt",
            ParamValue::F32(_) => "Float",
            ParamValue::Hash(_) => "Hash40",
            ParamValue::String(_) => "String",
            ParamValue::List(_) => "List",
            ParamValue::Struct(_) => "Struct",
        }
    }

    pub fn get_value_string(&self) -> String {
        match &self.value {
            ParamValue::Bool(v) => v.to_string(),
            ParamValue::I8(v) => v.to_string(),
            ParamValue::U8(v) => v.to_string(),
            ParamValue::I16(v) => v.to_string(),
            ParamValue::U16(v) => v.to_string(),
            ParamValue::I32(v) => v.to_string(),
            ParamValue::U32(v) => v.to_string(),
            ParamValue::F32(v) => v.to_string(),
            ParamValue::Hash(v) => format!("0x{:X}", v),
            ParamValue::String(v) => v.clone(),
            ParamValue::List(l) => format!("List ({} items)", l.values.len()),
            ParamValue::Struct(s) => format!("Struct ({} fields)", s.fields.len()),
        }
    }

    pub fn get_value_string_with_labels(&self, hash_labels: &crate::hash_labels::HashLabels) -> String {
        match &self.value {
            ParamValue::Hash(v) => hash_labels.hash_to_string(*v),
            _ => self.get_value_string(),
        }
    }
} 