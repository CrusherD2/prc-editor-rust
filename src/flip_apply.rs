use crate::flip::{AxisMask, FlipEntry, FlipListKind, FlipPrc};
use glam::{Mat4, Quat, Vec3};
use ssbh_data::prelude::*;
use ssbh_wgpu::animation::AnimatedBone;
use ssbh_wgpu::{ModelFolder, RenderMesh, RenderModel, SharedRenderData};

// Same heuristic as SSBH Editor, plus visemes that wait/eyelid normally hide.
const EXPRESSION_PATTERNS: &[&str] = &[
    "_bink", "_low", "appeal", "attack", "blink", "bodybig", "bound", "breath", "brow2",
    "brow2flip", "brow3", "brow4", "brow5", "brow5flip", "camerahit", "capture", "catch",
    "cliff", "damage", "down", "escape", "eye2", "eye3", "eye4", "fall", "facencenter",
    "facencenterflip", "facenflip", "final", "flip", "fura", "half", "harf", "heavy", "hot",
    "largemouth", "laugh", "mouthb", "open_mouth", "ottotto", "ouch", "pattern", "pattrn_eye",
    "result", "smalleye", "sorori", "steppose", "swell", "talk", "thirdeye_v", "throw",
    "voice", "viseme",
];

const DEFAULT_EXPRESSION_PATTERNS: &[&str] = &[
    "belly_low", "brow1", "eye1", "facen_", "hanamayu", "openblink", "thirdeye_non",
];

pub fn hide_expression_meshes(render_model: &mut RenderModel) {
    // Same one-shot heuristic as SSBH Editor's Hide Expressions. Vis tracks from
    // loaded anims are applied separately and can hide/unhide matching meshes.
    for mesh in &mut render_model.meshes {
        if is_expression_mesh(&mesh.name) {
            mesh.is_visible = false;
        }
    }
}

pub fn show_all_meshes(render_model: &mut RenderModel) {
    for mesh in &mut render_model.meshes {
        mesh.is_visible = true;
    }
}

pub fn apply_mesh_list_visibility(
    render_model: &mut RenderModel,
    entries: &[(String, u64, bool)],
) {
    for mesh in &mut render_model.meshes {
        if let Some((_, _, visible)) = entries
            .iter()
            .find(|(name, sub, _)| name == &mesh.name && *sub == mesh.subindex)
        {
            mesh.is_visible = *visible;
        }
    }
}

pub fn is_expression_mesh(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    !DEFAULT_EXPRESSION_PATTERNS.iter().any(|p| name.contains(p))
        && EXPRESSION_PATTERNS.iter().any(|p| name.contains(p))
}

pub fn snapshot_mesh_visibility(render_model: &RenderModel) -> Vec<(String, u64, bool)> {
    render_model
        .meshes
        .iter()
        .map(|m| (m.name.clone(), m.subindex, m.is_visible))
        .collect()
}

