use crate::param_types::*;
use crate::hash_labels::HashLabels;
use anyhow::{Result, anyhow};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::collections::HashMap;

use indexmap::IndexMap;

// Helper enum for reference entries (mimicking paracobNET's mixed list)
enum RefEntry {
    String(String),
    StructRef(Vec<(i32, i32)>), // (hash_index, param_offset) pairs
}

pub struct ParamFile {
    pub root: Option<ParamNode>,
    pub hash_labels: HashLabels,
    filename: String,
    original_hash_table: Vec<u64>,
}

impl ParamFile {
    pub fn new() -> Self {
        Self {
            root: None,
            hash_labels: HashLabels::new(),
            filename: String::new(),
            original_hash_table: Vec::new(),
        }
    }

    pub fn open(&mut self, data: &[u8], filename: &str) -> Result<()> {
        self.filename = filename.to_string();
        let mut cursor = Cursor::new(data);

        // Validate magic - first 8 bytes should be "paracobn"
        let mut magic = [0u8; 8];
        cursor.read_exact(&mut magic)?;
        let magic_str = std::str::from_utf8(&magic).map_err(|_| anyhow!("Invalid magic header"))?;
        
        if magic_str != "paracobn" {
            return Err(anyhow!("Invalid file format - magic mismatch. Expected 'paracobn', got '{}'", magic_str));
        }

        // Read sizes
        let hash_table_size = cursor.read_i32::<LittleEndian>()?;
        let ref_table_size = cursor.read_i32::<LittleEndian>()?;

        // Calculate offsets
        let hash_start = 0x10;
        let ref_start = 0x10 + hash_table_size;
        let param_start = 0x10 + hash_table_size + ref_table_size;

        // Read hash table
        cursor.seek(SeekFrom::Start(hash_start as u64))?;
        let hash_count = hash_table_size / 8;
        let mut hash_table = Vec::with_capacity(hash_count as usize);
        
        for _ in 0..hash_count {
            hash_table.push(cursor.read_u64::<LittleEndian>()?);
        }


        // Start reading from param section
        cursor.seek(SeekFrom::Start(param_start as u64))?;
        
        // Check if root is a struct
        let type_byte = cursor.read_u8()?;
        if type_byte != 12 { // ParamType::@struct = 12
            return Err(anyhow!("File does not have a struct root. Got type: {}", type_byte));
        }
        
        // Reset position to read the struct properly
        cursor.seek(SeekFrom::Start(param_start as u64))?;
        
        // Store the original hash table to preserve order during save
        self.original_hash_table = hash_table.clone();

        let root_value = self.read_param(&mut cursor, &hash_table, hash_start, ref_start)?;
        self.root = Some(ParamNode::from_value(0x0, root_value, &self.hash_labels));

        Ok(())
    }

