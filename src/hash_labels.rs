use std::collections::HashMap;
use anyhow::Result;

// CRC32 table from paracobNET
const CRC32_TABLE: [u32; 256] = [
    0x00000000, 0x77073096, 0xee0e612c, 0x990951ba, 0x076dc419, 0x706af48f, 0xe963a535, 0x9e6495a3,
    0x0edb8832, 0x79dcb8a4, 0xe0d5e91e, 0x97d2d988, 0x09b64c2b, 0x7eb17cbd, 0xe7b82d07, 0x90bf1d91,
    0x1db71064, 0x6ab020f2, 0xf3b97148, 0x84be41de, 0x1adad47d, 0x6ddde4eb, 0xf4d4b551, 0x83d385c7,
    0x136c9856, 0x646ba8c0, 0xfd62f97a, 0x8a65c9ec, 0x14015c4f, 0x63066cd9, 0xfa0f3d63, 0x8d080df5,
    0x3b6e20c8, 0x4c69105e, 0xd56041e4, 0xa2677172, 0x3c03e4d1, 0x4b04d447, 0xd20d85fd, 0xa50ab56b,
    0x35b5a8fa, 0x42b2986c, 0xdbbbc9d6, 0xacbcf940, 0x32d86ce3, 0x45df5c75, 0xdcd60dcf, 0xabd13d59,
    0x26d930ac, 0x51de003a, 0xc8d75180, 0xbfd06116, 0x21b4f4b5, 0x56b3c423, 0xcfba9599, 0xb8bda50f,
    0x2802b89e, 0x5f058808, 0xc60cd9b2, 0xb10be924, 0x2f6f7c87, 0x58684c11, 0xc1611dab, 0xb6662d3d,
    0x76dc4190, 0x01db7106, 0x98d220bc, 0xefd5102a, 0x71b18589, 0x06b6b51f, 0x9fbfe4a5, 0xe8b8d433,
    0x7807c9a2, 0x0f00f934, 0x9609a88e, 0xe10e9818, 0x7f6a0dbb, 0x086d3d2d, 0x91646c97, 0xe6635c01,
    0x6b6b51f4, 0x1c6c6162, 0x856530d8, 0xf262004e, 0x6c0695ed, 0x1b01a57b, 0x8208f4c1, 0xf50fc457,
    0x65b0d9c6, 0x12b7e950, 0x8bbeb8ea, 0xfcb9887c, 0x62dd1ddf, 0x15da2d49, 0x8cd37cf3, 0xfbd44c65,
    0x4db26158, 0x3ab551ce, 0xa3bc0074, 0xd4bb30e2, 0x4adfa541, 0x3dd895d7, 0xa4d1c46d, 0xd3d6f4fb,
    0x4369e96a, 0x346ed9fc, 0xad678846, 0xda60b8d0, 0x44042d73, 0x33031de5, 0xaa0a4c5f, 0xdd0d7cc9,
    0x5005713c, 0x270241aa, 0xbe0b1010, 0xc90c2086, 0x5768b525, 0x206f85b3, 0xb966d409, 0xce61e49f,
    0x5edef90e, 0x29d9c998, 0xb0d09822, 0xc7d7a8b4, 0x59b33d17, 0x2eb40d81, 0xb7bd5c3b, 0xc0ba6cad,
    0xedb88320, 0x9abfb3b6, 0x03b6e20c, 0x74b1d29a, 0xead54739, 0x9dd277af, 0x04db2615, 0x73dc1683,
    0xe3630b12, 0x94643b84, 0x0d6d6a3e, 0x7a6a5aa8, 0xe40ecf0b, 0x9309ff9d, 0x0a00ae27, 0x7d079eb1,
    0xf00f9344, 0x8708a3d2, 0x1e01f268, 0x6906c2fe, 0xf762575d, 0x806567cb, 0x196c3671, 0x6e6b06e7,
    0xfed41b76, 0x89d32be0, 0x10da7a5a, 0x67dd4acc, 0xf9b9df6f, 0x8ebeeff9, 0x17b7be43, 0x60b08ed5,
    0xd6d6a3e8, 0xa1d1937e, 0x38d8c2c4, 0x4fdff252, 0xd1bb67f1, 0xa6bc5767, 0x3fb506dd, 0x48b2364b,
    0xd80d2bda, 0xaf0a1b4c, 0x36034af6, 0x41047a60, 0xdf60efc3, 0xa867df55, 0x316e8eef, 0x4669be79,
    0xcb61b38c, 0xbc66831a, 0x256fd2a0, 0x5268e236, 0xcc0c7795, 0xbb0b4703, 0x220216b9, 0x5505262f,
    0xc5ba3bbe, 0xb2bd0b28, 0x2bb45a92, 0x5cb36a04, 0xc2d7ffa7, 0xb5d0cf31, 0x2cd99e8b, 0x5bdeae1d,
    0x9b64c2b0, 0xec63f226, 0x756aa39c, 0x026d930a, 0x9c0906a9, 0xeb0e363f, 0x72076785, 0x05005713,
    0x95bf4a82, 0xe2b87a14, 0x7bb12bae, 0x0cb61b38, 0x92d28e9b, 0xe5d5be0d, 0x7cdcefb7, 0x0bdbdf21,
    0x86d3d2d4, 0xf1d4e242, 0x68ddb3f8, 0x1fda836e, 0x81be16cd, 0xf6b9265b, 0x6fb077e1, 0x18b74777,
    0x88085ae6, 0xff0f6a70, 0x66063bca, 0x11010b5c, 0x8f659eff, 0xf862ae69, 0x616bffd3, 0x166ccf45,
    0xa00ae278, 0xd70dd2ee, 0x4e048354, 0x3903b3c2, 0xa7672661, 0xd06016f7, 0x4969474d, 0x3e6e77db,
    0xaed16a4a, 0xd9d65adc, 0x40df0b66, 0x37d83bf0, 0xa9bcae53, 0xdebb9ec5, 0x47b2cf7f, 0x30b5ffe9,
    0xbdbdf21c, 0xcabac28a, 0x53b39330, 0x24b4a3a6, 0xbad03605, 0xcdd70693, 0x54de5729, 0x23d967bf,
    0xb3667a2e, 0xc4614ab8, 0x5d681b02, 0x2a6f2b94, 0xb40bbe37, 0xc30c8ea1, 0x5a05df1b, 0x2d02ef8d,
];