/// Right: yaw 90° so file +Z faces Smash +X (screen right).
/// Left: Smash turns every fighter 180° from that (back to camera), then
/// flip.prc remaps bones/meshes so stance-mirrored characters show the front.
pub fn apply_pose_and_flip(
    queue: &wgpu::Queue,
    render_model: &mut RenderModel,
    model: &ModelFolder,
    shared_data: &SharedRenderData,
    flip: &FlipPrc,
    facing_left: bool,
    always_face_camera: bool,
    anims: &[&AnimData],
    current_frame: f32,
    original_modl: Option<&ModlData>,
) {
    // Files face +Z. Smash yaws 90° onto stage X. Facing left is another 180°
    // on every fighter; flip.prc then mirrors listed bone axes / trans deltas.
    // The camera-facing preview option adds a separate Y rotation after that
    // gameplay transform, leaving the flip logic itself unchanged.
    let orient = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let mut model_transform = if facing_left {
        Mat4::from_rotation_y(std::f32::consts::PI) * orient
    } else {
        orient
    };
    if facing_left && always_face_camera {
        model_transform = Mat4::from_rotation_y(std::f32::consts::PI) * model_transform;
    }
    render_model.set_model_transform(queue, model_transform);
    render_model.invert_winding = false;

    if let Some(modl) = original_modl {
        let mut working = modl.clone();
        if facing_left {
            swap_modl_materials(&mut working, &flip.pair_materials);
            for unknown in &flip.unknown {
                if FlipListKind::for_category(&unknown.name) == FlipListKind::MaterialPairs {
                    swap_modl_materials(&mut working, &unknown.entries);
                }
            }
        }
        render_model.reassign_materials(&working, model.find_matl());
    }

    let skel = model.find_skel();
    let matl = model.find_matl();
    let hlpb = model.find_hlpb();

    if facing_left {
        // Skip helper constraints on left: they would overwrite the engine
        // XYZ translation flip on flip.prc-listed bones (including HaveL/HaveR).
        render_model.apply_anims_with_skel_edit(
            queue,
            anims.iter().copied(),
            skel,
            matl,
            None,
            shared_data,
            current_frame,
            true,
            |bones| {
                apply_flip_anim_pairs(bones, flip);
                Vec::new()
            },
        );
    } else {
        render_model.apply_anims(
            queue,
            anims.iter().copied(),
            skel,
            matl,
            hlpb,
            shared_data,
            current_frame,
            true,
        );
    }

    remap_flip_mesh_pairs(&mut render_model.meshes, flip, facing_left);
}

pub fn remap_flip_mesh_pairs(meshes: &mut [RenderMesh], flip: &FlipPrc, facing_left: bool) {
    apply_mesh_pairs(meshes, &flip.meshes, facing_left);
    for unknown in &flip.unknown {
        if FlipListKind::for_category(&unknown.name) == FlipListKind::MeshPairs {
            apply_mesh_pairs(meshes, &unknown.entries, facing_left);
        }
    }
}

fn apply_flip_anim_pairs(bones: &mut [(usize, AnimatedBone)], flip: &FlipPrc) {
    // Facing left: Smash mirrors animation translation on X/Y/Z, but only for
    // bones that appear in flip.prc. Unlisted bones keep their normal anim.
    // PRC trans flags still cancel axes on listed entries afterward.
    apply_engine_trans_flip(bones, flip);
    for entry in flip.flip_bones.iter().chain(&flip.pair_bones) {
        remap_pair_anim(bones, entry);
    }
    for unknown in &flip.unknown {
        if FlipPrc::list_has_pairs(&unknown.entries) {
            for entry in &unknown.entries {
                remap_pair_anim(bones, entry);
            }
        }
    }
    for entry in flip.base_bones.iter().chain(&flip.single_bones) {
        remap_single_anim(bones, entry);
    }
    for unknown in &flip.unknown {
        if !FlipPrc::list_has_pairs(&unknown.entries) {
            for entry in &unknown.entries {
                remap_single_anim(bones, entry);
            }
        }
    }
}

fn remap_single_anim(bones: &mut [(usize, AnimatedBone)], entry: &FlipEntry) {
    let Some(i) = bone_pos(bones, &entry.lhs_name) else {
        return;
    };
    if entry.trans == AxisMask::default() && entry.rot == AxisMask::default() && !entry.scale {
        return;
    }
    let rest = bones[i].1.rest_trs();
    let cur = bones[i].1.current_trs();
    let (t, r, s) = apply_flags(
        rest,
        cur,
        rest,
        cur,
        entry.trans,
        entry.rot,
        entry.scale,
    );
    bones[i].1.set_current_trs(t, r, s);
}

fn apply_engine_trans_flip(bones: &mut [(usize, AnimatedBone)], flip: &FlipPrc) {
    let listed: std::collections::HashSet<String> = flip
        .affected_bone_names()
        .into_iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    if listed.is_empty() {
        return;
    }
    for i in 0..bones.len() {
        if !listed.contains(&bones[i].1.name().to_ascii_lowercase()) {
            continue;
        }
        let rest = bones[i].1.rest_trs();
        let cur = bones[i].1.current_trs();
        let d = cur.0 - rest.0;
        let t = rest.0 + Vec3::new(-d.x, -d.y, -d.z);
        bones[i].1.set_current_trs(t, cur.1, cur.2);
    }
}