    fn read_param(&mut self, cursor: &mut Cursor<&[u8]>, hash_table: &[u64], hash_start: i32, ref_start: i32) -> Result<ParamValue> {
        let type_byte = cursor.read_u8()?;
        
        match type_byte {
            1 => Ok(ParamValue::Bool(cursor.read_u8()? != 0)),
            2 => Ok(ParamValue::I8(cursor.read_i8()?)),
            3 => Ok(ParamValue::U8(cursor.read_u8()?)),
            4 => Ok(ParamValue::I16(cursor.read_i16::<LittleEndian>()?)),
            5 => Ok(ParamValue::U16(cursor.read_u16::<LittleEndian>()?)),
            6 => Ok(ParamValue::I32(cursor.read_i32::<LittleEndian>()?)),
            7 => Ok(ParamValue::U32(cursor.read_u32::<LittleEndian>()?)),
            8 => Ok(ParamValue::F32(cursor.read_f32::<LittleEndian>()?)),
            9 => {
                // hash40 - read index and lookup in hash table
                let hash_index = cursor.read_u32::<LittleEndian>()? as usize;
                if hash_index >= hash_table.len() {
                    return Err(anyhow!("Hash index {} out of bounds (table size: {})", hash_index, hash_table.len()));
                }
                Ok(ParamValue::Hash(hash_table[hash_index]))
            }
            10 => {
                // string - read offset and follow reference
                let string_offset = cursor.read_i32::<LittleEndian>()?;
                let current_pos = cursor.position();
                
                cursor.seek(SeekFrom::Start((ref_start + string_offset) as u64))?;
                let mut string_bytes = Vec::new();
                loop {
                    let byte = cursor.read_u8()?;
                    if byte == 0 {
                        break;
                    }
                    string_bytes.push(byte);
                }
                
                cursor.seek(SeekFrom::Start(current_pos))?;
                let string_value = String::from_utf8_lossy(&string_bytes).to_string();
                Ok(ParamValue::String(string_value))
            }
            11 => {
                // list
                let start_pos = cursor.position() - 1;
                let count = cursor.read_i32::<LittleEndian>()?;
                let mut offsets = Vec::with_capacity(count as usize);
                
                for _ in 0..count {
                    offsets.push(cursor.read_u32::<LittleEndian>()?);
                }
                
                let mut values = Vec::new();
                for offset in offsets {
                    cursor.seek(SeekFrom::Start(start_pos + offset as u64))?;
                    values.push(self.read_param(cursor, hash_table, hash_start, ref_start)?);
                }
                
                Ok(ParamValue::List(ParamList { values }))
            }
            12 => {
                // struct
                let start_pos = cursor.position() - 1;
                let size = cursor.read_i32::<LittleEndian>()?;
                let struct_ref_offset = cursor.read_i32::<LittleEndian>()?;
                
                // Read reference table entries
                cursor.seek(SeekFrom::Start((ref_start + struct_ref_offset) as u64))?;
                let mut hash_offsets = Vec::new();
                
                for _ in 0..size {
                    let hash_index = cursor.read_i32::<LittleEndian>()?;
                    let param_offset = cursor.read_i32::<LittleEndian>()?;
                    hash_offsets.push((hash_index, param_offset));
                }
                
                // Sort by hash index for consistent ordering
                hash_offsets.sort_by_key(|&(hash_index, _)| hash_index);
                
                let mut fields = IndexMap::new();
                for (hash_index, param_offset) in hash_offsets {
                    if hash_index >= 0 && (hash_index as usize) < hash_table.len() {
                        cursor.seek(SeekFrom::Start(start_pos + param_offset as u64))?;
                        let hash = hash_table[hash_index as usize];
                        let value = self.read_param(cursor, hash_table, hash_start, ref_start)?;
                        fields.insert(hash, value);
                    }
                }
                
                Ok(ParamValue::Struct(ParamStruct {
                    type_hash: 0x0,
                    fields,
                }))
            }
            _ => {
                // Handle unknown types like the JavaScript parser
                // Try to determine a reasonable default value size based on type number
                let default_size = if type_byte < 50 { 4 } else if type_byte < 100 { 8 } else { 12 };
                
                // Skip the unknown data
                for _ in 0..default_size {
                    if cursor.read_u8().is_err() {
                        break;
                    }
                }
                
                // Return a placeholder value that won't break the tree
                Ok(ParamValue::U32(type_byte as u32)) // Store the unknown type as a U32 for now
            }
        }
    }

    pub fn get_root(&self) -> Option<&ParamNode> {
        self.root.as_ref()
    }

    #[allow(dead_code)]
    pub fn get_root_mut(&mut self) -> Option<&mut ParamNode> {
        self.root.as_mut()
    }

    /// Parse a path like "root[0][1][2]" into indices
    pub fn parse_node_path(&self, path: &str) -> Option<Vec<usize>> {
        if path == "root" {
            return Some(vec![]);
        }
        
        let path_parts: Vec<&str> = path.split("[").skip(1).collect(); // Skip "root" part
        let mut indices = Vec::new();
        
        for part in path_parts {
            let index_str = part.trim_end_matches(']');
            if let Ok(index) = index_str.parse::<usize>() {
                indices.push(index);
            } else {
                return None;
            }
        }
        
        Some(indices)
    }
    
    /// Get mutable reference to node at path
    #[allow(dead_code)]
    pub fn get_node_mut(&mut self, path: &str) -> Option<&mut ParamNode> {
        let indices = self.parse_node_path(path)?;
        let root = self.get_root_mut()?;
        root.get_child_mut(&indices)
    }
    