fn crc32(word: &str) -> u32 {
    let mut hash = 0xffffffff;
    for byte in word.bytes() {
        hash = (hash >> 8) ^ CRC32_TABLE[((hash ^ byte as u32) & 0xff) as usize];
    }
    !hash
}

pub struct HashLabels {
    labels: HashMap<u64, String>,
    reverse_labels: HashMap<String, u64>,
    // Cache for hash_to_string lookups to improve performance
    hash_to_string_cache: std::cell::RefCell<HashMap<u64, String>>,
}

impl HashLabels {
    pub fn new() -> Self {
        Self {
            labels: HashMap::new(),
            reverse_labels: HashMap::new(),
            hash_to_string_cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    pub fn load_from_csv(&mut self, csv_content: &str) -> Result<usize> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true) // Allow records with varying number of fields
            .from_reader(csv_content.as_bytes());

        let mut count = 0;
        
        for result in reader.records() {
            let record = result?;
            // Only process records with exactly 2 fields
            if record.len() == 2 {
                if let (Some(hash_str), Some(label)) = (record.get(0), record.get(1)) {
                    // Normalize the hash string by removing leading zeros after 0x
                    let normalized_hash_str = if hash_str.starts_with("0x") || hash_str.starts_with("0X") {
                        let hex_part = &hash_str[2..];
                        // Remove leading zeros but keep at least one digit
                        let trimmed = hex_part.trim_start_matches('0');
                        if trimmed.is_empty() {
                            "0x0".to_string()
                        } else {
                            format!("0x{}", trimmed)
                        }
                    } else {
                        hash_str.to_string()
                    };
                    
                    if let Ok(hash) = u64::from_str_radix(normalized_hash_str.trim_start_matches("0x"), 16) {
                        self.labels.insert(hash, label.to_string());
                        self.reverse_labels.insert(label.to_string(), hash);
                        count += 1;
                    }
                    // Silently skip invalid hash formats
                }
            }
            // Silently skip malformed records
        }
        
        // Clear cache since we loaded new labels
        self.hash_to_string_cache.borrow_mut().clear();
        
        Ok(count)
    }

    pub fn get_label(&self, hash: u64) -> Option<&String> {
        self.labels.get(&hash)
    }

    #[allow(dead_code)]
    pub fn get_hash(&self, label: &str) -> Option<u64> {
        self.reverse_labels.get(label).copied()
    }

