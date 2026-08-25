use crate::flip::{mesh_flip_key, FlipListKind, FlipPrc};
use crate::flip_apply::{
    apply_mesh_list_visibility, apply_pose_and_flip, bone_highlight_colors, helper_bone_names,
    hide_expression_meshes, remap_flip_mesh_pairs, show_all_meshes, snapshot_mesh_visibility,
};
use egui_wgpu::{CallbackResources, CallbackTrait, ScreenDescriptor};
use ssbh_data::prelude::*;
use ssbh_wgpu::{
    CameraTransforms, ModelFolder, ModelRenderOptions, RenderModel, SharedRenderData, SsbhRenderer,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainTab {
    #[default]
    Editor,
    FlipPreview,
}

pub struct CameraState {
    pub translation: glam::Vec3,
    pub rotation_radians: glam::Vec3,
    pub fov_y_radians: f32,
    pub near_clip: f32,
    pub far_clip: f32,
    pub is_mouse_primary_drag: bool,
    pub is_mouse_secondary_drag: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewportView {
    #[default]
    Smash,
    Front,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPanelTab {
    #[default]
    Meshes,
    Anims,
}

impl CameraState {
    /// Look at the face: model is yawed to stage X, camera looks along that axis.
    pub fn front_view() -> Self {
        Self {
            translation: glam::Vec3::new(0.0, -8.0, -60.0),
            rotation_radians: glam::Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0),
            fov_y_radians: 30f32.to_radians(),
            near_clip: 1.0,
            far_clip: 400000.0,
            is_mouse_primary_drag: false,
            is_mouse_secondary_drag: false,
        }
    }

    /// Smash side view: look along -Z at a model facing +X (screen right).
    pub fn smash_side_view() -> Self {
        Self {
            translation: glam::Vec3::new(0.0, -8.0, -60.0),
            rotation_radians: glam::Vec3::ZERO,
            fov_y_radians: 30f32.to_radians(),
            near_clip: 1.0,
            far_clip: 400000.0,
            is_mouse_primary_drag: false,
            is_mouse_secondary_drag: false,
        }
    }
}

impl Default for CameraState {
    fn default() -> Self {
        Self::smash_side_view()
    }
}

pub struct AnimFolder {
    pub path: PathBuf,
    pub anims: Vec<(String, AnimData)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AnimIndex {
    pub folder_index: usize,
    pub anim_index: usize,
}

pub struct AnimSlot {
    pub is_enabled: bool,
    pub animation: Option<AnimIndex>,
}

impl AnimSlot {
    pub fn new() -> Self {
        Self {
            is_enabled: true,
            animation: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ViewportHover {
    Mesh { name: String, subindex: u64 },
    Bone { name: String },
}

impl ViewportHover {
    pub fn pick_name(&self) -> &str {
        match self {
            Self::Mesh { name, .. } | Self::Bone { name } => name,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Mesh { name, subindex } if *subindex > 0 => format!("mesh  {name} [{subindex}]"),
            Self::Mesh { name, .. } => format!("mesh  {name}"),
            Self::Bone { name } => format!("bone  {name}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewportPickMode {
    Meshes,
    Bones,
    Inspect,
}

#[derive(Debug, Clone)]
struct PickSample {
    pos: glam::Vec3,
    bones: [(i16, f32); 4],
}

#[derive(Debug, Clone)]
pub struct MeshPickBounds {
    pub name: String,
    pub subindex: u64,
    samples: Vec<PickSample>,
    parent_bone: i32,
}

#[derive(Debug, Clone, Default)]
pub enum AddEntryWizard {
    #[default]
    Inactive,
    PickCategory {
        category: String,
    },
    PickLhs {
        category: String,
        filter: String,
    },
    PickRhs {
        category: String,
        lhs: String,
        filter: String,
    },
}

impl AddEntryWizard {
    pub fn is_picking(&self) -> bool {
        matches!(self, Self::PickLhs { .. } | Self::PickRhs { .. })
    }

    pub fn pick_kind(&self) -> Option<FlipListKind> {
        match self {
            Self::PickLhs { category, .. } | Self::PickRhs { category, .. } => {
                Some(FlipListKind::for_category(category))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeshVisCommand {
    #[default]
    None,
    ApplyList,
    ShowAll,
    HideExpressions,
}

pub struct FlipPreviewState {
    pub tab: MainTab,
    pub facing_left: bool,
    pub show_bones: bool,
    pub model_path: Option<PathBuf>,
    pub model: Option<ModelFolder>,
    pub anim_folders: Vec<AnimFolder>,
    pub anim_slots: Vec<AnimSlot>,
    pub current_frame: f32,
    pub is_playing: bool,
    pub should_loop: bool,
    pub whole_frames: bool,
    pub whole_frame_remainder: f32,
    pub playback_speed: f32,
    pub last_frame_time: Option<Instant>,
    pub camera: CameraState,
    pub view_mode: ViewportView,
    pub right_panel: RightPanelTab,
    pub hovered_entry: Option<(String, usize)>,
    pub selected_entry: Option<(String, usize)>,
    pub add_wizard: AddEntryWizard,
    pub original_mesh_visibility: Vec<(String, u64, bool)>,
    pub mesh_entries: Vec<(String, u64, bool)>,
    pub mesh_vis_command: MeshVisCommand,
    pub original_modl: Option<ModlData>,
    pub mesh_bounds: Vec<MeshPickBounds>,
    pub bind_world: HashMap<String, glam::Mat4>,
    pub hovered_viewport: Option<ViewportHover>,
    pub pending_viewport_pick: Option<String>,
    pub inspected_mesh: Option<(String, u64)>,
    pub inspected_bone: Option<String>,
    pub scroll_to_inspected_mesh: bool,
    pub scroll_to_selected_entry: bool,
    pub shift_inspect_held: bool,
    pub inspect_clicked_this_hold: bool,
    pub needs_gpu_reload: bool,
    pub apply_pending: bool,
    pub param_dirty: bool,
    pub load_error: Option<String>,
    pub request_crack: bool,
}

impl Default for FlipPreviewState {
    fn default() -> Self {
        Self {
            tab: MainTab::Editor,
            facing_left: false,
            show_bones: false,
            model_path: None,
            model: None,
            anim_folders: Vec::new(),
            anim_slots: vec![AnimSlot::new()],
            current_frame: 0.0,
            is_playing: false,
            should_loop: true,
            whole_frames: false,
            whole_frame_remainder: 0.0,
            playback_speed: 1.0,
            last_frame_time: None,
            camera: CameraState::default(),
            view_mode: ViewportView::Smash,
            right_panel: RightPanelTab::Meshes,
            hovered_entry: None,
            selected_entry: None,
            add_wizard: AddEntryWizard::Inactive,
            original_mesh_visibility: Vec::new(),
            mesh_entries: Vec::new(),
            mesh_vis_command: MeshVisCommand::None,
            original_modl: None,
            mesh_bounds: Vec::new(),
            bind_world: HashMap::new(),
            hovered_viewport: None,
            pending_viewport_pick: None,
            inspected_mesh: None,
            inspected_bone: None,
            scroll_to_inspected_mesh: false,
            scroll_to_selected_entry: false,
            shift_inspect_held: false,
            inspect_clicked_this_hold: false,
            needs_gpu_reload: false,
            apply_pending: true,
            param_dirty: false,
            load_error: None,
            request_crack: false,
        }
    }
}

impl FlipPreviewState {
    pub fn load_model_folder(&mut self, path: PathBuf) {
        let model = ModelFolder::load_folder(&path);
        if model.find_mesh().is_none() && model.find_skel().is_none() {
            self.load_error = Some(format!(
                "No Smash model files (numshb/nusktb) in {}",
                path.display()
            ));
            return;
        }
        self.original_modl = model.find_modl().cloned();
        self.mesh_bounds = mesh_pick_bounds(&model);
        self.bind_world = model
            .find_skel()
            .map(bind_world_map)
            .unwrap_or_default();
        self.model = Some(model);
        self.model_path = Some(path);
        self.mesh_vis_command = MeshVisCommand::None;
        self.mesh_entries.clear();
        self.needs_gpu_reload = true;
        self.apply_pending = true;
        self.load_error = None;
        self.current_frame = 0.0;
        self.inspected_mesh = None;
        self.inspected_bone = None;
    }

    pub fn bone_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let Some(model) = &self.model else {
            return names;
        };
        for (_, skel) in &model.skels {
            let Some(skel) = skel else {
                continue;
            };
            names.extend(skel.bones.iter().map(|bone| bone.name.clone()));
        }
        names
    }

    /// Bone, mesh, material, helper, texture, and anim names from the loaded
    /// Flip Preview model — used as a Hash40 dictionary for unresolved labels.
    pub fn dictionary_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut push = |value: &str| {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                names.push(trimmed.to_string());
            }
        };

        for name in self.bone_names() {
            push(&name);
        }
        for name in self.unique_mesh_names() {
            push(&name);
        }
        for name in self.material_names() {
            push(&name);
        }

        if let Some(model) = &self.model {
            for (_, mesh) in &model.meshes {
                let Some(mesh) = mesh else {
                    continue;
                };
                for object in &mesh.objects {
                    push(&object.name);
                    push(&object.parent_bone_name);
                    for influence in &object.bone_influences {
                        push(&influence.bone_name);
                    }
                }
            }
            for (_, matl) in &model.matls {
                let Some(matl) = matl else {
                    continue;
                };
                for entry in &matl.entries {
                    push(&entry.material_label);
                    push(&entry.shader_label);
                    for texture in &entry.textures {
                        push(&texture.data);
                    }
                }
            }
            for (_, modl) in &model.modls {
                let Some(modl) = modl else {
                    continue;
                };
                for entry in &modl.entries {
                    push(&entry.mesh_object_name);
                    push(&entry.material_label);
                }
            }
            if let Some(modl) = &self.original_modl {
                for entry in &modl.entries {
                    push(&entry.mesh_object_name);
                    push(&entry.material_label);
                }
            }
            for (_, hlpb) in &model.hlpbs {
                let Some(hlpb) = hlpb else {
                    continue;
                };
                for constraint in &hlpb.aim_constraints {
                    push(&constraint.name);
                    push(&constraint.aim_bone_name1);
                    push(&constraint.aim_bone_name2);
                    push(&constraint.target_bone_name1);
                    push(&constraint.target_bone_name2);
                }
                for constraint in &hlpb.orient_constraints {
                    push(&constraint.name);
                    push(&constraint.parent_bone_name1);
                    push(&constraint.parent_bone_name2);
                    push(&constraint.source_bone_name);
                    push(&constraint.target_bone_name);
                }
            }
            for (_, meshex) in &model.meshexes {
                let Some(meshex) = meshex else {
                    continue;
                };
                for group in &meshex.mesh_object_groups {
                    push(&group.mesh_object_name);
                    push(&group.mesh_object_full_name);
                }
            }
            for (filename, _) in &model.nutexbs {
                if let Some(stem) = std::path::Path::new(filename).file_stem() {
                    push(&stem.to_string_lossy());
                }
            }
            for (_, anim) in &model.anims {
                let Some(anim) = anim else {
                    continue;
                };
                for group in &anim.groups {
                    for node in &group.nodes {
                        push(&node.name);
                    }
                }
            }
        }

        for folder in &self.anim_folders {
            for (_, anim) in &folder.anims {
                for group in &anim.groups {
                    for node in &group.nodes {
                        push(&node.name);
                    }
                }
            }
        }

        names.sort();
        names.dedup();
        names
    }

    /// Visibility / bone track names from nearby `.nuanmb` files.
    pub fn anim_names_from_folders(folders: &[PathBuf]) -> Vec<String> {
        let mut names = Vec::new();
        let mut loaded = 0usize;
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            for entry in entries.flatten() {
                if loaded >= 48 {
                    break;
                }
                let path = entry.path();
                let is_anim = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("nuanmb"))
                    .unwrap_or(false);
                if !is_anim {
                    continue;
                }
                let Ok(anim) = AnimData::from_file(&path) else {
                    continue;
                };
                loaded += 1;
                for group in &anim.groups {
                    for node in &group.nodes {
                        names.push(node.name.clone());
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    pub fn unique_mesh_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for (name, _, _) in &self.mesh_entries {
            let key = crate::flip::mesh_flip_key(name);
            if key.is_empty() || names.iter().any(|n| n == &key) {
                continue;
            }
            names.push(key);
        }
        names.sort();
        names
    }

    pub fn material_names(&self) -> Vec<String> {
        self.model
            .as_ref()
            .and_then(|m| m.find_matl())
            .map(|matl| {
                matl.entries
                    .iter()
                    .map(|e| e.material_label.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn picker_names(&self, category: &str) -> Vec<String> {
        match crate::flip::FlipListKind::for_category(category) {
            crate::flip::FlipListKind::MeshPairs => self.unique_mesh_names(),
            crate::flip::FlipListKind::MaterialPairs => self.material_names(),
            _ => self.bone_names(),
        }
    }

    /// Name stored in flip.prc for a wizard/edit pick. Always lowercase.
    /// For pair_materials, a mesh pick becomes that mesh's material label.
    pub fn name_for_flip_add(&self, category: &str, picked: &str, subindex: Option<u64>) -> String {
        self.store_flip_name(FlipListKind::for_category(category), picked, subindex)
    }

    pub fn store_flip_name(
        &self,
        kind: FlipListKind,
        picked: &str,
        subindex: Option<u64>,
    ) -> String {
        let name = match kind {
            FlipListKind::MaterialPairs => self.material_name_for_pick(picked, subindex),
            FlipListKind::MeshPairs => mesh_flip_key(picked),
            _ => picked.to_string(),
        };
        crate::flip::flip_store_name(&name)
    }

    fn material_name_for_pick(&self, picked: &str, subindex: Option<u64>) -> String {
        let is_mesh = picked.to_ascii_lowercase().contains("_vis")
            || self
                .mesh_entries
                .iter()
                .any(|(n, _, _)| n.eq_ignore_ascii_case(picked));
        if is_mesh {
            let sub = subindex
                .or_else(|| {
                    self.mesh_entries
                        .iter()
                        .find(|(n, _, _)| n.eq_ignore_ascii_case(picked))
                        .map(|(_, s, _)| *s)
                })
                .unwrap_or(0);
            if let Some(mat) = self.material_for_mesh(picked, sub) {
                return mat;
            }
        }
        picked.to_string()
    }

    fn material_for_mesh(&self, mesh_name: &str, subindex: u64) -> Option<String> {
        self.original_modl.as_ref().and_then(|modl| {
            modl.entries.iter().find(|entry| {
                entry.mesh_object_name.eq_ignore_ascii_case(mesh_name)
                    && entry.mesh_object_subindex == subindex
            })
            .map(|entry| entry.material_label.clone())
        })
    }

    pub fn load_anim_folder(&mut self, path: PathBuf) {
        let folder = ModelFolder::load_folder(&path);
        if folder.anims.is_empty() {
            self.load_error = Some(format!("No .nuanmb files in {}", path.display()));
            return;
        }
        let mut anims: Vec<(String, AnimData)> = folder
            .anims
            .into_iter()
            .filter_map(|(name, anim)| anim.map(|anim| (name, anim)))
            .collect();
        anims.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
        if let Some(existing) = self
            .anim_folders
            .iter_mut()
            .find(|folder| folder.path == path)
        {
            existing.anims = anims;
        } else {
            self.anim_folders.push(AnimFolder { path, anims });
        }
        if self.anim_slots.is_empty() {
            self.anim_slots.push(AnimSlot::new());
        }
        self.current_frame = 0.0;
        self.is_playing = false;
        self.last_frame_time = None;
        self.apply_pending = true;
        self.load_error = None;
        self.right_panel = RightPanelTab::Anims;
    }

    pub fn anim_at(&self, index: AnimIndex) -> Option<&(String, AnimData)> {
        self.anim_folders
            .get(index.folder_index)
            .and_then(|folder| folder.anims.get(index.anim_index))
    }

    pub fn enabled_anims<'a>(
        slots: &'a [AnimSlot],
        folders: &'a [AnimFolder],
    ) -> Vec<&'a AnimData> {
        slots
            .iter()
            .filter(|slot| slot.is_enabled)
            .filter_map(|slot| {
                slot.animation.and_then(|index| {
                    folders
                        .get(index.folder_index)
                        .and_then(|folder| folder.anims.get(index.anim_index))
                })
            })
            .map(|(_, anim)| anim)
            .collect()
    }

    pub fn final_frame_index(&self) -> f32 {
        Self::enabled_anims(&self.anim_slots, &self.anim_folders)
            .iter()
            .map(|anim| anim.final_frame_index)
            .fold(0.0f32, f32::max)
    }

    pub fn has_enabled_anim(&self) -> bool {
        !Self::enabled_anims(&self.anim_slots, &self.anim_folders).is_empty()
    }

    pub fn all_anim_indices(&self) -> Vec<AnimIndex> {
        let mut indices = Vec::new();
        for (folder_index, folder) in self.anim_folders.iter().enumerate().rev() {
            for anim_index in 0..folder.anims.len() {
                indices.push(AnimIndex {
                    folder_index,
                    anim_index,
                });
            }
        }
        indices
    }

    pub fn tick_animation(&mut self) {
        if !self.is_playing || !self.has_enabled_anim() {
            self.last_frame_time = None;
            return;
        }
        let max_frame = self.final_frame_index().max(1.0);
        let now = Instant::now();
        if let Some(prev) = self.last_frame_time {
            let dt = now.saturating_duration_since(prev);
            if self.whole_frames {
                self.whole_frame_remainder +=
                    self.current_frame.fract() + dt.as_secs_f32() * 60.0 * self.playback_speed;
                let steps = self.whole_frame_remainder.floor();
                self.whole_frame_remainder -= steps;
                let mut frame = self.current_frame.floor() + steps;
                if frame > max_frame && self.should_loop {
                    frame = if max_frame > 0.0 {
                        frame.rem_euclid(max_frame).floor()
                    } else {
                        0.0
                    };
                } else if frame > max_frame {
                    frame = max_frame;
                    self.is_playing = false;
                }
                self.current_frame = frame;
            } else {
                self.whole_frame_remainder = 0.0;
                self.current_frame = ssbh_wgpu::next_frame(
                    self.current_frame,
                    dt,
                    max_frame,
                    self.playback_speed,
                    self.should_loop,
                );
                if !self.should_loop && self.current_frame >= max_frame {
                    self.current_frame = max_frame;
                    self.is_playing = false;
                }
            }
            self.apply_pending = true;
        }
        self.last_frame_time = Some(now);
    }
}

pub struct FlipRenderState {
    pub renderer: SsbhRenderer,
    pub shared_data: SharedRenderData,
    pub render_model: Option<RenderModel>,
    pub model_render_options: ModelRenderOptions,
    pub previous_width: f32,
    pub previous_height: f32,
}

impl FlipRenderState {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        // Same initial clear as SSBH Editor (`SsbhRenderer::new(..., [0,0,0,1], ...)`).
        let renderer = SsbhRenderer::new(
            device,
            queue,
            512,
            512,
            1.0,
            [0.0, 0.0, 0.0, 1.0],
            surface_format,
        );
        Self {
            renderer,
            shared_data: SharedRenderData::new(device, queue),
            render_model: None,
            model_render_options: ModelRenderOptions {
                draw_floor_grid: true,
                ..Default::default()
            },
            previous_width: 512.0,
            previous_height: 512.0,
        }
    }
}

pub struct ViewportCallback;

impl CallbackTrait for ViewportCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let state: &mut FlipRenderState = callback_resources.get_mut().unwrap();
        if let Some(render_model) = state.render_model.as_ref() {
            state.renderer.begin_render_models(
                encoder,
                std::slice::from_ref(render_model),
                state.shared_data.database(),
                &state.model_render_options,
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &CallbackResources,
    ) {
        let state: &FlipRenderState = callback_resources.get().unwrap();
        if state.render_model.is_some() {
            state.renderer.end_render_models(render_pass);
        }
    }
}

pub fn calculate_mvp(
    width: f32,
    height: f32,
    camera: &CameraState,
) -> (glam::Vec4, glam::Mat4, glam::Mat4, glam::Mat4) {
    let aspect = if height.abs() > f32::EPSILON {
        width / height
    } else {
        1.0
    };
    let rotation = glam::Mat4::from_euler(
        glam::EulerRot::XYZ,
        camera.rotation_radians.x,
        camera.rotation_radians.y,
        camera.rotation_radians.z,
    );
    let model_view_matrix = glam::Mat4::from_translation(camera.translation) * rotation;
    let projection_matrix = glam::Mat4::perspective_rh(
        camera.fov_y_radians,
        aspect,
        camera.near_clip,
        camera.far_clip,
    );
    let camera_pos = model_view_matrix.inverse().col(3);
    (
        camera_pos,
        model_view_matrix,
        projection_matrix,
        projection_matrix * model_view_matrix,
    )
}

pub fn update_camera(
    queue: &wgpu::Queue,
    renderer: &mut SsbhRenderer,
    camera: &CameraState,
    width: f32,
    height: f32,
    scale_factor: f32,
) {
    let (camera_pos, model_view_matrix, projection_matrix, mvp_matrix) =
        calculate_mvp(width, height, camera);
    renderer.update_camera(
        queue,
        CameraTransforms {
            model_view_matrix,
            mvp_matrix,
            projection_matrix,
            mvp_inv_matrix: mvp_matrix.inverse(),
            camera_pos,
            screen_dimensions: glam::Vec4::new(width, height, scale_factor, 0.0),
        },
    );
}

pub fn handle_camera_input(
    camera: &mut CameraState,
    input: &egui::InputState,
    viewport_height: f32,
    ui_contains_pointer: bool,
) {
    // Start orbit only after the pointer actually moves so a click can pick.
    // Shift is inspect-mode, so don't steal those clicks for the camera.
    if ui_contains_pointer && input.pointer.primary_down() && !input.modifiers.shift {
        if camera.is_mouse_primary_drag || input.pointer.delta().length() > 2.0 {
            camera.is_mouse_primary_drag = true;
        }
    }
    if ui_contains_pointer && input.pointer.secondary_pressed() {
        camera.is_mouse_secondary_drag = true;
    }
    if input.pointer.primary_released() {
        camera.is_mouse_primary_drag = false;
    }
    if input.pointer.secondary_released() {
        camera.is_mouse_secondary_drag = false;
    }

    if camera.is_mouse_primary_drag {
        let delta = input.pointer.delta();
        camera.rotation_radians.x += delta.y * 0.01;
        camera.rotation_radians.y += delta.x * 0.01;
    }

    if camera.is_mouse_secondary_drag {
        let fac = camera.fov_y_radians.sin() * camera.translation.z.abs() / viewport_height.max(1.0);
        let delta = input.pointer.delta();
        camera.translation.x += delta.x * fac;
        camera.translation.y -= delta.y * fac;
    }

    if ui_contains_pointer {
        let delta_z = input.smooth_scroll_delta.y * camera.translation.z.abs() * 0.002;
        if delta_z != 0.0 {
            camera.translation.z = (camera.translation.z + delta_z).min(-1.0);
        }
    }
}

pub fn paint_viewport(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    flip: &FlipPrc,
    wgpu_state: &egui_wgpu::RenderState,
) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

    let scale_factor = ui.pixels_per_point();
    let width = rect.width() * scale_factor;
    let height = rect.height() * scale_factor;
    if width < 1.0 || height < 1.0 {
        return;
    }

    let picking = preview.add_wizard.is_picking();
    let pick_kind = preview.add_wizard.pick_kind();
    let ui_contains_pointer = response.hovered();
    let shift_inspect = ui.input(|i| i.modifiers.shift) && !picking;
    if shift_inspect && !preview.shift_inspect_held {
        preview.inspect_clicked_this_hold = false;
    }
    preview.shift_inspect_held = shift_inspect;
    ui.input(|input| {
        handle_camera_input(
            &mut preview.camera,
            input,
            height,
            ui_contains_pointer,
        );
    });

    let picking_bones = picking
        && matches!(
            pick_kind,
            Some(FlipListKind::BonePairs | FlipListKind::BoneSingles)
        );
    let draw_bones = preview.show_bones || picking_bones;
    {
        let mut renderer = wgpu_state.renderer.write();
        if !renderer.callback_resources.contains::<FlipRenderState>() {
            renderer.callback_resources.insert(FlipRenderState::new(
                &wgpu_state.device,
                &wgpu_state.queue,
                wgpu_state.target_format,
            ));
        }
        let state: &mut FlipRenderState = renderer.callback_resources.get_mut().unwrap();
        state.model_render_options.draw_bones = draw_bones;
        state.model_render_options.draw_bone_axes = draw_bones;

        if preview.needs_gpu_reload {
            if let Some(model) = &preview.model {
                let render_model = RenderModel::from_folder(
                    &wgpu_state.device,
                    &wgpu_state.queue,
                    model,
                    &state.shared_data,
                );
                preview.original_mesh_visibility = snapshot_mesh_visibility(&render_model);
                preview.mesh_entries = preview.original_mesh_visibility.clone();
                state.render_model = Some(render_model);
            } else {
                state.render_model = None;
            }
            preview.needs_gpu_reload = false;
            preview.apply_pending = true;
        }

        if width != state.previous_width || height != state.previous_height {
            state.renderer.resize(
                &wgpu_state.device,
                width.round() as u32,
                height.round() as u32,
                scale_factor,
            );
            state.previous_width = width;
            state.previous_height = height;
        }

        update_camera(
            &wgpu_state.queue,
            &mut state.renderer,
            &preview.camera,
            width,
            height,
            scale_factor,
        );

        if preview.apply_pending {
            if let (Some(render_model), Some(model)) =
                (state.render_model.as_mut(), preview.model.as_ref())
            {
                let anims = FlipPreviewState::enabled_anims(
                    &preview.anim_slots,
                    &preview.anim_folders,
                );
                apply_pose_and_flip(
                    &wgpu_state.queue,
                    render_model,
                    model,
                    &state.shared_data,
                    flip,
                    preview.facing_left,
                    &anims,
                    preview.current_frame,
                    preview.original_modl.as_ref(),
                );
                preview.mesh_entries = snapshot_mesh_visibility(render_model);
            }
            preview.apply_pending = false;
            preview.mesh_vis_command = MeshVisCommand::None;
        } else if preview.mesh_vis_command != MeshVisCommand::None {
            if let Some(render_model) = state.render_model.as_mut() {
                match preview.mesh_vis_command {
                    MeshVisCommand::None => {}
                    MeshVisCommand::ApplyList => {
                        apply_mesh_list_visibility(render_model, &preview.mesh_entries);
                    }
                    MeshVisCommand::ShowAll => show_all_meshes(render_model),
                    MeshVisCommand::HideExpressions => hide_expression_meshes(render_model),
                }
                remap_flip_mesh_pairs(
                    &mut render_model.meshes,
                    flip,
                    preview.facing_left,
                );
                preview.mesh_entries = snapshot_mesh_visibility(render_model);
            }
            preview.mesh_vis_command = MeshVisCommand::None;
        }

        if let Some(render_model) = state.render_model.as_mut() {
            render_model.clear_mesh_selection();
            preview.hovered_viewport = None;
            let picking_meshes = picking
                && matches!(
                    pick_kind,
                    Some(FlipListKind::MeshPairs | FlipListKind::MaterialPairs)
                );
            let pick_mode = if picking_meshes {
                Some(ViewportPickMode::Meshes)
            } else if picking_bones {
                Some(ViewportPickMode::Bones)
            } else if shift_inspect && ui_contains_pointer {
                Some(if draw_bones {
                    ViewportPickMode::Inspect
                } else {
                    ViewportPickMode::Meshes
                })
            } else {
                None
            };
            if pick_mode.is_some() && ui_contains_pointer {
                if let Some(pointer) = response.hover_pos() {
                    let parent_indices: Vec<Option<usize>> = preview
                        .model
                        .as_ref()
                        .and_then(|m| m.find_skel())
                        .map(|skel| skel.bones.iter().map(|b| b.parent_index).collect())
                        .unwrap_or_default();
                    let live_vis: Vec<(String, u64, bool)> = render_model
                        .meshes
                        .iter()
                        .map(|m| (m.name.clone(), m.subindex, m.is_visible))
                        .collect();
                    let vis_src = if shift_inspect {
                        live_vis.as_slice()
                    } else {
                        preview.mesh_entries.as_slice()
                    };
                    preview.hovered_viewport = pick_viewport_hover(
                        pointer,
                        rect,
                        &preview.camera,
                        width,
                        height,
                        &preview.mesh_bounds,
                        vis_src,
                        &preview.bind_world,
                        &render_model.bone_world_transforms(),
                        &parent_indices,
                        render_model.model_transform(),
                        pick_mode,
                    );
                    if picking_meshes {
                        if let Some(ViewportHover::Mesh { name, subindex }) =
                            &preview.hovered_viewport
                        {
                            select_visible_mesh(render_model, name, *subindex);
                        }
                    }
                }
            }

            if shift_inspect {
                match &preview.hovered_viewport {
                    Some(ViewportHover::Mesh { name, subindex }) => {
                        select_visible_mesh(render_model, name, *subindex);
                    }
                    Some(ViewportHover::Bone { .. }) => {}
                    None if !preview.inspect_clicked_this_hold => {
                        if let Some((name, sub)) = &preview.inspected_mesh {
                            select_visible_mesh(render_model, name, *sub);
                        }
                    }
                    None => {}
                }
            } else if !picking_meshes {
                highlight_flip_meshes(render_model, flip, preview);
            }

            if draw_bones {
                if let Some(model) = preview.model.as_ref() {
                    let helpers = helper_bone_names(model.find_hlpb());
                    let mut selected_names = Vec::new();
                    if let Some((list, idx)) = preview
                        .hovered_entry
                        .as_ref()
                        .or(preview.selected_entry.as_ref())
                    {
                        if let Some(entry) = flip.list(list).get(*idx) {
                            selected_names.push(entry.lhs_name.clone());
                            if let Some(rhs) = &entry.rhs_name {
                                selected_names.push(rhs.clone());
                            }
                        }
                    }
                    if picking_bones || shift_inspect {
                        if let Some(ViewportHover::Bone { name }) = &preview.hovered_viewport {
                            selected_names.push(name.clone());
                        }
                    }
                    if let Some(name) = &preview.inspected_bone {
                        selected_names.push(name.clone());
                    }
                    let mut flip_names = FlipPrc::list_bone_names(&flip.flip_bones);
                    let pair_names = FlipPrc::list_bone_names(&flip.pair_bones);
                    let mut base_names = FlipPrc::list_bone_names(&flip.base_bones);
                    let single_names = FlipPrc::list_bone_names(&flip.single_bones);
                    for unknown in &flip.unknown {
                        let names = FlipPrc::list_bone_names(&unknown.entries);
                        if FlipPrc::list_has_pairs(&unknown.entries) {
                            flip_names.extend(names);
                        } else {
                            base_names.extend(names);
                        }
                    }
                    let colors = bone_highlight_colors(
                        render_model.bone_names(),
                        &helpers,
                        &flip_names,
                        &pair_names,
                        &base_names,
                        &single_names,
                        &selected_names,
                        draw_bones,
                    );
                    render_model.set_bone_colors(&wgpu_state.queue, &colors);
                }
            }
        }
    }

    if picking && response.clicked() && !response.dragged() {
        if let Some(hover) = &preview.hovered_viewport {
            let accept = match (preview.add_wizard.pick_kind(), hover) {
                (
                    Some(FlipListKind::MeshPairs | FlipListKind::MaterialPairs),
                    ViewportHover::Mesh { .. },
                ) => true,
                (
                    Some(FlipListKind::BonePairs | FlipListKind::BoneSingles),
                    ViewportHover::Bone { .. },
                ) => true,
                _ => false,
            };
            if accept {
                let category = match &preview.add_wizard {
                    AddEntryWizard::PickLhs { category, .. }
                    | AddEntryWizard::PickRhs { category, .. } => category.as_str(),
                    _ => "",
                };
                let (raw, sub) = match hover {
                    ViewportHover::Mesh { name, subindex } => (name.as_str(), Some(*subindex)),
                    ViewportHover::Bone { name } => (name.as_str(), None),
                };
                preview.pending_viewport_pick =
                    Some(preview.name_for_flip_add(category, raw, sub));
            }
        }
    } else if shift_inspect && response.clicked() && !response.dragged() {
        if let Some(hover) = preview.hovered_viewport.clone() {
            apply_inspect_click(preview, flip, &hover);
        }
    }

    ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
    let cb = egui_wgpu::Callback::new_paint_callback(rect, ViewportCallback);
    ui.painter().add(cb);

    if picking || shift_inspect {
        if let Some(hover) = &preview.hovered_viewport {
            if let Some(pos) = response.hover_pos() {
                draw_hover_label(ui, pos, &hover.label());
            }
        }
    }
}

fn bind_world_map(skel: &SkelData) -> HashMap<String, glam::Mat4> {
    let rest = ssbh_wgpu::animation::AnimationTransforms::from_skel(skel);
    skel.bones
        .iter()
        .enumerate()
        .map(|(i, bone)| (bone.name.clone(), rest.world_transforms[i]))
        .collect()
}

fn mesh_pick_bounds(model: &ModelFolder) -> Vec<MeshPickBounds> {
    let Some(mesh) = model.find_mesh() else {
        return Vec::new();
    };
    let skel = model.find_skel();
    mesh.objects
        .iter()
        .filter_map(|obj| mesh_pick_from_object(obj, skel))
        .collect()
}

const MAX_PICK_SAMPLES: usize = 160;

fn mesh_pick_from_object(
    obj: &ssbh_data::mesh_data::MeshObjectData,
    skel: Option<&SkelData>,
) -> Option<MeshPickBounds> {
    let attr = obj.positions.first()?;
    let pts = attr.data.to_vec4_with_w(1.0);
    if pts.is_empty() {
        return None;
    }
    let parent_bone = if obj.bone_influences.is_empty() {
        skel.and_then(|skel| {
            skel.bones
                .iter()
                .position(|b| b.name == obj.parent_bone_name)
                .map(|i| i as i32)
        })
        .unwrap_or(-1)
    } else {
        -1
    };
    let weights = vertex_bone_weights(obj, skel, pts.len());
    let stride = (pts.len() / MAX_PICK_SAMPLES).max(1);
    let mut samples = Vec::new();
    for (i, p) in pts.iter().enumerate().step_by(stride) {
        samples.push(PickSample {
            pos: glam::Vec3::new(p[0], p[1], p[2]),
            bones: weights.get(i).copied().unwrap_or([(-1, 0.0); 4]),
        });
        if samples.len() >= MAX_PICK_SAMPLES {
            break;
        }
    }
    Some(MeshPickBounds {
        name: obj.name.clone(),
        subindex: obj.subindex,
        samples,
        parent_bone,
    })
}

fn vertex_bone_weights(
    obj: &ssbh_data::mesh_data::MeshObjectData,
    skel: Option<&SkelData>,
    vertex_count: usize,
) -> Vec<[(i16, f32); 4]> {
    let mut weights = vec![[(-1i16, 0.0f32); 4]; vertex_count];
    let Some(skel) = skel else {
        return weights;
    };
    for influence in &obj.bone_influences {
        let Some(bone_index) = skel
            .bones
            .iter()
            .position(|b| b.name == influence.bone_name)
        else {
            continue;
        };
        let bone_index = bone_index as i16;
        for w in &influence.vertex_weights {
            let Some(slot) = weights.get_mut(w.vertex_index as usize) else {
                continue;
            };
            if let Some(entry) = slot.iter_mut().find(|(i, _)| *i < 0) {
                *entry = (bone_index, w.vertex_weight);
            }
        }
    }
    weights
}

fn posed_sample(
    sample: &PickSample,
    parent_bone: i32,
    bone_worlds: &[glam::Mat4],
    bind_worlds: &[glam::Mat4],
    model_transform: glam::Mat4,
) -> glam::Vec3 {
    if parent_bone >= 0 {
        if let Some(world) = bone_worlds.get(parent_bone as usize) {
            return world.transform_point3(sample.pos);
        }
        return model_transform.transform_point3(sample.pos);
    }
    let mut posed = glam::Vec3::ZERO;
    let mut weight_sum = 0.0;
    for &(bone, weight) in &sample.bones {
        if bone < 0 || weight == 0.0 {
            continue;
        }
        let i = bone as usize;
        let Some(anim) = bone_worlds.get(i) else {
            continue;
        };
        let bind = bind_worlds.get(i).copied().unwrap_or(glam::Mat4::IDENTITY);
        posed += weight * anim.transform_point3(bind.inverse().transform_point3(sample.pos));
        weight_sum += weight;
    }
    if weight_sum > 0.0 {
        posed
    } else {
        model_transform.transform_point3(sample.pos)
    }
}

fn pick_viewport_hover(
    pointer: egui::Pos2,
    rect: egui::Rect,
    camera: &CameraState,
    width: f32,
    height: f32,
    mesh_bounds: &[MeshPickBounds],
    mesh_entries: &[(String, u64, bool)],
    bind_world: &HashMap<String, glam::Mat4>,
    bone_transforms: &[(String, glam::Mat4)],
    parent_indices: &[Option<usize>],
    model_transform: glam::Mat4,
    mode: Option<ViewportPickMode>,
) -> Option<ViewportHover> {
    let Some(mode) = mode else {
        return None;
    };
    let (_, _, _, mvp) = calculate_mvp(width, height, camera);
    let bone_worlds: Vec<glam::Mat4> = bone_transforms.iter().map(|(_, m)| *m).collect();
    let bind_worlds: Vec<glam::Mat4> = bone_transforms
        .iter()
        .map(|(name, _)| bind_world.get(name).copied().unwrap_or(glam::Mat4::IDENTITY))
        .collect();

    let pick_meshes = matches!(mode, ViewportPickMode::Meshes | ViewportPickMode::Inspect);
    let pick_bones = matches!(mode, ViewportPickMode::Bones | ViewportPickMode::Inspect);
    let bone_radius = if mode == ViewportPickMode::Inspect {
        16.0
    } else {
        12.0
    };

    let mut best_mesh: Option<(f32, f32, ViewportHover)> = None;
    if pick_meshes {
        for bounds in mesh_bounds {
            let visible = mesh_entries
                .iter()
                .find(|(name, sub, _)| name == &bounds.name && *sub == bounds.subindex)
                .map(|(_, _, vis)| *vis)
                .unwrap_or(false);
            if !visible {
                continue;
            }
            let mut closest = f32::MAX;
            let mut closest_depth = f32::MAX;
            for sample in &bounds.samples {
                let world = posed_sample(
                    sample,
                    bounds.parent_bone,
                    &bone_worlds,
                    &bind_worlds,
                    model_transform,
                );
                let Some(screen) = project_world_to_screen(world, mvp, rect) else {
                    continue;
                };
                let dist = pointer.distance(screen);
                if dist < closest {
                    closest = dist;
                    closest_depth = screen_depth(world, mvp);
                }
            }
            const MESH_HIT_PX: f32 = 18.0;
            if closest > MESH_HIT_PX {
                continue;
            }
            let better = match &best_mesh {
                None => true,
                Some((best_dist, best_depth, _)) => {
                    closest + 0.5 < *best_dist
                        || ((closest - *best_dist).abs() <= 4.0 && closest_depth < *best_depth)
                }
            };
            if better {
                best_mesh = Some((
                    closest,
                    closest_depth,
                    ViewportHover::Mesh {
                        name: bounds.name.clone(),
                        subindex: bounds.subindex,
                    },
                ));
            }
        }
    }

    let mut best_bone: Option<(f32, ViewportHover)> = None;
    if pick_bones {
        for (i, (name, transform)) in bone_transforms.iter().enumerate() {
            let joint = transform.col(3).truncate();
            let Some(head) = project_world_to_screen(joint, mvp, rect) else {
                continue;
            };
            let mut dist = pointer.distance(head);
            if let Some(parent) = parent_indices.get(i).and_then(|p| *p) {
                if let Some((_, parent_transform)) = bone_transforms.get(parent) {
                    let parent_joint = parent_transform.col(3).truncate();
                    if let Some(tail) = project_world_to_screen(parent_joint, mvp, rect) {
                        dist = dist.min(dist_to_segment(pointer, head, tail));
                    }
                }
            }
            if dist <= bone_radius {
                if best_bone.as_ref().map(|(s, _)| dist < *s).unwrap_or(true) {
                    best_bone = Some((dist, ViewportHover::Bone { name: name.clone() }));
                }
            }
        }
    }

    match mode {
        ViewportPickMode::Meshes => best_mesh.map(|(_, _, h)| h),
        ViewportPickMode::Bones => best_bone.map(|(_, h)| h),
        ViewportPickMode::Inspect => best_mesh
            .map(|(_, _, h)| h)
            .or_else(|| best_bone.map(|(_, h)| h)),
    }
}

fn apply_inspect_click(preview: &mut FlipPreviewState, flip: &FlipPrc, hover: &ViewportHover) {
    let found = find_affected_entry(flip, hover, preview.original_modl.as_ref());
    match hover {
        ViewportHover::Mesh { name, subindex } => {
            preview.right_panel = RightPanelTab::Meshes;
            preview.inspected_mesh = Some((name.clone(), *subindex));
            preview.inspected_bone = None;
            preview.scroll_to_inspected_mesh = true;
            preview.inspect_clicked_this_hold = true;
        }
        ViewportHover::Bone { name } => {
            preview.inspected_bone = Some(name.clone());
            preview.inspected_mesh = None;
            preview.inspect_clicked_this_hold = true;
        }
    }
    if let Some(found) = found {
        preview.selected_entry = Some(found);
        preview.scroll_to_selected_entry = true;
        preview.apply_pending = true;
    } else {
        preview.selected_entry = None;
    }
}

fn find_affected_entry(
    flip: &FlipPrc,
    hover: &ViewportHover,
    modl: Option<&ModlData>,
) -> Option<(String, usize)> {
    for list_name in flip.category_names() {
        let kind = flip.list_kind(&list_name);
        for (i, entry) in flip.list(&list_name).iter().enumerate() {
            let hit = match hover {
                ViewportHover::Mesh { name, subindex } => match kind {
                    FlipListKind::MeshPairs => {
                        mesh_key_matches(&entry.lhs_name, name)
                            || entry
                                .rhs_name
                                .as_deref()
                                .is_some_and(|rhs| mesh_key_matches(rhs, name))
                    }
                    FlipListKind::MaterialPairs => mesh_uses_listed_material(
                        name,
                        *subindex,
                        &entry.lhs_name,
                        entry.rhs_name.as_deref(),
                        modl,
                    ),
                    _ => false,
                },
                ViewportHover::Bone { name } => {
                    if matches!(kind, FlipListKind::MeshPairs | FlipListKind::MaterialPairs) {
                        continue;
                    }
                    entry.lhs_name.eq_ignore_ascii_case(name)
                        || entry
                            .rhs_name
                            .as_deref()
                            .is_some_and(|rhs| rhs.eq_ignore_ascii_case(name))
                }
            };
            if hit {
                return Some((list_name, i));
            }
        }
    }
    None
}

fn mesh_uses_listed_material(
    mesh_name: &str,
    subindex: u64,
    lhs: &str,
    rhs: Option<&str>,
    modl: Option<&ModlData>,
) -> bool {
    let labels = mesh_material_labels(modl, mesh_name, subindex);
    let listed = std::iter::once(lhs).chain(rhs);
    if labels.iter().any(|label| {
        listed
            .clone()
            .any(|name| material_label_matches(label, name))
    }) {
        return true;
    }
    listed.clone().any(|name| material_label_matches(mesh_name, name))
}

fn mesh_material_labels(
    modl: Option<&ModlData>,
    mesh_name: &str,
    subindex: u64,
) -> Vec<String> {
    let Some(modl) = modl else {
        return Vec::new();
    };
    modl.entries
        .iter()
        .filter(|entry| {
            entry.mesh_object_name.eq_ignore_ascii_case(mesh_name)
                && entry.mesh_object_subindex == subindex
        })
        .map(|entry| entry.material_label.clone())
        .collect()
}

fn material_label_matches(label: &str, listed: &str) -> bool {
    let label = label.to_ascii_lowercase();
    let listed = listed.trim().to_ascii_lowercase();
    if listed.len() < 3 {
        return false;
    }
    label == listed || label.ends_with(&listed) || label.contains(&listed)
}

fn mesh_key_matches(listed: &str, mesh_name: &str) -> bool {
    mesh_flip_key(listed) == mesh_flip_key(mesh_name)
}

fn dist_to_segment(point: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    if len_sq < 1.0 {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let closest = a + ab * t;
    point.distance(closest)
}

fn project_world_to_screen(
    world: glam::Vec3,
    mvp: glam::Mat4,
    rect: egui::Rect,
) -> Option<egui::Pos2> {
    let clip = mvp * world.extend(1.0);
    if clip.w.abs() < f32::EPSILON {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    Some(egui::pos2(
        rect.min.x + (ndc.x * 0.5 + 0.5) * rect.width(),
        rect.min.y + (1.0 - (ndc.y * 0.5 + 0.5)) * rect.height(),
    ))
}

fn screen_depth(world: glam::Vec3, mvp: glam::Mat4) -> f32 {
    let clip = mvp * world.extend(1.0);
    if clip.w.abs() < f32::EPSILON {
        return f32::MAX;
    }
    clip.z / clip.w
}

fn select_visible_mesh(render_model: &mut RenderModel, name: &str, subindex: u64) {
    for mesh in &mut render_model.meshes {
        mesh.is_selected =
            mesh.is_visible && mesh.name == name && mesh.subindex == subindex;
    }
}

fn highlight_flip_meshes(
    render_model: &mut RenderModel,
    flip: &FlipPrc,
    preview: &FlipPreviewState,
) {
    let mut targets: Vec<String> = Vec::new();
    if let Some((list, idx)) = preview
        .hovered_entry
        .as_ref()
        .or(preview.selected_entry.as_ref())
    {
        if matches!(
            FlipListKind::for_category(list),
            FlipListKind::MeshPairs | FlipListKind::MaterialPairs
        ) {
            if let Some(entry) = flip.list(list).get(*idx) {
                targets.push(entry.lhs_name.clone());
                if let Some(rhs) = &entry.rhs_name {
                    targets.push(rhs.clone());
                }
            }
        }
    }
    if targets.is_empty() {
        return;
    }
    let material_list = matches!(
        FlipListKind::for_category(
            preview
                .hovered_entry
                .as_ref()
                .or(preview.selected_entry.as_ref())
                .map(|(list, _)| list.as_str())
                .unwrap_or("")
        ),
        FlipListKind::MaterialPairs
    );
    for mesh in &mut render_model.meshes {
        if !mesh.is_visible {
            mesh.is_selected = false;
            continue;
        }
        let by_name = targets.iter().any(|target| {
            crate::flip::mesh_flip_key(&mesh.name) == crate::flip::mesh_flip_key(target)
        });
        let by_material = material_list
            && preview.original_modl.as_ref().is_some_and(|modl| {
                mesh_uses_listed_material(
                    &mesh.name,
                    mesh.subindex,
                    targets[0].as_str(),
                    targets.get(1).map(String::as_str),
                    Some(modl),
                )
            });
        mesh.is_selected = by_name || by_material;
    }
}

fn draw_hover_label(ui: &egui::Ui, pointer: egui::Pos2, text: &str) {
    let pos = pointer + egui::vec2(16.0, 10.0);
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(14.0),
        egui::Color32::WHITE,
    );
    let rect = egui::Rect::from_min_size(pos, galley.size()).expand(5.0);
    ui.painter().rect_filled(
        rect,
        4.0,
        egui::Color32::from_rgba_unmultiplied(20, 40, 70, 220),
    );
    ui.painter().rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 170, 255)),
        egui::StrokeKind::Outside,
    );
    ui.painter().galley(pos, galley, egui::Color32::WHITE);
}
