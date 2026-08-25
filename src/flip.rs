use crate::hash_labels::HashLabels;
use crate::param_file::ParamFile;
use crate::param_types::{ParamList, ParamStruct, ParamValue};
use std::path::Path;

/// A `flip.prc` axis tag (`kxy`, `kyz`, `kxyz`, `knone`).
/// Each letter is an animation channel whose sign is flipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AxisMask {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

impl AxisMask {
    pub fn from_label(label: &str) -> Self {
        let lower = label.trim().to_ascii_lowercase();
        let axes = lower.strip_prefix('k').unwrap_or(&lower);
        if axes == "none" || axes.is_empty() {
            return Self::default();
        }
        Self {
            x: axes.contains('x'),
            y: axes.contains('y'),
            z: axes.contains('z'),
        }
    }

    pub fn to_label(self) -> String {
        if !self.x && !self.y && !self.z {
            return "knone".to_string();
        }
        let mut label = String::from("k");
        if self.x {
            label.push('x');
        }
        if self.y {
            label.push('y');
        }
        if self.z {
            label.push('z');
        }
        label
    }
}

/// One list entry: a named item, or an L/R pair with optional transform flags.
#[derive(Debug, Clone)]
pub struct FlipEntry {
    pub lhs_name: String,
    pub rhs_name: Option<String>,
    pub trans: AxisMask,
    pub rot: AxisMask,
    pub scale: bool,
}