    pub fn hash_to_string(&self, hash: u64) -> String {
        // Check cache first for performance
        if let Some(cached) = self.hash_to_string_cache.borrow().get(&hash) {
            return cached.clone();
        }
        
        let result = {
            // First try direct lookup like the original prcEditor
            if let Some(label) = self.get_label(hash) {
                label.clone()
            } else {
                // If direct lookup fails, try different masking approaches for compatibility
                let variants = [
                    hash & 0x00FFFFFFFFFFFFFF,             // 56-bit mask (remove top 8 bits)
                    hash & 0x0000FFFFFFFFFFFF,             // 48-bit mask (remove top 16 bits)
                    hash & 0x000000FFFFFFFFFF,             // 40-bit mask (Hash40 format)
                    hash & 0x00000000FFFFFFFF,             // 32-bit mask (CRC32 only)
                    hash | 0xFF00000000000000,             // Set top 8 bits
                    hash | 0xFFFF000000000000,             // Set top 16 bits
                    hash & 0x7FFFFFFFFFFFFFFF,             // Clear sign bit
                    hash | 0x8000000000000000,             // Set sign bit
                ];

                let mut found_label = None;
                for variant in variants {
                    if let Some(label) = self.get_label(variant) {
                        found_label = Some(label.clone());
                        break;
                    }
                }

                // If no label found, return hex representation
                found_label.unwrap_or_else(|| format!("0x{:X}", hash))
            }
        };
        
        // Cache the result for future lookups
        self.hash_to_string_cache.borrow_mut().insert(hash, result.clone());
        result
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    #[allow(dead_code)]
    pub fn get_all_labels(&self) -> &HashMap<u64, String> {
        &self.labels
    }
    
    pub fn get_labels_filtered(&self, filter: &str) -> Vec<(u64, &String)> {
        if filter.is_empty() {
            self.labels.iter().map(|(k, v)| (*k, v)).collect()
        } else {
            let filter_lower = filter.to_lowercase();
            self.labels.iter()
                .filter(|(hash, label)| {
                    label.to_lowercase().contains(&filter_lower) ||
                    format!("{:X}", hash).to_lowercase().contains(&filter_lower)
                })
                .map(|(k, v)| (*k, v))
                .collect()
        }
    }

    /// Generate Hash40 from string using the same algorithm as paracobNET
    /// Hash40 = (string_length << 32) | CRC32(string)
    pub fn string_to_hash40(&self, word: &str) -> u64 {
        let length = word.len() as u64;
        let crc = crc32(word) as u64;
        (length << 32) | crc
    }

    /// Add a new label and automatically generate its hash
    /// If the label already exists, return the existing hash instead of creating a new one
    pub fn add_label(&mut self, label: &str) -> u64 {
        // First check if this label already exists
        if let Some(&existing_hash) = self.reverse_labels.get(label) {
            return existing_hash;
        }
        
        // Only generate new hash if label doesn't exist
        let hash = self.string_to_hash40(label);
        self.labels.insert(hash, label.to_string());
        self.reverse_labels.insert(label.to_string(), hash);
        
        // Clear cache since we added a new label
        self.hash_to_string_cache.borrow_mut().clear();
        
        hash
    }

    /// Save all labels to a CSV file
    pub fn save_to_csv(&self, file_path: &str) -> Result<()> {
        use std::fs::File;
        use std::io::Write;
        
        let mut file = File::create(file_path)?;
        
        // Sort by hash for consistent output
        let mut sorted_labels: Vec<_> = self.labels.iter().collect();
        sorted_labels.sort_by_key(|(hash, _)| *hash);
        
        for (hash, label) in sorted_labels {
            writeln!(file, "0x{:X},{}", hash, label)?;
        }
        
        Ok(())
    }

    /// Add a new label and save to CSV file if provided
    pub fn add_label_and_save(&mut self, label: &str, csv_path: Option<&str>) -> u64 {
        let hash = self.add_label(label);
        
        if let Some(path) = csv_path {
            let _ = self.save_to_csv(path);
        }
        
        hash
    }

    /// Try to parse a string as either a hex hash or a label name
    #[allow(dead_code)]
    pub fn parse_hash_or_label(&self, input: &str) -> Result<u64, String> {
        // Try to parse as hex first
        if input.starts_with("0x") {
            if let Ok(hash) = u64::from_str_radix(&input[2..], 16) {
                return Ok(hash);
            }
        }
        
        // Try to find existing label
        if let Some(&hash) = self.reverse_labels.get(input) {
            return Ok(hash);
        }
        
        // Generate new hash for this label
        let hash = self.string_to_hash40(input);
        Err(format!("Unknown label '{}' - would generate hash 0x{:X}", input, hash))
    }

    /// Add a label for an existing hash value
    pub fn add_label_for_hash(&mut self, hash: u64, label: &str) {
        self.labels.insert(hash, label.to_string());
        self.reverse_labels.insert(label.to_string(), hash);
        
        // Clear cache since we added a new label
        self.hash_to_string_cache.borrow_mut().clear();
    }

    /// Add a label for an existing hash and save to CSV
    pub fn add_label_for_hash_and_save(&mut self, hash: u64, label: &str, csv_path: Option<&str>) -> Result<()> {
        self.add_label_for_hash(hash, label);
        
        if let Some(path) = csv_path {
            self.save_to_csv(path)?;
        }
        
        Ok(())
    }
}

impl Default for HashLabels {
    fn default() -> Self {
        Self::new()
    }
} 