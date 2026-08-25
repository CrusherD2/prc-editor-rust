use crate::hash_labels::HashLabels;
use crate::param_types::ParamValue;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
const HYBRID_CHARSET: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
const MAX_BRUTE_LEN: usize = 4;

const FLIP_SEEDS: &[&str] = &[
    "flip_bones",
    "base_bones",
    "center_bones",
    "middle_bones",
    "core_bones",
    "axis_bones",
    "body_bones",
    "root_bones",
    "extra_bones",
    "trans_bones",
    "rot_bones",
    "scale_bones",
    "mirror_bones",
    "helper_bones",
    "swing_bones",
    "cloth_bones",
    "hair_bones",
    "skirt_bones",
    "cape_bones",
    "weapon_bones",
    "option_bones",
    "pair_meshes",
    "flip_meshes",
    "base_meshes",
    "single_meshes",
    "mesh_pairs",
    "meshes",
    "vis_meshes",
    "hide_meshes",
    "show_meshes",
    "extra_meshes",
    "pair_materials",
    "flip_materials",
    "base_materials",
    "mat_pairs",
    "materials",
    "lhs_name",
    "rhs_name",
    "pair_list",
    "flip_list",
    "bone_list",
    "mesh_list",
    "material_list",
    "center_line",
    "centerline",
    "middle_line",
    "flip_param",
    "flip_axis",
    "mirror_axis",
    "pair_axis",
    "pair_vis",
    "flip_vis",
    "kxy",
    "kyz",
    "kxz",
    "kxyz",
    "knone",
    "kx",
    "ky",
    "kz",
];

const AFFIXES: &[&str] = &[
    "pair_", "flip_", "base_", "single_", "center_", "middle_", "core_", "extra_", "body_",
    "root_", "axis_", "vis_", "mesh_", "mat_", "bone_", "left_", "right_", "hide_", "show_",
    "helper_", "swing_", "cloth_", "hair_", "weapon_", "option_",
];

const SUFFIXES: &[&str] = &[
    "1", "2", "3", "n", "l", "r", "c", "N", "L", "R", "C", "00", "01", "_l", "_r", "_n",
    "_vis", "_l1", "_r1", "l1", "r1",
];

const SWAPS: &[(&str, &str)] = &[
    ("bones", "meshes"),
    ("bones", "materials"),
    ("meshes", "bones"),
    ("meshes", "materials"),
    ("materials", "bones"),
    ("materials", "meshes"),
    ("bone", "mesh"),
    ("mesh", "bone"),
    ("mat", "mesh"),
    ("left", "right"),
    ("right", "left"),
    ("lhs", "rhs"),
    ("rhs", "lhs"),
];

const FOCUS_WORDS: &[&str] = &[
    "flip", "pair", "bone", "bones", "mesh", "meshes", "mat", "material", "materials",
    "axis", "trans", "rot", "scale", "left", "right", "vis", "hide", "show", "base",
    "single", "center", "middle", "body", "hair", "cloth", "weapon", "swing", "helper",
    "joint", "leg", "arm", "hand", "foot", "head", "hip", "waist", "bust", "neck",
    "mirror", "option", "extra", "root", "core", "line", "list", "name", "lhs", "rhs",
    "have", "item", "hold", "catch", "throw", "additional", "attach", "entry",
    "sword", "shield", "group", "table", "param",
];

/// Snake-case pieces used to build leftover-length names (14, 23, …).
const FLIP_VOCAB: &[&str] = &[
    "flip", "pair", "pairs", "bone", "bones", "mesh", "meshes", "mat", "material",
    "materials", "list", "name", "names", "lhs", "rhs", "left", "right", "base",
    "single", "center", "middle", "extra", "additional", "option", "weapon", "item",
    "have", "hold", "hand", "helper", "vis", "hide", "show", "trans", "rot", "scale",
    "joint", "attach", "entry", "entries", "param", "table", "group", "data",
    "mirror", "side", "body", "root", "core", "catch", "throw", "sword", "shield",
    "l", "r", "lr",
];

#[derive(Debug, Clone)]
pub struct CrackHit {
    pub hash: u64,
    pub label: String,
    pub source: &'static str,
}