fn bone_pos(bones: &[(usize, AnimatedBone)], name: &str) -> Option<usize> {
    bones
        .iter()
        .position(|(_, bone)| bone.name().eq_ignore_ascii_case(name))
}

fn remap_pair_anim(bones: &mut [(usize, AnimatedBone)], entry: &FlipEntry) {
    let Some(lhs) = bone_pos(bones, &entry.lhs_name) else {
        return;
    };
    let same = entry
        .rhs_name
        .as_deref()
        .map(|rhs| rhs.eq_ignore_ascii_case(&entry.lhs_name))
        .unwrap_or(true);
    if same {
        remap_single_anim(bones, entry);
        return;
    }
    let Some(rhs) = entry
        .rhs_name
        .as_deref()
        .and_then(|name| bone_pos(bones, name))
    else {
        return;
    };
    if lhs == rhs {
        return;
    }

    let rest_l = bones[lhs].1.rest_trs();
    let rest_r = bones[rhs].1.rest_trs();
    let cur_l = bones[lhs].1.current_trs();
    let cur_r = bones[rhs].1.current_trs();

    let (t_l, r_l, s_l) = apply_flags(
        rest_l, cur_l, rest_r, cur_r, entry.trans, entry.rot, entry.scale,
    );
    let (t_r, r_r, s_r) = apply_flags(
        rest_r, cur_r, rest_l, cur_l, entry.trans, entry.rot, entry.scale,
    );
    bones[lhs].1.set_current_trs(t_l, r_l, s_l);
    bones[rhs].1.set_current_trs(t_r, r_r, s_r);
}

/// Put `source`'s animation onto `self`'s rest pose.
/// `kyz` mirrors Y and Z of that motion (sign-flip those channels), same for `kxy` / `kxz`.
/// Translation is rest-relative so bind height stays.
fn apply_flags(
    self_rest: (Vec3, Quat, Vec3),
    self_cur: (Vec3, Quat, Vec3),
    source_rest: (Vec3, Quat, Vec3),
    source_cur: (Vec3, Quat, Vec3),
    trans: AxisMask,
    rot: AxisMask,
    scale: bool,
) -> (Vec3, Quat, Vec3) {
    let d_trans = source_cur.0 - source_rest.0;
    let d_rot = source_rest.1.inverse() * source_cur.1;
    let t = self_rest.0 + mask_vec3(d_trans, trans);
    let r = self_rest.1 * mask_quat(d_rot, rot);
    let s = if scale { source_cur.2 } else { self_cur.2 };
    (t, r, s)
}

fn mask_vec3(v: Vec3, mask: AxisMask) -> Vec3 {
    Vec3::new(
        if mask.x { -v.x } else { v.x },
        if mask.y { -v.y } else { v.y },
        if mask.z { -v.z } else { v.z },
    )
}

/// Mirror a rotation on the listed axes: negate those quaternion xyz channels
/// (`kyz` → `(x, -y, -z, w)`). This matches nuanmb XYZ, not Euler rebuilds.
fn mask_quat(q: Quat, mask: AxisMask) -> Quat {
    if !mask.x && !mask.y && !mask.z {
        return q;
    }
    Quat::from_xyzw(
        if mask.x { -q.x } else { q.x },
        if mask.y { -q.y } else { q.y },
        if mask.z { -q.z } else { q.z },
        q.w,
    )
    .normalize()
}

fn material_name_matches(label: &str, target: &str) -> bool {
    label.eq_ignore_ascii_case(target)
}

fn resolve_modl_material_label(modl: &ModlData, name: &str) -> Option<String> {
    modl.entries
        .iter()
        .find(|entry| material_name_matches(&entry.material_label, name))
        .map(|entry| entry.material_label.clone())
}

fn swap_modl_materials(modl: &mut ModlData, pairs: &[FlipEntry]) {
    for pair in pairs {
        let Some(rhs) = pair.rhs_name.as_deref() else {
            continue;
        };
        let Some(lhs_actual) = resolve_modl_material_label(modl, &pair.lhs_name) else {
            continue;
        };
        let Some(rhs_actual) = resolve_modl_material_label(modl, rhs) else {
            continue;
        };
        if lhs_actual == rhs_actual {
            continue;
        }
        for entry in &mut modl.entries {
            if entry.material_label == lhs_actual {
                entry.material_label = rhs_actual.clone();
            } else if entry.material_label == rhs_actual {
                entry.material_label = lhs_actual.clone();
            }
        }
    }
}