    /// Update a node's key (hash and name) and update the underlying data structure
    pub fn update_node_key(&mut self, path: &str, new_name: String, new_hash: u64) -> bool {
        let indices = match self.parse_node_path(path) {
            Some(indices) => indices,
            None => return false,
        };
        
        if indices.is_empty() {
            // Updating root key
            if let Some(root) = &mut self.root {
                root.name = new_name;
                root.hash = new_hash;
                return true;
            }
            return false;
        }
        
        // For child nodes, we need to update both the node AND the parent struct's field mapping
        Self::update_nested_key(&mut self.root, &indices, new_name, new_hash, 0)
    }
    
    fn update_nested_key(
        node: &mut Option<ParamNode>, 
        indices: &[usize], 
        new_name: String,
        new_hash: u64,
        depth: usize
    ) -> bool {
        if let Some(current_node) = node {
            let current_index = indices[depth];
            
            if depth == indices.len() - 1 {
                // This is the parent of the target node
                match &mut current_node.value {
                    ParamValue::Struct(ref mut s) => {
                        if current_index < current_node.children.len() {
                            let old_hash = current_node.children[current_index].hash;
                            
                            // Remove old field and add with new hash
                            if let Some(field_value) = s.fields.shift_remove(&old_hash) {
                                s.fields.insert(new_hash, field_value);
                            }
                            
                            // Update the child node
                            current_node.children[current_index].name = new_name;
                            current_node.children[current_index].hash = new_hash;
                            return true;
                        }
                    }
                    _ => {} // Lists don't have named keys
                }
            } else {
                // Continue recursing
                if current_index < current_node.children.len() {
                    let mut child_option = Some(std::mem::replace(&mut current_node.children[current_index], ParamNode::new("temp".to_string(), 0, ParamValue::Bool(false))));
                    let result = Self::update_nested_key(
                        &mut child_option, 
                        indices, 
                        new_name, 
                        new_hash,
                        depth + 1
                    );
                    if let Some(updated_child) = child_option {
                        current_node.children[current_index] = updated_child;
                    }
                    return result;
                }
            }
        }
        false
    }

    /// Get the current value of a node at the given path
    pub fn get_node_value(&self, path: &str) -> Option<ParamValue> {
        let indices = match self.parse_node_path(path) {
            Some(indices) => indices,
            None => return None,
        };
        
        if indices.is_empty() {
            // Getting root value
            return self.root.as_ref().map(|r| r.value.clone());
        }
        
        if let Some(root) = &self.root {
            Self::get_param_value_at_path(&root.value, &indices, 0)
        } else {
            None
        }
    }
    