/// Hash40 stores the string length in the high byte. Length 0 (e.g. `0x16`)
/// is a numeric/type value, not a reversible label.
pub fn is_string_hash40(hash: u64) -> bool {
    hash > 0xFF && HashLabels::hash40_length(hash) > 0
}

pub fn collect_hashes(value: &ParamValue, out: &mut HashSet<u64>) {
    match value {
        ParamValue::Hash(hash) => {
            if is_string_hash40(*hash) {
                out.insert(*hash);
            }
        }
        ParamValue::Struct(s) => {
            if is_string_hash40(s.type_hash) {
                out.insert(s.type_hash);
            }
            for (key, child) in &s.fields {
                if is_string_hash40(*key) {
                    out.insert(*key);
                }
                collect_hashes(child, out);
            }
        }
        ParamValue::List(list) => {
            for child in &list.values {
                collect_hashes(child, out);
            }
        }
        _ => {}
    }
}

pub fn unresolved_hashes(labels: &HashLabels, hashes: &HashSet<u64>) -> Vec<u64> {
    let mut unknown: Vec<u64> = hashes
        .iter()
        .copied()
        .filter(|hash| is_string_hash40(*hash) && labels.is_unresolved(*hash))
        .collect();
    unknown.sort_unstable();
    unknown.dedup();
    unknown
}