impl FlipEntry {
    pub fn pair_name(&self) -> String {
        match &self.rhs_name {
            Some(rhs) if rhs != &self.lhs_name => format!("{} ↔ {}", self.lhs_name, rhs),
            _ => self.lhs_name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnknownList {
    pub name: String,
    pub entries: Vec<FlipEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct FlipPrc {
    pub base_bones: Vec<FlipEntry>,
    pub flip_bones: Vec<FlipEntry>,
    pub single_bones: Vec<FlipEntry>,
    pub pair_bones: Vec<FlipEntry>,
    pub meshes: Vec<FlipEntry>,
    pub pair_materials: Vec<FlipEntry>,
    pub unknown: Vec<UnknownList>,
}

impl FlipPrc {
    pub fn from_param_file(file: &ParamFile) -> Option<Self> {
        let root = file.get_root()?;
        let ParamValue::Struct(root_struct) = &root.value else {
            return None;
        };
        Some(Self::from_struct(root_struct, &file.hash_labels))
    }

    pub fn from_struct(root: &ParamStruct, labels: &HashLabels) -> Self {
        let mut flip = Self::default();
        for (hash, value) in &root.fields {
            let name = labels.hash_to_string(*hash);
            let entries = parse_list(value, labels);
            match name.as_str() {
                "base_bones" => flip.base_bones = entries,
                "flip_bones" => flip.flip_bones = entries,
                "single_bones" => flip.single_bones = entries,
                "pair_bones" => flip.pair_bones = entries,
                "meshes" => flip.meshes = entries,
                "pair_materials" => flip.pair_materials = entries,
                _ => {
                    if !entries.is_empty() {
                        flip.unknown.push(UnknownList {
                            name,
                            entries,
                        });
                    }
                }
            }
        }
        flip
    }

    pub fn looks_like_flip(&self) -> bool {
        !self.flip_bones.is_empty()
            || !self.pair_materials.is_empty()
            || !self.meshes.is_empty()
            || !self.pair_bones.is_empty()
            || !self.single_bones.is_empty()
            || !self.base_bones.is_empty()
            || !self.unknown.is_empty()
    }

    pub fn list(&self, name: &str) -> &[FlipEntry] {
        match name {
            "flip_bones" => &self.flip_bones,
            "base_bones" => &self.base_bones,
            "single_bones" => &self.single_bones,
            "pair_bones" => &self.pair_bones,
            "meshes" => &self.meshes,
            "pair_materials" => &self.pair_materials,
            other => self
                .unknown
                .iter()
                .find(|u| u.name.eq_ignore_ascii_case(other))
                .map(|u| u.entries.as_slice())
                .unwrap_or(&[]),
        }
    }

    /// Custom/unlabeled lists with L/R pairs (e.g. havel/haver) behave like flip_bones.
    pub fn is_flip_style_list(&self, name: &str) -> bool {
        if name == "flip_bones" {
            return true;
        }
        if matches!(
            name,
            "pair_bones" | "base_bones" | "single_bones" | "meshes" | "pair_materials"
        ) {
            return false;
        }
        if matches!(
            FlipListKind::for_category(name),
            FlipListKind::MeshPairs | FlipListKind::MaterialPairs
        ) {
            return false;
        }
        Self::list_has_pairs(self.list(name))
    }

    pub fn list_kind(&self, name: &str) -> FlipListKind {
        match name {
            "base_bones" | "single_bones" => FlipListKind::BoneSingles,
            "meshes" => FlipListKind::MeshPairs,
            "pair_materials" => FlipListKind::MaterialPairs,
            "flip_bones" | "pair_bones" => FlipListKind::BonePairs,
            _ => {
                let inferred = FlipListKind::for_category(name);
                if matches!(
                    inferred,
                    FlipListKind::MeshPairs | FlipListKind::MaterialPairs
                ) {
                    return inferred;
                }
                if Self::list_has_pairs(self.list(name)) || self.list(name).is_empty() {
                    FlipListKind::BonePairs
                } else {
                    FlipListKind::BoneSingles
                }
            }
        }
    }

    pub fn category_names(&self) -> Vec<String> {
        let mut names = vec![
            "flip_bones".to_string(),
            "base_bones".to_string(),
            "single_bones".to_string(),
            "pair_bones".to_string(),
            "meshes".to_string(),
            "pair_materials".to_string(),
        ];
        for unknown in &self.unknown {
            if !names.iter().any(|n| n == &unknown.name) {
                names.push(unknown.name.clone());
            }
        }
        names
    }

    pub fn all_entries(&self) -> impl Iterator<Item = &FlipEntry> {
        self.flip_bones
            .iter()
            .chain(&self.base_bones)
            .chain(&self.single_bones)
            .chain(&self.pair_bones)
            .chain(&self.meshes)
            .chain(&self.pair_materials)
            .chain(self.unknown.iter().flat_map(|list| list.entries.iter()))
    }

    pub fn list_bone_names(entries: &[FlipEntry]) -> Vec<String> {
        let mut names = Vec::new();
        for entry in entries {
            names.push(entry.lhs_name.clone());
            if let Some(rhs) = &entry.rhs_name {
                names.push(rhs.clone());
            }
        }
        names
    }

    pub fn affected_bone_names(&self) -> Vec<String> {
        let mut names = Self::list_bone_names(&self.flip_bones);
        names.extend(Self::list_bone_names(&self.base_bones));
        names.extend(Self::list_bone_names(&self.single_bones));
        names.extend(Self::list_bone_names(&self.pair_bones));
        for u in &self.unknown {
            names.extend(Self::list_bone_names(&u.entries));
        }
        names
    }

    pub(crate) fn list_has_pairs(entries: &[FlipEntry]) -> bool {
        entries.iter().any(|e| {
            e.rhs_name
                .as_ref()
                .map(|r| r != &e.lhs_name)
                .unwrap_or(false)
        })
    }
}

pub fn is_flip_prc(file: &ParamFile, path: Option<&Path>) -> bool {
    if let Some(path) = path {
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("flip.prc"))
            .unwrap_or(false)
        {
            return true;
        }
    }
    FlipPrc::from_param_file(file)
        .map(|f| f.looks_like_flip())
        .unwrap_or(false)
}

/// `fighter/<char>/motion/body/cXX` next to `flip.prc`, used for wait/eyelid anims.
pub fn suggested_motion_folder(prc_path: &Path) -> Option<std::path::PathBuf> {
    let costume = prc_path.parent()?;
    let body = costume.parent()?;
    let motion = body.parent()?;
    let motion_name = motion.file_name()?.to_str()?;
    let body_name = body.file_name()?.to_str()?;
    if !motion_name.eq_ignore_ascii_case("motion") || !body_name.eq_ignore_ascii_case("body") {
        return None;
    }
    Some(costume.to_path_buf())
}

/// If this PRC lives at `fighter/<char>/motion/body/cXX/flip.prc`,
/// suggest `fighter/<char>/model/body/cXX`.
pub fn suggested_model_folder(prc_path: &Path) -> Option<std::path::PathBuf> {
    let costume = prc_path.parent()?;
    let body = costume.parent()?;
    let motion = body.parent()?;
    let fighter_char = motion.parent()?;
    let motion_name = motion.file_name()?.to_str()?;
    let body_name = body.file_name()?.to_str()?;
    if !motion_name.eq_ignore_ascii_case("motion") || !body_name.eq_ignore_ascii_case("body") {
        return None;
    }
    Some(
        fighter_char
            .join("model")
            .join("body")
            .join(costume.file_name()?),
    )
}

fn parse_list(value: &ParamValue, labels: &HashLabels) -> Vec<FlipEntry> {
    match value {
        ParamValue::List(list) => list
            .values
            .iter()
            .filter_map(|item| parse_entry(item, labels))
            .collect(),
        ParamValue::Struct(s) => vec![parse_struct_entry(s, labels)],
        ParamValue::Hash(h) => vec![FlipEntry {
            lhs_name: labels.hash_to_string(*h),
            rhs_name: None,
            trans: AxisMask::default(),
            rot: AxisMask::default(),
            scale: false,
        }],
        ParamValue::String(s) => vec![FlipEntry {
            lhs_name: s.clone(),
            rhs_name: None,
            trans: AxisMask::default(),
            rot: AxisMask::default(),
            scale: false,
        }],
        _ => Vec::new(),
    }
}

fn parse_entry(value: &ParamValue, labels: &HashLabels) -> Option<FlipEntry> {
    match value {
        ParamValue::Struct(s) => Some(parse_struct_entry(s, labels)),
        ParamValue::Hash(h) => Some(FlipEntry {
            lhs_name: labels.hash_to_string(*h),
            rhs_name: None,
            trans: AxisMask::default(),
            rot: AxisMask::default(),
            scale: false,
        }),
        ParamValue::String(s) => Some(FlipEntry {
            lhs_name: s.clone(),
            rhs_name: None,
            trans: AxisMask::default(),
            rot: AxisMask::default(),
            scale: false,
        }),
        ParamValue::List(list) => parse_nested_pair(list, labels),
        _ => None,
    }
}

fn parse_nested_pair(list: &ParamList, labels: &HashLabels) -> Option<FlipEntry> {
    let names: Vec<String> = list
        .values
        .iter()
        .filter_map(|v| match v {
            ParamValue::Hash(h) => Some(labels.hash_to_string(*h)),
            ParamValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    match names.len() {
        0 => None,
        1 => Some(FlipEntry {
            lhs_name: names[0].clone(),
            rhs_name: None,
            trans: AxisMask::default(),
            rot: AxisMask::default(),
            scale: false,
        }),
        _ => Some(FlipEntry {
            lhs_name: names[0].clone(),
            rhs_name: Some(names[1].clone()),
            trans: AxisMask::default(),
            rot: AxisMask::default(),
            scale: false,
        }),
    }
}

fn parse_struct_entry(s: &ParamStruct, labels: &HashLabels) -> FlipEntry {
    let mut lhs = None;
    let mut rhs = None;
    let mut trans = AxisMask::default();
    let mut rot = AxisMask::default();
    let mut scale = false;
    let mut other_hashes = Vec::new();

    for (hash, value) in &s.fields {
        let key = labels.hash_to_string(*hash);
        match (key.as_str(), value) {
            ("lhs_name" | "left" | "name" | "mesh_name" | "bonename" | "start_bonename" | "bone", ParamValue::Hash(h)) => {
                lhs = Some(labels.hash_to_string(*h));
            }
            ("lhs_name" | "left" | "name" | "mesh_name" | "bonename" | "start_bonename" | "bone", ParamValue::String(v)) => {
                lhs = Some(v.clone());
            }
            ("rhs_name" | "right" | "flip_name" | "end_bonename" | "pair_name", ParamValue::Hash(h)) => {
                rhs = Some(labels.hash_to_string(*h));
            }
            ("rhs_name" | "right" | "flip_name" | "end_bonename" | "pair_name", ParamValue::String(v)) => {
                rhs = Some(v.clone());
            }
            ("trans", ParamValue::Hash(h)) => {
                trans = AxisMask::from_label(&labels.hash_to_string(*h));
            }
            ("rot", ParamValue::Hash(h)) => {
                rot = AxisMask::from_label(&labels.hash_to_string(*h));
            }
            ("scale", ParamValue::Bool(v)) => {
                scale = *v;
            }
            (_, ParamValue::Hash(h)) => {
                other_hashes.push(labels.hash_to_string(*h));
            }
            (_, ParamValue::String(v)) => {
                other_hashes.push(v.clone());
            }
            _ => {}
        }
    }

    let (lhs_name, rhs_name) = match (lhs, rhs) {
        (Some(l), r) => (l, r),
        (None, Some(r)) if !other_hashes.is_empty() => (other_hashes[0].clone(), Some(r)),
        (None, Some(r)) => (r, None),
        (None, None) if other_hashes.len() >= 2 => {
            (other_hashes[0].clone(), Some(other_hashes[1].clone()))
        }
        (None, None) if other_hashes.len() == 1 => (other_hashes[0].clone(), None),
        (None, None) => ("?".to_string(), None),
    };

    FlipEntry {
        lhs_name,
        rhs_name,
        trans,
        rot,
        scale,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipListKind {
    BonePairs,
    BoneSingles,
    MeshPairs,
    MaterialPairs,
}

impl FlipListKind {
    pub fn for_category(name: &str) -> Self {
        match name {
            "base_bones" | "single_bones" => Self::BoneSingles,
            "meshes" => Self::MeshPairs,
            "pair_materials" => Self::MaterialPairs,
            _ if name.to_ascii_lowercase().contains("mesh") => Self::MeshPairs,
            _ if name.to_ascii_lowercase().contains("mat") => Self::MaterialPairs,
            "flip_bones" | "pair_bones" => Self::BonePairs,
            _ => Self::BonePairs,
        }
    }

    pub fn allows_pair(self) -> bool {
        !matches!(self, Self::BoneSingles)
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::BonePairs | Self::BoneSingles => "bone",
            Self::MeshPairs => "mesh",
            Self::MaterialPairs => "material",
        }
    }
}

/// Smash vis meshes are `PanM_VIS_O_OBJShape`; flip.prc stores `panm`.
pub fn mesh_flip_key(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    match lower.split_once("_vis") {
        Some((base, _)) if !base.is_empty() => base.to_string(),
        _ => lower,
    }
}

/// Labels written into flip.prc are always lowercase (`GripL` → `gripl`).
pub fn flip_store_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

pub fn guess_pair_name(name: &str, candidates: &[String]) -> Option<String> {
    let mut guesses = Vec::new();
    let lower = name.to_ascii_lowercase();
    if let Some(rest) = lower.strip_suffix("flip") {
        if !rest.is_empty() {
            guesses.push(rest.to_string());
        }
    } else {
        guesses.push(format!("{lower}flip"));
    }
    let replacements = [
        ("left", "right"),
        ("Left", "Right"),
        ("LEFT", "RIGHT"),
        ("right", "left"),
        ("Right", "Left"),
        ("RIGHT", "LEFT"),
        ("_l", "_r"),
        ("_L", "_R"),
        ("_r", "_l"),
        ("_R", "_L"),
    ];
    for (from, to) in replacements {
        if name.contains(from) {
            guesses.push(name.replacen(from, to, 1));
        }
    }
    if let Some(rest) = name.strip_suffix('l') {
        guesses.push(format!("{rest}r"));
    }
    if let Some(rest) = name.strip_suffix('r') {
        guesses.push(format!("{rest}l"));
    }
    guesses.into_iter().find(|guess| {
        !guess.eq_ignore_ascii_case(name)
            && candidates
                .iter()
                .any(|c| c.eq_ignore_ascii_case(guess))
    })
}

fn list_field_hash(file: &ParamFile, list_name: &str) -> Option<u64> {
    let root = file.get_root()?;
    let ParamValue::Struct(root_struct) = &root.value else {
        return None;
    };
    root_struct.fields.iter().find_map(|(hash, _)| {
        (file.hash_labels.hash_to_string(*hash) == list_name).then_some(*hash)
    })
}

/// Create an empty list field on the flip.prc root when the category is missing.
fn ensure_flip_list(file: &mut ParamFile, list_name: &str) -> bool {
    if list_field_hash(file, list_name).is_some() {
        return true;
    }
    let Some(root) = file.root.as_mut() else {
        return false;
    };
    let ParamValue::Struct(root_struct) = &mut root.value else {
        return false;
    };
    let field_hash = file.hash_labels.add_label(list_name);
    root_struct.fields.insert(
        field_hash,
        ParamValue::List(ParamList {
            values: Vec::new(),
        }),
    );
    true
}

fn list_last_value(file: &ParamFile, list_name: &str) -> Option<ParamValue> {
    let hash = list_field_hash(file, list_name)?;
    let root = file.get_root()?;
    let ParamValue::Struct(root_struct) = &root.value else {
        return None;
    };
    let ParamValue::List(list) = root_struct.fields.get(&hash)? else {
        return None;
    };
    list.values.last().cloned()
}

fn entry_template(file: &ParamFile, list_name: &str) -> Option<ParamValue> {
    if let Some(value) = list_last_value(file, list_name) {
        return Some(value);
    }
    let kind = FlipListKind::for_category(list_name);
    let flip = FlipPrc::from_param_file(file)?;
    for name in flip.category_names() {
        if name == list_name {
            continue;
        }
        if FlipListKind::for_category(&name) != kind {
            continue;
        }
        if let Some(value) = list_last_value(file, &name) {
            return Some(value);
        }
    }
    None
}

fn with_list_mut<R>(
    file: &mut ParamFile,
    list_name: &str,
    f: impl FnOnce(&mut ParamList, &mut HashLabels) -> R,
) -> Option<R> {
    let hash = list_field_hash(file, list_name)?;
    let root = file.root.as_mut()?;
    let ParamValue::Struct(root_struct) = &mut root.value else {
        return None;
    };
    let value = root_struct.fields.get_mut(&hash)?;
    let ParamValue::List(list) = value else {
        return None;
    };
    Some(f(list, &mut file.hash_labels))
}

fn apply_entry_to_value(value: &mut ParamValue, entry: &FlipEntry, labels: &mut HashLabels) {
    match value {
        ParamValue::Struct(s) => {
            let keys: Vec<u64> = s.fields.keys().copied().collect();
            for hash in keys {
                let key = labels.hash_to_string(hash);
                match key.as_str() {
                    "lhs_name" | "left" | "name" | "mesh_name" | "bonename" | "start_bonename" => {
                        s.fields.insert(
                            hash,
                            ParamValue::Hash(labels.add_label(&entry.lhs_name)),
                        );
                    }
                    "rhs_name" | "right" | "flip_name" | "end_bonename" | "pair_name" => {
                        if let Some(rhs) = &entry.rhs_name {
                            s.fields
                                .insert(hash, ParamValue::Hash(labels.add_label(rhs)));
                        }
                    }
                    "trans" => {
                        s.fields.insert(
                            hash,
                            ParamValue::Hash(labels.add_label(&entry.trans.to_label())),
                        );
                    }
                    "rot" => {
                        s.fields.insert(
                            hash,
                            ParamValue::Hash(labels.add_label(&entry.rot.to_label())),
                        );
                    }
                    "scale" => {
                        s.fields.insert(hash, ParamValue::Bool(entry.scale));
                    }
                    _ => {}
                }
            }
        }
        ParamValue::Hash(h) => {
            *h = labels.add_label(&entry.lhs_name);
        }
        ParamValue::String(s) => {
            *s = entry.lhs_name.clone();
        }
        ParamValue::List(list) => {
            if !list.values.is_empty() {
                list.values[0] = ParamValue::Hash(labels.add_label(&entry.lhs_name));
            }
            if list.values.len() >= 2 {
                if let Some(rhs) = &entry.rhs_name {
                    list.values[1] = ParamValue::Hash(labels.add_label(rhs));
                }
            }
        }
        _ => {}
    }
}

fn new_entry_value(
    list_name: &str,
    entry: &FlipEntry,
    template: Option<&ParamValue>,
    labels: &mut HashLabels,
) -> ParamValue {
    if let Some(template) = template {
        let mut value = template.clone();
        apply_entry_to_value(&mut value, entry, labels);
        return value;
    }
    let kind = FlipListKind::for_category(list_name);
    match kind {
        FlipListKind::BoneSingles => ParamValue::Hash(labels.add_label(&entry.lhs_name)),
        FlipListKind::MeshPairs | FlipListKind::MaterialPairs => {
            if let Some(rhs) = &entry.rhs_name {
                ParamValue::List(ParamList {
                    values: vec![
                        ParamValue::Hash(labels.add_label(&entry.lhs_name)),
                        ParamValue::Hash(labels.add_label(rhs)),
                    ],
                })
            } else {
                ParamValue::Hash(labels.add_label(&entry.lhs_name))
            }
        }
        FlipListKind::BonePairs => {
            let mut fields = indexmap::IndexMap::new();
            fields.insert(
                labels.add_label("trans"),
                ParamValue::Hash(labels.add_label(&entry.trans.to_label())),
            );
            fields.insert(
                labels.add_label("rot"),
                ParamValue::Hash(labels.add_label(&entry.rot.to_label())),
            );
            fields.insert(
                labels.add_label("lhs_name"),
                ParamValue::Hash(labels.add_label(&entry.lhs_name)),
            );
            if let Some(rhs) = &entry.rhs_name {
                fields.insert(
                    labels.add_label("rhs_name"),
                    ParamValue::Hash(labels.add_label(rhs)),
                );
            }
            fields.insert(labels.add_label("scale"), ParamValue::Bool(entry.scale));
            ParamValue::Struct(ParamStruct {
                type_hash: 0,
                fields,
            })
        }
    }
}

pub fn update_flip_entry(file: &mut ParamFile, list_name: &str, index: usize, entry: &FlipEntry) -> bool {
    let ok = with_list_mut(file, list_name, |list, labels| {
        let Some(value) = list.values.get_mut(index) else {
            return false;
        };
        apply_entry_to_value(value, entry, labels);
        true
    })
    .unwrap_or(false);
    if ok {
        file.rebuild_tree_with_labels();
    }
    ok
}

pub fn append_flip_entry(file: &mut ParamFile, list_name: &str, entry: &FlipEntry) -> bool {
    if !ensure_flip_list(file, list_name) {
        return false;
    }
    let template = entry_template(file, list_name);
    let ok = with_list_mut(file, list_name, |list, labels| {
        let value = new_entry_value(list_name, entry, template.as_ref(), labels);
        list.values.push(value);
        true
    })
    .unwrap_or(false);
    if ok {
        file.rebuild_tree_with_labels();
    }
    ok
}

pub fn remove_flip_entry(file: &mut ParamFile, list_name: &str, index: usize) -> bool {
    let ok = with_list_mut(file, list_name, |list, _labels| {
        if index < list.values.len() {
            list.values.remove(index);
            true
        } else {
            false
        }
    })
    .unwrap_or(false);
    if ok {
        file.rebuild_tree_with_labels();
    }
    ok
}