    /// Get a ParamValue at a specific path in the data structure
    fn get_param_value_at_path(value: &ParamValue, indices: &[usize], depth: usize) -> Option<ParamValue> {
        if depth >= indices.len() {
            return Some(value.clone());
        }
        
        let current_index = indices[depth];
        
        match value {
            ParamValue::Struct(s) => {
                if let Some((_, field_value)) = s.fields.iter().nth(current_index) {
                    if depth == indices.len() - 1 {
                        Some(field_value.clone())
                    } else {
                        Self::get_param_value_at_path(field_value, indices, depth + 1)
                    }
                } else {
                    None
                }
            }
            ParamValue::List(l) => {
                if current_index < l.values.len() {
                    if depth == indices.len() - 1 {
                        Some(l.values[current_index].clone())
                    } else {
                        Self::get_param_value_at_path(&l.values[current_index], indices, depth + 1)
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Update a node's value and update the underlying data structure
    pub fn update_node_value(&mut self, path: &str, new_value: ParamValue) -> bool {
        let indices = match self.parse_node_path(path) {
            Some(indices) => indices,
            None => return false,
        };
        
        if indices.is_empty() {
            // Updating root
            if let Some(root) = &mut self.root {
                root.value = new_value;
                return true;
            }
            return false;
        }
        
        // CRITICAL FIX: Update the underlying ParamValue structure directly
        // This ensures that the save process will see the updated values
        if let Some(root) = &mut self.root {
            if Self::update_param_value_directly(&mut root.value, &indices, new_value.clone(), 0) {
                // Also update the display tree to keep UI in sync
                Self::update_nested_value(&mut self.root, &indices, new_value, 0);
                return true;
            }
        }
        
        false
    }
    
    /// Update the underlying ParamValue structure directly (not just the display tree)
    fn update_param_value_directly(
        value: &mut ParamValue,
        indices: &[usize],
        new_value: ParamValue,
        depth: usize
    ) -> bool {
        if depth >= indices.len() {
            return false;
        }
        
        let current_index = indices[depth];
        
        if depth == indices.len() - 1 {
            // This is the final level - update the actual value
            match value {
                ParamValue::Struct(ref mut s) => {
                    // Find the field by index in the struct
                    if let Some((field_hash, _)) = s.fields.iter().nth(current_index) {
                        let field_hash = *field_hash; // Copy the hash
                        s.fields.insert(field_hash, new_value);
                        return true;
                    }
                }
                ParamValue::List(ref mut l) => {
                    if current_index < l.values.len() {
                        l.values[current_index] = new_value;
                        return true;
                    }
                }
                _ => return false,
            }
        } else {
            // Continue recursing
            match value {
                ParamValue::Struct(ref mut s) => {
                    if let Some((_, field_value)) = s.fields.iter_mut().nth(current_index) {
                        return Self::update_param_value_directly(field_value, indices, new_value, depth + 1);
                    }
                }
                ParamValue::List(ref mut l) => {
                    if current_index < l.values.len() {
                        return Self::update_param_value_directly(&mut l.values[current_index], indices, new_value, depth + 1);
                    }
                }
                _ => return false,
            }
        }
        
        false
    }

    /// Update a node's value and update the underlying data structure
    fn update_nested_value(
        node: &mut Option<ParamNode>, 
        indices: &[usize], 
        new_value: ParamValue, 
        depth: usize
    ) -> bool {
        if let Some(current_node) = node {
            let current_index = indices[depth];
            
            if depth == indices.len() - 1 {
                // This is the parent of the target node
                match &mut current_node.value {
                    ParamValue::Struct(ref mut s) => {
                        if current_index < current_node.children.len() {
                            let child_hash = current_node.children[current_index].hash;
                            
                            // Update the field in the struct
                            s.fields.insert(child_hash, new_value.clone());
                            
                            // Update the child node
                            current_node.children[current_index].value = new_value;
                            
                            return true;
                        }
                    }
                    ParamValue::List(ref mut l) => {
                        if current_index < current_node.children.len() && current_index < l.values.len() {
                            // Update the item in the list
                            l.values[current_index] = new_value.clone();
                            
                            // Update the child node
                            current_node.children[current_index].value = new_value;
                            
                            return true;
                        }
                    }
                    _ => return false,
                }
            } else {
                // Continue recursing
                if current_index < current_node.children.len() {
                    let mut child_option = Some(std::mem::replace(&mut current_node.children[current_index], ParamNode::new("temp".to_string(), 0, ParamValue::Bool(false))));
                    let result = Self::update_nested_value(
                        &mut child_option, 
                        indices, 
                        new_value, 
                        depth + 1
                    );
                    if let Some(updated_child) = child_option {
                        current_node.children[current_index] = updated_child;
                    }
                    return result;
                }
            }
        }
        false
    }

    pub fn get_filename(&self) -> &str {
        &self.filename
    }
    
    /// Rebuild the tree from the current root data structure
    /// This ensures the display tree is synchronized with the underlying data
    #[allow(dead_code)]
    pub fn rebuild_tree(&mut self) {
        if let Some(root_value) = &self.root.as_ref().map(|n| n.value.clone()) {
            self.root = Some(ParamNode::from_value(0x0, root_value.clone(), &self.hash_labels));
        }
    }
    
    /// Rebuild the tree with updated hash labels
    /// Call this after loading new labels to update all field names
    pub fn rebuild_tree_with_labels(&mut self) {
        if let Some(root_value) = &self.root.as_ref().map(|n| n.value.clone()) {
            self.root = Some(ParamNode::from_value(0x0, root_value.clone(), &self.hash_labels));
        }
    }
    
    /// Save the current parameter file to binary format
    pub fn save(&self, output_path: &str) -> Result<()> {
        let root = self.get_root().ok_or_else(|| anyhow!("No data to save"))?;
        
        // Step 1: Build hash table exactly like paracobNET
        // CRITICAL: paracobNET starts with WriteHash(0) then calls IterateHashes
        let mut hash_table = Vec::new();
        let mut hash_to_index = HashMap::new();
        
        // Start with hash 0 (like paracobNET's WriteHash(0) call)
        self.write_hash(0, &mut hash_table, &mut hash_to_index);
        
        // Now collect all hashes in the exact same order as paracobNET
        self.iterate_hashes(&root.value, &mut hash_table, &mut hash_to_index);
        
        // Step 2: Write parameters with deferred resolution (like paracobNET Write method)
        let mut param_data = Vec::new();
        let mut ref_entries = Vec::new(); // Mixed list of strings and RefTableEntries
        let mut struct_ref_entries = HashMap::new(); // Maps struct hash to RefTableEntry index
        let mut unresolved_structs = Vec::new(); // (position, struct_hash)
        let mut unresolved_strings = Vec::new(); // (position, string)
        
        self.write_param_value(
            &root.value, 
            &mut param_data, 
            &hash_to_index, 
            &mut ref_entries,
            &mut struct_ref_entries,
            &mut unresolved_structs,
            &mut unresolved_strings
        )?;
        
        // Step 3: Skip merging for now to ensure compatibility
        // self.merge_ref_tables(&mut ref_entries, &mut struct_ref_entries, &mut unresolved_structs);
        
        // Step 4: Write reference table (like WriteRefTables)
        let mut ref_table = Vec::new();
        let mut string_offsets = HashMap::new();
        let mut ref_table_offsets = HashMap::new(); // Maps RefTableEntry index to offset
        
        for (i, entry) in ref_entries.iter().enumerate() {
            match entry {
                RefEntry::String(s) => {
                    string_offsets.insert(s.clone(), ref_table.len());
                    ref_table.extend_from_slice(s.as_bytes());
                    ref_table.push(0); // null terminator
                }
                RefEntry::StructRef(entries) => {
                    ref_table_offsets.insert(i, ref_table.len());
                    for (hash_index, param_offset) in entries {
                        ref_table.write_i32::<LittleEndian>(*hash_index)?;
                        ref_table.write_i32::<LittleEndian>(*param_offset)?;
                    }
                }
            }
        }
        
        // Step 5: Resolve struct and string references (like ResolveStructStringRefs)
        let mut param_cursor = Cursor::new(&mut param_data);
        
        // Resolve struct references
        for (position, struct_id) in unresolved_structs {
            if let Some(ref_entry_index) = struct_ref_entries.get(&struct_id) {
                if let Some(ref_table_offset) = ref_table_offsets.get(ref_entry_index) {
                    param_cursor.seek(SeekFrom::Start(position as u64))?;
                    param_cursor.write_i32::<LittleEndian>(*ref_table_offset as i32)?;
                }
            }
        }
        
        // Resolve string references
        for (position, string) in unresolved_strings {
            if let Some(offset) = string_offsets.get(&string) {
                param_cursor.seek(SeekFrom::Start(position as u64))?;
                param_cursor.write_i32::<LittleEndian>(*offset as i32)?;
            }
        }
        
        // Step 6: Build hash table
        let mut hash_data = Vec::new();
        for hash in hash_table {
            hash_data.write_u64::<LittleEndian>(hash)?;
        }
        
        // Step 7: Write complete file (like the final assembly in Start())
        let mut output = Vec::new();
        
        // Write header
        output.extend_from_slice(b"paracobn");
        output.write_i32::<LittleEndian>(hash_data.len() as i32)?; // hash table size
        output.write_i32::<LittleEndian>(ref_table.len() as i32)?;  // ref table size
        
        // Write hash table
        output.extend(hash_data);
        
        // Write reference table
        output.extend(ref_table);
        
        // Write parameter data
        output.extend(param_data);
        
        // Write to file
        std::fs::write(output_path, output)?;
        Ok(())
    }
    
    /// Collect hashes like paracobNET's IterateHashes method
    /// CRITICAL: This must match the exact order that paracobNET processes hashes
    fn iterate_hashes(&self, value: &ParamValue, hash_table: &mut Vec<u64>, hash_to_index: &mut HashMap<u64, usize>) {
        match value {
            ParamValue::Struct(s) => {
                // CRITICAL: paracobNET's IterateHashes processes struct fields in NATURAL ORDER (not sorted!)
                // Only the Write method sorts them - this is the key difference!
                for (field_hash, field_value) in &s.fields {
                    self.write_hash(*field_hash, hash_table, hash_to_index);
                    self.iterate_hashes(field_value, hash_table, hash_to_index);
                }
            }
            ParamValue::List(l) => {
                for item in &l.values {
                    self.iterate_hashes(item, hash_table, hash_to_index);
                }
            }
            ParamValue::Hash(h) => {
                self.write_hash(*h, hash_table, hash_to_index);
            }
            _ => {}
        }
    }
    
    /// Write hash like paracobNET's WriteHash method
    fn write_hash(&self, hash: u64, hash_table: &mut Vec<u64>, hash_to_index: &mut HashMap<u64, usize>) {
        if !hash_to_index.contains_key(&hash) {
            hash_to_index.insert(hash, hash_table.len());
            hash_table.push(hash);
        }
    }
    
    /// Write parameter value like paracobNET's Write method with deferred resolution
    fn write_param_value(
        &self,
        value: &ParamValue,
        output: &mut Vec<u8>,
        hash_to_index: &HashMap<u64, usize>,
        ref_entries: &mut Vec<RefEntry>,
        struct_ref_entries: &mut HashMap<u64, usize>, // Maps struct hash to ref entry index
        unresolved_structs: &mut Vec<(usize, u64)>, // (position, struct_hash)
        unresolved_strings: &mut Vec<(usize, String)>
    ) -> Result<()> {
        match value {
            ParamValue::Bool(v) => {
                output.write_u8(1)?; // type
                output.write_u8(if *v { 1 } else { 0 })?;
            }
            ParamValue::I8(v) => {
                output.write_u8(2)?; // type
                output.write_i8(*v)?;
            }
            ParamValue::U8(v) => {
                output.write_u8(3)?; // type
                output.write_u8(*v)?;
            }
            ParamValue::I16(v) => {
                output.write_u8(4)?; // type
                output.write_i16::<LittleEndian>(*v)?;
            }
            ParamValue::U16(v) => {
                output.write_u8(5)?; // type
                output.write_u16::<LittleEndian>(*v)?;
            }
            ParamValue::I32(v) => {
                output.write_u8(6)?; // type
                output.write_i32::<LittleEndian>(*v)?;
            }
            ParamValue::U32(v) => {
                output.write_u8(7)?; // type
                output.write_u32::<LittleEndian>(*v)?;
            }
            ParamValue::F32(v) => {
                output.write_u8(8)?; // type
                output.write_f32::<LittleEndian>(*v)?;
            }
            ParamValue::Hash(v) => {
                output.write_u8(9)?; // type
                let index = hash_to_index.get(v).ok_or_else(|| anyhow!("Hash not found in hash table"))?;
                output.write_u32::<LittleEndian>(*index as u32)?;
            }
            ParamValue::String(v) => {
                output.write_u8(10)?; // type
                
                // Add string to ref_entries if not already present (like AppendRefTableString)
                let string_exists = ref_entries.iter().any(|entry| {
                    matches!(entry, RefEntry::String(s) if s == v)
                });
                if !string_exists {
                    ref_entries.push(RefEntry::String(v.clone()));
                }
                
                // Record unresolved string reference (position BEFORE writing placeholder)
                unresolved_strings.push((output.len(), v.clone()));
                output.write_i32::<LittleEndian>(0)?; // placeholder
            }
            ParamValue::List(l) => {
                output.write_u8(11)?; // type
                let start_pos = output.len() - 1;
                output.write_i32::<LittleEndian>(l.values.len() as i32)?; // count
                
                // Write placeholder offsets
                let offset_start = output.len();
                for _ in 0..l.values.len() {
                    output.write_u32::<LittleEndian>(0)?; // placeholder
                }
                
                // Write actual values and update offsets
                let mut offsets = Vec::new();
                for item in &l.values {
                    let item_offset = output.len() - start_pos;
                    offsets.push(item_offset as u32);
                    self.write_param_value(item, output, hash_to_index, ref_entries, struct_ref_entries, unresolved_structs, unresolved_strings)?;
                }
                
                // Update the offset table
                let mut temp_cursor = Cursor::new(&mut output[offset_start..]);
                for offset in offsets {
                    temp_cursor.write_u32::<LittleEndian>(offset)?;
                }
            }
            ParamValue::Struct(s) => {
                output.write_u8(12)?; // type
                let start_pos = output.len() - 1;
                output.write_i32::<LittleEndian>(s.fields.len() as i32)?; // size
                
                // Create a RefTableEntry for this struct (like paracobNET)
                // We'll handle deduplication later in merge_ref_tables
                let ref_entry_index = ref_entries.len();
                ref_entries.push(RefEntry::StructRef(Vec::new())); // Will be filled later
                
                // Use the struct's memory address as a unique identifier for now
                // This will be used for deduplication in merge_ref_tables
                let struct_id = s as *const ParamStruct as u64;
                struct_ref_entries.insert(struct_id, ref_entry_index);
                
                // Record unresolved struct reference (position BEFORE writing placeholder)
                unresolved_structs.push((output.len(), struct_id));
                output.write_i32::<LittleEndian>(0)?; // placeholder for ref table offset
                
                // Sort fields by hash for consistent ordering (like paracobNET)
                let mut sorted_fields: Vec<_> = s.fields.iter().collect();
                sorted_fields.sort_by_key(|(hash, _)| *hash);
                
                // Write each field and record its offset in the RefTableEntry
                let mut hash_offsets = Vec::new();
                for (field_hash, field_value) in sorted_fields {
                    let hash_index = *hash_to_index.get(field_hash).ok_or_else(|| anyhow!("Field hash not found"))?;
                    let param_offset = output.len() - start_pos;
                    hash_offsets.push((hash_index as i32, param_offset as i32));
                    
                    self.write_param_value(field_value, output, hash_to_index, ref_entries, struct_ref_entries, unresolved_structs, unresolved_strings)?;
                }
                
                // Update the RefTableEntry with the hash offsets
                if let RefEntry::StructRef(entries) = &mut ref_entries[ref_entry_index] {
                    *entries = hash_offsets;
                }
            }
        }
        Ok(())
    }
    
    /// Calculate a hash for struct based on its field pattern (for deduplication)
    #[allow(dead_code)]
    fn calculate_struct_hash(&self, s: &ParamStruct) -> u64 {
        let mut sorted_fields: Vec<_> = s.fields.keys().collect();
        sorted_fields.sort();
        
        // Simple hash based on field hashes - this creates a unique signature for the struct pattern
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for field_hash in sorted_fields {
            std::hash::Hasher::write_u64(&mut hasher, *field_hash);
        }
        std::hash::Hasher::finish(&hasher)
    }
    
    /// Merge duplicate struct reference entries like paracobNET's MergeRefTables
    #[allow(dead_code)]
    fn merge_ref_tables(
        &self, 
        ref_entries: &mut Vec<RefEntry>, 
        struct_ref_entries: &mut HashMap<u64, usize>,
        _unresolved_structs: &mut Vec<(usize, u64)>
    ) {
        let mut i = 0;
        while i < ref_entries.len() {
            if let RefEntry::StructRef(current_entries) = &ref_entries[i] {
                // Look for an earlier identical struct reference
                let mut found_duplicate = None;
                for j in 0..i {
                    if let RefEntry::StructRef(earlier_entries) = &ref_entries[j] {
                        if current_entries == earlier_entries {
                            found_duplicate = Some(j);
                            break;
                        }
                    }
                }
                
                if let Some(duplicate_index) = found_duplicate {
                    // Update struct_ref_entries to point to the earlier entry
                    for (_struct_hash, ref_entry_index) in struct_ref_entries.iter_mut() {
                        if *ref_entry_index == i {
                            *ref_entry_index = duplicate_index;
                        } else if *ref_entry_index > i {
                            *ref_entry_index -= 1; // Adjust for removal
                        }
                    }
                    
                    // Remove the duplicate entry
                    ref_entries.remove(i);
                    continue; // Don't increment i since we removed an element
                }
            }
            i += 1;
        }
    }
}

impl Default for ParamFile {
    fn default() -> Self {
        Self::new()
    }
} 