fn apply_mesh_pairs(meshes: &mut [RenderMesh], pairs: &[FlipEntry], facing_left: bool) {
    for pair in pairs {
        match pair.rhs_name.as_deref() {
            Some(rhs) if rhs != pair.lhs_name => {
                let lhs_visible = meshes.iter().any(|mesh| {
                    mesh_name_matches(&mesh.name, &pair.lhs_name) && mesh.is_visible
                });
                let rhs_visible = meshes
                    .iter()
                    .any(|mesh| mesh_name_matches(&mesh.name, rhs) && mesh.is_visible);
                if facing_left {
                    // pan visible → pan off, panFLIP on
                    if lhs_visible {
                        set_pair_visibility(meshes, &pair.lhs_name, rhs, false);
                    }
                } else if rhs_visible {
                    // panFLIP visible → pan on, panFLIP off
                    set_pair_visibility(meshes, &pair.lhs_name, rhs, true);
                }
            }
            _ => {
                if facing_left {
                    for mesh in meshes.iter_mut() {
                        if mesh_name_matches(&mesh.name, &pair.lhs_name) {
                            mesh.is_visible = false;
                        }
                    }
                }
            }
        }
    }
}

fn set_pair_visibility(
    meshes: &mut [RenderMesh],
    lhs: &str,
    rhs: &str,
    show_lhs: bool,
) {
    for mesh in meshes.iter_mut() {
        if mesh_name_matches(&mesh.name, lhs) {
            mesh.is_visible = show_lhs;
        } else if mesh_name_matches(&mesh.name, rhs) {
            mesh.is_visible = !show_lhs;
        }
    }
}

pub fn bone_highlight_colors(
    bone_names: &[String],
    hlpb_helper_names: &[String],
    flip_names: &[String],
    pair_names: &[String],
    base_names: &[String],
    single_names: &[String],
    selected_names: &[String],
    highlight_categories: bool,
) -> Vec<[f32; 4]> {
    let helper = [0.3, 0.0, 0.6, 1.0];
    let default = [0.65, 0.65, 0.65, 1.0];
    let flip = [1.0, 0.55, 0.08, 1.0];
    let pair = [0.95, 0.85, 0.15, 1.0];
    let base = [0.95, 0.35, 0.75, 1.0];
    let single = [0.35, 0.9, 0.4, 1.0];
    let selected = [0.15, 0.95, 1.0, 1.0];
    bone_names
        .iter()
        .map(|name| {
            if selected_names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                selected
            } else if highlight_categories && flip_names.iter().any(|n| n.eq_ignore_ascii_case(name))
            {
                flip
            } else if highlight_categories && pair_names.iter().any(|n| n.eq_ignore_ascii_case(name))
            {
                pair
            } else if highlight_categories && base_names.iter().any(|n| n.eq_ignore_ascii_case(name))
            {
                base
            } else if highlight_categories
                && single_names.iter().any(|n| n.eq_ignore_ascii_case(name))
            {
                single
            } else if hlpb_helper_names
                .iter()
                .any(|n| n.eq_ignore_ascii_case(name))
            {
                helper
            } else {
                default
            }
        })
        .collect()
}

pub fn helper_bone_names(hlpb: Option<&HlpbData>) -> Vec<String> {
    let Some(hlpb) = hlpb else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for constraint in &hlpb.aim_constraints {
        names.push(constraint.target_bone_name2.clone());
    }
    for constraint in &hlpb.orient_constraints {
        names.push(constraint.target_bone_name.clone());
    }
    names
}

fn mesh_name_matches(mesh_name: &str, target: &str) -> bool {
    // panm matches PanM_VIS_O_OBJShape; panmflip matches PanMFLIP_VIS_O_OBJShape.
    crate::flip::mesh_flip_key(mesh_name) == crate::flip::mesh_flip_key(target)
}