pub fn format_leftover(hashes: &[u64]) -> String {
    hashes
        .iter()
        .map(|hash| format!("0x{hash:X} (len {})", HashLabels::hash40_length(*hash)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Try one guessed string against unresolved hashes in the open file.
pub fn try_guess(targets: &[u64], guess: &str) -> Option<CrackHit> {
    let guess = guess.trim();
    if guess.is_empty() {
        return None;
    }
    let mut variants = vec![
        guess.to_string(),
        guess.to_ascii_lowercase(),
        guess.to_ascii_uppercase(),
        to_pascal(guess),
        to_camel(guess),
        guess.replace('_', ""),
    ];
    variants.extend(lr_variants(guess));
    variants.sort();
    variants.dedup();
    for variant in variants {
        let hash = HashLabels::hash40(&variant);
        if targets.iter().any(|target| *target == hash) {
            return Some(CrackHit {
                hash,
                label: variant,
                source: "guess",
            });
        }
    }
    None
}

pub fn crack_hashes(
    known_labels: &[String],
    targets: &[u64],
    extra_names: &[String],
) -> Vec<CrackHit> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut remaining: HashSet<u64> = targets
        .iter()
        .copied()
        .filter(|hash| is_string_hash40(*hash))
        .collect();
    let mut found: HashMap<u64, CrackHit> = HashMap::new();

    // Hash every loaded bone/mesh/material/anim name against leftovers first.
    match_names_against(&mut found, &mut remaining, extra_names, "model");
    if remaining.is_empty() {
        return finish(found);
    }

    let mut wanted_by_len = group_by_length(&remaining);
    if wanted_by_len.is_empty() {
        return finish(found);
    }

    let mut by_len: HashMap<usize, Vec<&str>> = HashMap::new();
    for name in known_labels.iter().chain(extra_names.iter()) {
        by_len.entry(name.len()).or_default().push(name.as_str());
    }

    for seed in FLIP_SEEDS {
        if record(&mut found, &mut wanted_by_len, seed, "flip seed") {
            return finish(found);
        }
        if record(&mut found, &mut wanted_by_len, &to_pascal(seed), "flip seed") {
            return finish(found);
        }
    }
    for name in extra_names {
        if record_name_variants(&mut found, &mut wanted_by_len, name, "model") {
            return finish(found);
        }
    }

    let lengths: Vec<usize> = wanted_by_len.keys().copied().collect();
    for total in lengths {
        if let Some(labels) = by_len.get(&total) {
            for label in labels {
                for (from, to) in SWAPS {
                    if label.contains(from) {
                        let swapped = label.replace(from, to);
                        if record(&mut found, &mut wanted_by_len, &swapped, "label swap") {
                            return finish(found);
                        }
                    }
                }
            }
        }
        for affix in AFFIXES {
            if total > affix.len() {
                if let Some(bases) = by_len.get(&(total - affix.len())) {
                    for base in bases {
                        if record(
                            &mut found,
                            &mut wanted_by_len,
                            &format!("{affix}{base}"),
                            "label affix",
                        ) {
                            return finish(found);
                        }
                    }
                }
            }
            let suffix = affix.trim_end_matches('_');
            if total > suffix.len() + 1 {
                if let Some(bases) = by_len.get(&(total - suffix.len() - 1)) {
                    for base in bases {
                        if record(
                            &mut found,
                            &mut wanted_by_len,
                            &format!("{base}_{suffix}"),
                            "label affix",
                        ) {
                            return finish(found);
                        }
                    }
                }
            }
        }
        for suffix in SUFFIXES {
            if total > suffix.len() {
                if let Some(bases) = by_len.get(&(total - suffix.len())) {
                    for base in bases {
                        if record(
                            &mut found,
                            &mut wanted_by_len,
                            &format!("{base}{suffix}"),
                            "label suffix",
                        ) {
                            return finish(found);
                        }
                    }
                }
            }
        }
    }

    let focus = focused_tokens(known_labels, extra_names);
    let focus_lengths: Vec<usize> = wanted_by_len.keys().copied().collect();
    recombine_tokens(&focus, &focus_lengths, |label| {
        record(&mut found, &mut wanted_by_len, &label, "focus tokens")
    });
    if wanted_by_len.is_empty() {
        return finish(found);
    }

    concat_pass(&mut found, &mut wanted_by_len, &focus);
    if wanted_by_len.is_empty() {
        return finish(found);
    }

    hybrid_pass(&mut found, &mut wanted_by_len, &by_len);
    if wanted_by_len.is_empty() {
        return finish(found);
    }

    leftover_word_combos(&mut found, &mut wanted_by_len);
    if wanted_by_len.is_empty() {
        return finish(found);
    }

    leftover_affix_brute(&mut found, &mut wanted_by_len, extra_names);
    if wanted_by_len.is_empty() {
        return finish(found);
    }

    let remaining: Vec<u64> = wanted_by_len.values().flatten().copied().collect();
    for (hash, label) in brute_remaining(&remaining) {
        found.entry(hash).or_insert(CrackHit {
            hash,
            label,
            source: "brute force",
        });
    }

    finish(found)
}

fn finish(found: HashMap<u64, CrackHit>) -> Vec<CrackHit> {
    let mut hits: Vec<CrackHit> = found.into_values().collect();
    hits.sort_by(|a, b| a.label.cmp(&b.label));
    hits
}

fn match_names_against(
    found: &mut HashMap<u64, CrackHit>,
    remaining: &mut HashSet<u64>,
    names: &[String],
    source: &'static str,
) {
    for name in names {
        for variant in name_variants(name) {
            let hash = HashLabels::hash40(&variant);
            if remaining.remove(&hash) {
                found.entry(hash).or_insert(CrackHit {
                    hash,
                    label: variant,
                    source,
                });
                if remaining.is_empty() {
                    return;
                }
            }
        }
    }
}

fn record(
    found: &mut HashMap<u64, CrackHit>,
    wanted_by_len: &mut HashMap<usize, HashSet<u64>>,
    label: &str,
    source: &'static str,
) -> bool {
    if label.is_empty() {
        return wanted_by_len.is_empty();
    }
    let Some(needed) = wanted_by_len.get_mut(&label.len()) else {
        return wanted_by_len.is_empty();
    };
    let hash = HashLabels::hash40(label);
    if needed.contains(&hash) {
        needed.remove(&hash);
        found.entry(hash).or_insert_with(|| CrackHit {
            hash,
            label: label.to_string(),
            source,
        });
        if needed.is_empty() {
            wanted_by_len.remove(&label.len());
        }
    }
    wanted_by_len.is_empty()
}

fn record_name_variants(
    found: &mut HashMap<u64, CrackHit>,
    wanted_by_len: &mut HashMap<usize, HashSet<u64>>,
    name: &str,
    source: &'static str,
) -> bool {
    for variant in name_variants(name) {
        if record(found, wanted_by_len, &variant, source) {
            return true;
        }
    }
    wanted_by_len.is_empty()
}

fn name_variants(name: &str) -> Vec<String> {
    let name = name.trim();
    if name.is_empty() {
        return Vec::new();
    }
    let mut out = vec![
        name.to_string(),
        name.to_ascii_lowercase(),
        name.to_ascii_uppercase(),
        to_pascal(name),
        to_camel(name),
        name.replace('_', ""),
    ];
    if let Some(base) = vis_base(name) {
        out.push(base.clone());
        out.push(base.to_ascii_lowercase());
        out.push(to_pascal(&base));
    }
    out.extend(lr_variants(name));
    out.sort();
    out.dedup();
    out
}

fn hybrid_pass(
    found: &mut HashMap<u64, CrackHit>,
    wanted_by_len: &mut HashMap<usize, HashSet<u64>>,
    by_len: &HashMap<usize, Vec<&str>>,
) {
    let targets: Vec<usize> = wanted_by_len.keys().copied().collect();
    for total in targets {
        if !wanted_by_len.contains_key(&total) {
            continue;
        }
        if let Some(bases) = by_len.get(&(total.saturating_sub(1))) {
            for base in bases {
                for &ch in HYBRID_CHARSET {
                    let c = ch as char;
                    if record(found, wanted_by_len, &format!("{base}{c}"), "hybrid")
                        || record(found, wanted_by_len, &format!("{c}{base}"), "hybrid")
                    {
                        return;
                    }
                }
            }
        }
        if total >= 2 && wanted_by_len.contains_key(&total) {
            if let Some(bases) = by_len.get(&(total - 2)) {
                for base in bases {
                    for &a in CHARSET {
                        for &b in CHARSET {
                            if record(
                                found,
                                wanted_by_len,
                                &format!("{base}{}{}", a as char, b as char),
                                "hybrid",
                            ) || record(
                                found,
                                wanted_by_len,
                                &format!("{}{}{base}", a as char, b as char),
                                "hybrid",
                            ) {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn concat_pass(
    found: &mut HashMap<u64, CrackHit>,
    wanted_by_len: &mut HashMap<usize, HashSet<u64>>,
    tokens: &[String],
) {
    let mut token_len: HashMap<usize, Vec<&str>> = HashMap::new();
    for token in tokens {
        token_len.entry(token.len()).or_default().push(token.as_str());
    }
    let targets: Vec<usize> = wanted_by_len.keys().copied().collect();
    for total in targets {
        for left_len in 2..total {
            if !wanted_by_len.contains_key(&total) {
                break;
            }
            let right_len = total - left_len;
            let Some(lefts) = token_len.get(&left_len) else {
                continue;
            };
            let Some(rights) = token_len.get(&right_len) else {
                continue;
            };
            if lefts.len().saturating_mul(rights.len()) > 250_000 {
                continue;
            }
            for left in lefts {
                for right in rights {
                    if record(
                        found,
                        wanted_by_len,
                        &format!("{left}{right}"),
                        "concat",
                    ) {
                        return;
                    }
                }
            }
        }
    }
}

fn group_by_length(hashes: &HashSet<u64>) -> HashMap<usize, HashSet<u64>> {
    let mut map: HashMap<usize, HashSet<u64>> = HashMap::new();
    for hash in hashes {
        map.entry(HashLabels::hash40_length(*hash))
            .or_default()
            .insert(*hash);
    }
    map
}

fn vis_base(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let idx = lower.find("_vis")?;
    let base = &name[..idx];
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

fn lr_variants(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let replacements = [
        ("left", "right"),
        ("right", "left"),
        ("Left", "Right"),
        ("Right", "Left"),
        ("_l", "_r"),
        ("_r", "_l"),
        ("_L", "_R"),
        ("_R", "_L"),
    ];
    for (from, to) in replacements {
        if name.contains(from) {
            out.push(name.replace(from, to));
        }
    }
    if let Some(last) = name.chars().last() {
        let swapped = match last {
            'l' => Some('r'),
            'r' => Some('l'),
            'L' => Some('R'),
            'R' => Some('L'),
            _ => None,
        };
        if let Some(ch) = swapped {
            let mut s = name.to_string();
            s.pop();
            s.push(ch);
            out.push(s);
        }
    }
    out
}

fn to_pascal(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-' || c == '.')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn to_camel(s: &str) -> String {
    let pascal = to_pascal(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn focused_tokens(labels: &[String], extra: &[String]) -> Vec<String> {
    let mut tokens: HashSet<String> = HashSet::new();
    for seed in FLIP_SEEDS {
        for part in seed.split('_') {
            tokens.insert(part.to_string());
        }
    }
    for word in FOCUS_WORDS {
        tokens.insert((*word).to_string());
    }
    for label in labels.iter().chain(extra.iter()) {
        let lower = label.to_ascii_lowercase();
        if FOCUS_WORDS.iter().any(|word| lower.contains(word)) {
            for part in label.split(|c: char| !c.is_ascii_alphanumeric()) {
                if (2..=16).contains(&part.len()) {
                    tokens.insert(part.to_ascii_lowercase());
                }
            }
        }
    }
    tokens.into_iter().collect()
}

fn recombine_tokens(
    tokens: &[String],
    lengths: &[usize],
    mut consider: impl FnMut(String) -> bool,
) {
    let mut by_len: HashMap<usize, Vec<&str>> = HashMap::new();
    for token in tokens {
        by_len.entry(token.len()).or_default().push(token.as_str());
    }
    for &total in lengths {
        if total >= 3 {
            for left_len in 1..total.saturating_sub(1) {
                let right_len = total - 1 - left_len;
                let Some(lefts) = by_len.get(&left_len) else {
                    continue;
                };
                let Some(rights) = by_len.get(&right_len) else {
                    continue;
                };
                if lefts.len().saturating_mul(rights.len()) > 250_000 {
                    continue;
                }
                for left in lefts {
                    for right in rights {
                        if consider(format!("{left}_{right}")) {
                            return;
                        }
                    }
                }
            }
        }
        if (8..=20).contains(&total) {
            for a_len in 2..=8 {
                for b_len in 2..=8 {
                    let rest = total.saturating_sub(a_len + b_len + 2);
                    if !(2..=10).contains(&rest) {
                        continue;
                    }
                    let Some(as_) = by_len.get(&a_len) else {
                        continue;
                    };
                    let Some(bs) = by_len.get(&b_len) else {
                        continue;
                    };
                    let Some(cs) = by_len.get(&rest) else {
                        continue;
                    };
                    if as_.len().saturating_mul(bs.len()).saturating_mul(cs.len()) > 250_000 {
                        continue;
                    }
                    for a in as_ {
                        for b in bs {
                            for c in cs {
                                if consider(format!("{a}_{b}_{c}")) {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn leftover_word_combos(
    found: &mut HashMap<u64, CrackHit>,
    wanted_by_len: &mut HashMap<usize, HashSet<u64>>,
) {
    if wanted_by_len.is_empty() {
        return;
    }
    let lengths: HashSet<usize> = wanted_by_len.keys().copied().collect();
    for a in FLIP_VOCAB {
        for b in FLIP_VOCAB {
            let two = format!("{a}_{b}");
            if lengths.contains(&two.len())
                && record(found, wanted_by_len, &two, "flip vocab")
            {
                return;
            }
            let concat = format!("{a}{b}");
            if lengths.contains(&concat.len())
                && record(found, wanted_by_len, &concat, "flip vocab")
            {
                return;
            }
            let pascal = format!("{}{}", to_pascal(a), to_pascal(b));
            if lengths.contains(&pascal.len())
                && record(found, wanted_by_len, &pascal, "flip vocab")
            {
                return;
            }
            for c in FLIP_VOCAB {
                let three_len = a.len() + b.len() + c.len() + 2;
                if lengths.contains(&three_len) {
                    let three = format!("{a}_{b}_{c}");
                    if record(found, wanted_by_len, &three, "flip vocab") {
                        return;
                    }
                }
                for d in FLIP_VOCAB {
                    let four_len = a.len() + b.len() + c.len() + d.len() + 3;
                    if lengths.contains(&four_len) {
                        let four = format!("{a}_{b}_{c}_{d}");
                        if record(found, wanted_by_len, &four, "flip vocab") {
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn leftover_affix_brute(
    found: &mut HashMap<u64, CrackHit>,
    wanted_by_len: &mut HashMap<usize, HashSet<u64>>,
    extra_names: &[String],
) {
    if wanted_by_len.is_empty() {
        return;
    }
    let mut crc_by_len: HashMap<usize, HashMap<u32, u64>> = HashMap::new();
    for hash in wanted_by_len.values().flatten() {
        let len = HashLabels::hash40_length(*hash);
        crc_by_len.entry(len).or_default().insert(*hash as u32, *hash);
    }

    let mut bases: Vec<String> = extra_names.iter().cloned().collect();
    for seed in FLIP_SEEDS {
        bases.push((*seed).to_string());
    }
    for word in FLIP_VOCAB {
        bases.push((*word).to_string());
    }
    bases.sort();
    bases.dedup();

    let hits: Vec<(u64, String)> = bases
        .par_iter()
        .flat_map(|base| {
            let mut local = Vec::new();
            for (&total, crc_map) in &crc_by_len {
                if total <= base.len() {
                    continue;
                }
                let pad = total - base.len();
                if pad == 0 || pad > 3 {
                    continue;
                }
                brute_pad_onto(base, pad, true, crc_map, &mut local);
            }
            local
        })
        .collect();

        for (_hash, label) in hits {
            if record(found, wanted_by_len, &label, "affix brute") {
                return;
            }
        }
    }

fn brute_pad_onto(
    base: &str,
    pad: usize,
    also_prefix: bool,
    crc_map: &HashMap<u32, u64>,
    out: &mut Vec<(u64, String)>,
) {
    let mut buf = vec![CHARSET[0]; pad];
    loop {
        let Ok(pad_str) = std::str::from_utf8(&buf) else {
            if !increment_all(&mut buf) {
                break;
            }
            continue;
        };
        let suffix = format!("{base}{pad_str}");
        push_if_hit_str(&suffix, crc_map, out);
        if also_prefix {
            let prefix = format!("{pad_str}{base}");
            push_if_hit_str(&prefix, crc_map, out);
        }
        if !increment_all(&mut buf) {
            break;
        }
    }
}

fn push_if_hit_str(label: &str, crc_map: &HashMap<u32, u64>, out: &mut Vec<(u64, String)>) {
    let crc = crate::hash_labels::crc32(label);
    if let Some(&hash) = crc_map.get(&crc) {
        if HashLabels::hash40(label) == hash {
            out.push((hash, label.to_string()));
        }
    }
}

fn increment_all(buf: &mut [u8]) -> bool {
    let n = CHARSET.len();
    for i in (0..buf.len()).rev() {
        let pos = CHARSET.iter().position(|&c| c == buf[i]).unwrap_or(0);
        if pos + 1 < n {
            buf[i] = CHARSET[pos + 1];
            return true;
        }
        buf[i] = CHARSET[0];
    }
    false
}

fn brute_remaining(targets: &[u64]) -> Vec<(u64, String)> {
    let mut by_len: HashMap<usize, HashMap<u32, u64>> = HashMap::new();
    for hash in targets {
        let len = HashLabels::hash40_length(*hash);
        if (1..=MAX_BRUTE_LEN).contains(&len) {
            by_len.entry(len).or_default().insert(*hash as u32, *hash);
        }
    }
    let mut hits = Vec::new();
    for (len, crc_map) in by_len {
        hits.extend(brute_length(len, &crc_map));
    }
    hits
}

fn brute_length(len: usize, crc_map: &HashMap<u32, u64>) -> Vec<(u64, String)> {
    if len == 0 || crc_map.is_empty() {
        return Vec::new();
    }
    CHARSET
        .par_iter()
        .flat_map(|&first| {
            let mut local = Vec::new();
            let mut buf = vec![0u8; len];
            buf[0] = first;
            if len == 1 {
                push_if_hit(&buf, crc_map, &mut local);
                return local;
            }
            for i in 1..len {
                buf[i] = CHARSET[0];
            }
            loop {
                push_if_hit(&buf, crc_map, &mut local);
                if !increment_tail(&mut buf) {
                    break;
                }
            }
            local
        })
        .collect()
}

fn push_if_hit(buf: &[u8], crc_map: &HashMap<u32, u64>, out: &mut Vec<(u64, String)>) {
    let Ok(label) = std::str::from_utf8(buf) else {
        return;
    };
    let crc = crate::hash_labels::crc32(label);
    if let Some(&hash) = crc_map.get(&crc) {
        if HashLabels::hash40(label) == hash {
            out.push((hash, label.to_string()));
        }
    }
}

fn increment_tail(buf: &mut [u8]) -> bool {
    let n = CHARSET.len();
    for i in (1..buf.len()).rev() {
        let pos = CHARSET.iter().position(|&c| c == buf[i]).unwrap_or(0);
        if pos + 1 < n {
            buf[i] = CHARSET[pos + 1];
            return true;
        }
        buf[i] = CHARSET[0];
    }
    false
}
