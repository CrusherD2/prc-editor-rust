use crate::flip::{
    append_flip_entry, guess_pair_name, is_flip_prc, remove_flip_entry, suggested_model_folder,
    suggested_motion_folder, update_flip_entry, FlipEntry, FlipListKind, FlipPrc,
};
use crate::param_file::ParamFile;
use crate::viewport::{
    paint_viewport, AddEntryWizard, AnimFolder, AnimIndex, AnimSlot, CameraState, FlipPreviewState,
    MainTab, MeshVisCommand, RightPanelTab, ViewportView,
};
use eframe::egui;
use rfd::FileDialog;
use std::path::Path;

pub fn show_tab_bar(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    param_file: &ParamFile,
    current_file_path: Option<&Path>,
    can_crack: bool,
) {
    let flip_available = is_flip_prc(param_file, current_file_path);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut preview.tab, MainTab::Editor, "Editor");
        let flip_tab = ui.add_enabled(
            flip_available,
            egui::Button::selectable(preview.tab == MainTab::FlipPreview, "Flip Preview"),
        );
        if flip_tab.clicked() && flip_available {
            preview.tab = MainTab::FlipPreview;
            preview.apply_pending = true;
        }
        if !flip_available {
            flip_tab.on_hover_text("Open a flip.prc to use the 3D preview");
            if preview.tab == MainTab::FlipPreview {
                preview.tab = MainTab::Editor;
            }
        }
        if ui
            .add_enabled(can_crack, egui::Button::new("Crack hashes"))
            .on_hover_text("Hash the loaded model's bones, meshes, and materials against unresolved 0x names, then try ParamLabels")
            .clicked()
        {
            preview.request_crack = true;
        }
    });
}

pub fn show_flip_preview(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    current_file_path: Option<&Path>,
    status_message: &mut String,
    wgpu_state: Option<&egui_wgpu::RenderState>,
) {
    let flip = FlipPrc::from_param_file(param_file).unwrap_or_default();
    preview.tick_animation();
    if preview.is_playing {
        ui.ctx().request_repaint();
    }

    ui.horizontal(|ui| {
        if ui.button("Load Model Folder…").clicked() {
            let mut dialog = FileDialog::new();
            if let Some(path) = current_file_path.and_then(suggested_model_folder) {
                dialog = dialog.set_directory(path);
            } else if let Some(parent) = current_file_path.and_then(|p| p.parent()) {
                dialog = dialog.set_directory(parent);
            }
            if let Some(folder) = dialog.pick_folder() {
                preview.load_model_folder(folder.clone());
                *status_message = format!("Loaded model folder {}", folder.display());
            }
        }

        if let Some(suggested) = current_file_path.and_then(suggested_model_folder) {
            if suggested.exists() && ui.button("Load matching model").clicked() {
                preview.load_model_folder(suggested.clone());
                *status_message = format!("Loaded {}", suggested.display());
            }
        }

        if ui.button("Load Anim Folder…").clicked() {
            let mut dialog = FileDialog::new();
            if let Some(motion) = current_file_path.and_then(suggested_motion_folder) {
                dialog = dialog.set_directory(motion);
            } else if let Some(parent) = current_file_path.and_then(|p| p.parent()) {
                dialog = dialog.set_directory(parent);
            }
            if let Some(folder) = dialog.pick_folder() {
                let display = folder.display().to_string();
                preview.load_anim_folder(folder);
                *status_message = format!(
                    "Added motion folder {display} ({} animations). Select one in a slot.",
                    preview
                        .anim_folders
                        .last()
                        .map(|folder| folder.anims.len())
                        .unwrap_or(0)
                );
            }
        }

        if let Some(suggested) = current_file_path.and_then(suggested_motion_folder) {
            if suggested.exists() && ui.button("Load matching motion").clicked() {
                let display = suggested.display().to_string();
                preview.load_anim_folder(suggested);
                *status_message = format!(
                    "Added motion folder {display} ({} animations). Select one in a slot.",
                    preview
                        .anim_folders
                        .last()
                        .map(|folder| folder.anims.len())
                        .unwrap_or(0)
                );
            }
        }

        if ui
            .selectable_label(preview.view_mode == ViewportView::Front, "Front view")
            .on_hover_text("Look at the face")
            .clicked()
        {
            preview.view_mode = ViewportView::Front;
            preview.camera = CameraState::front_view();
        }
        if ui
            .selectable_label(preview.view_mode == ViewportView::Smash, "Smash view")
            .on_hover_text("In-game camera: Right faces screen-right, Left faces screen-left")
            .clicked()
        {
            preview.view_mode = ViewportView::Smash;
            preview.camera = CameraState::smash_side_view();
        }

        ui.separator();
        ui.label("Face:");
        if ui
            .selectable_label(!preview.facing_left, "Right")
            .clicked()
        {
            preview.facing_left = false;
            preview.apply_pending = true;
        }
        if ui.selectable_label(preview.facing_left, "Left").clicked() {
            preview.facing_left = true;
            preview.apply_pending = true;
        }
        if ui
            .checkbox(&mut preview.always_face_camera, "Always face camera")
            .on_hover_text(
                "When facing left, rotate the model another 180° around Y so its front remains visible",
            )
            .changed()
        {
            preview.apply_pending = true;
        }
        ui.label(
            egui::RichText::new("Left = 180° + trans XYZ (listed) + flip.prc")
                .small()
                .color(egui::Color32::GRAY),
        );

        ui.separator();
        if ui
            .checkbox(&mut preview.show_bones, "Show bones")
            .changed()
        {
            preview.apply_pending = true;
        }
        if preview.show_bones {
            ui.colored_label(egui::Color32::from_rgb(255, 140, 20), "● flip");
            ui.colored_label(egui::Color32::from_rgb(242, 217, 38), "● pair");
            ui.colored_label(egui::Color32::from_rgb(242, 89, 191), "● base");
            ui.colored_label(egui::Color32::from_rgb(89, 230, 102), "● single");
            ui.colored_label(egui::Color32::from_rgb(80, 180, 255), "● mesh");
            ui.colored_label(egui::Color32::from_rgb(40, 220, 255), "● selected");
        }
        ui.separator();
        ui.label(
            egui::RichText::new("Shift: inspect mesh/bone")
                .small()
                .color(egui::Color32::GRAY),
        );
        ui.separator();
        ui.label(
            egui::RichText::new("Shift: inspect mesh/bone")
                .small()
                .color(egui::Color32::GRAY),
        );
    });

    if let Some(path) = &preview.model_path {
        ui.label(format!("Model: {}", path.display()));
    } else {
        ui.colored_label(
            egui::Color32::YELLOW,
            "Load a Smash model folder (numdlb / numshb / nusktb / numatb / nutexb) to preview.",
        );
    }

    if let Some(err) = &preview.load_error {
        ui.colored_label(egui::Color32::LIGHT_RED, err);
    }

    ui.separator();

    egui::Panel::bottom("flip_anim_bar")
        .resizable(false)
        .min_size(72.0)
        .show(ui, |ui| {
            show_animation_bar(ui, preview);
        });

    egui::Panel::left("flip_preview_lists")
        .resizable(true)
        .default_size(280.0)
        .min_size(180.0)
        .show(ui, |ui| {
            show_flip_lists(ui, &flip, preview, param_file, status_message);
        });

    egui::Panel::right("flip_preview_meshes")
        .resizable(true)
        .default_size(260.0)
        .min_size(180.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut preview.right_panel, RightPanelTab::Meshes, "Meshes");
                ui.selectable_value(&mut preview.right_panel, RightPanelTab::Anims, "Anims");
            });
            ui.separator();
            match preview.right_panel {
                RightPanelTab::Meshes => {
                    show_mesh_visibility(ui, preview, param_file, status_message);
                }
                RightPanelTab::Anims => {
                    show_anim_list(ui, preview, current_file_path, status_message);
                }
            }
        });

    show_add_wizard(ui, preview, param_file, &flip, status_message);

    let flip = FlipPrc::from_param_file(param_file).unwrap_or_default();

    egui::CentralPanel::default().show(ui, |ui| {
        if preview.model.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label("Load a model folder to see facing-left flip.prc changes.");
            });
            return;
        }
        if let Some(wgpu_state) = wgpu_state {
            paint_viewport(ui, preview, &flip, wgpu_state);
        } else {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "wgpu renderer is not available. Restart the editor.",
            );
        }
    });
}

fn show_flip_lists(
    ui: &mut egui::Ui,
    flip: &FlipPrc,
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    status_message: &mut String,
) {
    preview.hovered_entry = None;
    ui.horizontal(|ui| {
        ui.heading("Flip lists");
        if ui.button("+ New entry").clicked() {
            preview.add_wizard = AddEntryWizard::PickCategory {
                category: "flip_bones".to_string(),
            };
        }
    });
    ui.label("Click an entry to edit. + New entry lets you click bones/meshes in the 3D view.");
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 180.0)
        .show(ui, |ui| {
            for name in flip.category_names() {
                list_section(ui, &name, flip.list(&name), preview);
            }
        });
    preview.scroll_to_selected_entry = false;

    ui.separator();
    if let Some((list_name, index)) = preview.selected_entry.clone() {
        if let Some(entry) = flip.list(&list_name).get(index).cloned() {
            show_entry_editor(
                ui,
                preview,
                param_file,
                &list_name,
                index,
                entry,
                status_message,
            );
        } else {
            preview.selected_entry = None;
        }
    } else {
        ui.weak("Select an entry to edit names, axes, and scale.");
    }
}

fn show_entry_editor(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    list_name: &str,
    index: usize,
    mut entry: FlipEntry,
    status_message: &mut String,
) {
    ui.heading("Edit entry");
    ui.label(format!("{list_name} [{index}]"));
    let candidates = preview.picker_names(list_name);
    let kind = FlipListKind::for_category(list_name);
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(format!("{}:", kind.noun()));
        if name_combo(ui, "edit_lhs", &mut entry.lhs_name, &candidates) {
            entry.lhs_name = stored_entry_name(kind, preview, &entry.lhs_name, None);
            changed = true;
        }
    });
    if kind.allows_pair() || entry.rhs_name.is_some() {
        ui.horizontal(|ui| {
            ui.label("flipped:");
            let mut rhs = entry.rhs_name.clone().unwrap_or_default();
            if name_combo(ui, "edit_rhs", &mut rhs, &candidates) {
                if rhs.is_empty() {
                    entry.rhs_name = None;
                } else {
                    entry.rhs_name = Some(stored_entry_name(kind, preview, &rhs, None));
                }
                changed = true;
            }
        });
    }

    if matches!(kind, FlipListKind::BonePairs | FlipListKind::BoneSingles) {
        ui.horizontal(|ui| {
            ui.label("trans");
            changed |= axis_checkboxes(ui, "edit_trans", &mut entry.trans);
        });
        ui.horizontal(|ui| {
            ui.label("rot");
            changed |= axis_checkboxes(ui, "edit_rot", &mut entry.rot);
        });
        if ui.checkbox(&mut entry.scale, "scale").changed() {
            changed = true;
        }
    }

    ui.horizontal(|ui| {
        if ui.button("Delete").clicked() {
            if remove_flip_entry(param_file, list_name, index) {
                *status_message = format!("Deleted {list_name}[{index}]");
                preview.selected_entry = None;
                preview.apply_pending = true;
                preview.param_dirty = true;
            }
        }
    });

    if changed {
        if update_flip_entry(param_file, list_name, index, &entry) {
            preview.apply_pending = true;
            preview.param_dirty = true;
            *status_message = format!("Updated {list_name}[{index}]");
        }
    }
}

fn axis_checkboxes(ui: &mut egui::Ui, id: &str, mask: &mut crate::flip::AxisMask) -> bool {
    let mut changed = false;
    ui.push_id(id, |ui| {
        changed |= ui.checkbox(&mut mask.x, "X").changed();
        changed |= ui.checkbox(&mut mask.y, "Y").changed();
        changed |= ui.checkbox(&mut mask.z, "Z").changed();
    });
    changed
}

fn name_combo(ui: &mut egui::Ui, id: &str, value: &mut String, candidates: &[String]) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .width(160.0)
        .show_ui(ui, |ui| {
            ui.set_min_width(200.0);
            for name in candidates {
                if ui.selectable_label(*value == *name, name).clicked() {
                    *value = name.clone();
                    changed = true;
                }
            }
        });
    if ui
        .add(egui::TextEdit::singleline(value).desired_width(120.0).id_salt(format!("{id}_edit")))
        .changed()
    {
        changed = true;
    }
    changed
}

fn entry_list_label(entry: &FlipEntry) -> String {
    let mut label = entry.pair_name();
    let has_flags = entry.trans != crate::flip::AxisMask::default()
        || entry.rot != crate::flip::AxisMask::default()
        || entry.scale;
    if has_flags {
        label.push_str(&format!(
            "  {} / {}",
            entry.trans.to_label(),
            entry.rot.to_label()
        ));
        if entry.scale {
            label.push_str(" scale");
        }
    }
    label
}

fn entry_hover_text(entry: &FlipEntry) -> String {
    match &entry.rhs_name {
        Some(rhs) if rhs != &entry.lhs_name => format!(
            "{} → {}\ntrans: {}\nrot: {}\nscale: {}",
            entry.lhs_name,
            rhs,
            entry.trans.to_label(),
            entry.rot.to_label(),
            entry.scale
        ),
        _ => format!(
            "{}\ntrans: {}\nrot: {}\nscale: {}",
            entry.lhs_name,
            entry.trans.to_label(),
            entry.rot.to_label(),
            entry.scale
        ),
    }
}

fn list_header_label(title: &str, entries: &[FlipEntry], label: &str) -> egui::RichText {
    let color = match title {
        "flip_bones" => egui::Color32::from_rgb(255, 140, 20),
        "pair_bones" => egui::Color32::from_rgb(242, 217, 38),
        "base_bones" => egui::Color32::from_rgb(242, 89, 191),
        "single_bones" => egui::Color32::from_rgb(89, 230, 102),
        "meshes" => egui::Color32::from_rgb(80, 180, 255),
        "pair_materials" => egui::Color32::from_rgb(180, 140, 255),
        _ => match FlipListKind::for_category(title) {
            FlipListKind::MeshPairs => egui::Color32::from_rgb(80, 180, 255),
            FlipListKind::MaterialPairs => egui::Color32::from_rgb(180, 140, 255),
            _ if crate::flip::FlipPrc::list_has_pairs(entries) => {
                egui::Color32::from_rgb(255, 140, 20)
            }
            FlipListKind::BoneSingles => egui::Color32::from_rgb(89, 230, 102),
            _ => egui::Color32::from_rgb(255, 140, 20),
        },
    };
    egui::RichText::new(label).color(color)
}

fn list_section(
    ui: &mut egui::Ui,
    title: &str,
    entries: &[FlipEntry],
    preview: &mut FlipPreviewState,
) {
    let label = format!("{title} ({})", entries.len());
    let header = list_header_label(title, entries, &label);
    egui::CollapsingHeader::new(header)
        .default_open(true)
        .show(ui, |ui| {
            if entries.is_empty() {
                ui.weak("empty");
                return;
            }
            for (i, entry) in entries.iter().enumerate() {
                let selected = preview
                    .selected_entry
                    .as_ref()
                    .map(|(t, idx)| t == title && *idx == i)
                    .unwrap_or(false);
                let response = ui.selectable_label(selected, entry_list_label(entry));
                if selected && preview.scroll_to_selected_entry {
                    response.scroll_to_me(Some(egui::Align::Center));
                }
                if response.hovered() {
                    preview.hovered_entry = Some((title.to_string(), i));
                }
                if response.clicked() {
                    preview.selected_entry = Some((title.to_string(), i));
                    preview.apply_pending = true;
                }
                response.on_hover_text(entry_hover_text(entry));
            }
        });
}

fn show_add_wizard(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    flip: &FlipPrc,
    status_message: &mut String,
) {
    if matches!(preview.add_wizard, AddEntryWizard::Inactive) {
        preview.pending_viewport_pick = None;
        return;
    }

    if let Some(name) = preview.pending_viewport_pick.take() {
        match preview.add_wizard.clone() {
            AddEntryWizard::PickLhs { category, .. } => {
                finish_or_pick_rhs(preview, param_file, &category, name, status_message);
                return;
            }
            AddEntryWizard::PickRhs { category, lhs, .. } => {
                create_new_entry(preview, param_file, category, lhs, Some(name), status_message);
                return;
            }
            _ => {}
        }
    }

    let mut open = true;
    let mut picked_lhs: Option<(String, String)> = None;
    let mut picked_rhs: Option<(String, String, Option<String>)> = None;
    let mut next_step: Option<AddEntryWizard> = None;
    let picker_names = match &preview.add_wizard {
        AddEntryWizard::PickLhs { category, .. }
        | AddEntryWizard::PickRhs { category, .. } => preview.picker_names(category),
        _ => Vec::new(),
    };
    let mut wizard = preview.add_wizard.clone();

    egui::Window::new("New flip entry")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(360.0)
        .show(ui.ctx(), |ui| {
            match &mut wizard {
                AddEntryWizard::Inactive => {}
                AddEntryWizard::PickCategory { category } => {
                    ui.label("Which list should this go in?");
                    egui::ComboBox::from_id_salt("wizard_category")
                        .selected_text(category.clone())
                        .show_ui(ui, |ui| {
                            for name in flip.category_names() {
                                ui.selectable_value(category, name.clone(), name);
                            }
                        });
                    if ui.button("Next: pick source").clicked() {
                        next_step = Some(AddEntryWizard::PickLhs {
                            category: category.clone(),
                            filter: String::new(),
                        });
                    }
                }
                AddEntryWizard::PickLhs { category, filter } => {
                    let kind = FlipListKind::for_category(category);
                    ui.label(if kind == FlipListKind::MaterialPairs {
                        "Click a mesh in the 3D view that uses the material, or pick a material from the list.".to_string()
                    } else {
                        format!(
                            "Click the {} in the 3D viewport (outlined in blue). You can also pick from the list.",
                            kind.noun()
                        )
                    });
                    ui.text_edit_singleline(filter);
                    let names = &picker_names;
                    let filter_lower = filter.to_ascii_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for name in names {
                                if !filter_lower.is_empty()
                                    && !name.to_ascii_lowercase().contains(&filter_lower)
                                {
                                    continue;
                                }
                                if ui.selectable_label(false, name).clicked() {
                                    picked_lhs = Some((category.clone(), name.clone()));
                                }
                            }
                        });
                }
                AddEntryWizard::PickRhs {
                    category,
                    lhs,
                    filter,
                } => {
                    let kind = FlipListKind::for_category(category);
                    ui.label(if kind == FlipListKind::MaterialPairs {
                        format!("Click a mesh that uses the flipped material for `{lhs}`.")
                    } else {
                        format!(
                            "Click the flipped {} in the 3D viewport for `{lhs}`.",
                            kind.noun()
                        )
                    });
                    ui.text_edit_singleline(filter);
                    let names = &picker_names;
                    let guessed = guess_pair_name(lhs, names);
                    if let Some(guess) = &guessed {
                        ui.colored_label(
                            egui::Color32::LIGHT_BLUE,
                            format!("Suggested: {guess}"),
                        );
                        if ui.button(format!("Use {guess}")).clicked() {
                            picked_rhs = Some((
                                category.clone(),
                                lhs.clone(),
                                Some(guess.clone()),
                            ));
                        }
                    }
                    let filter_lower = filter.to_ascii_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for name in names {
                                if name.eq_ignore_ascii_case(lhs) {
                                    continue;
                                }
                                if !filter_lower.is_empty()
                                    && !name.to_ascii_lowercase().contains(&filter_lower)
                                {
                                    continue;
                                }
                                if ui.selectable_label(false, name).clicked() {
                                    picked_rhs =
                                        Some((category.clone(), lhs.clone(), Some(name.clone())));
                                }
                            }
                        });
                    if ui.button("Skip (no pair)").clicked() {
                        picked_rhs = Some((category.clone(), lhs.clone(), None));
                    }
                }
            }
        });

    if next_step.is_none() && picked_lhs.is_none() && picked_rhs.is_none() && open {
        preview.add_wizard = wizard;
    }

    if let Some(step) = next_step {
        preview.add_wizard = step;
    }
    if let Some((category, lhs)) = picked_lhs {
        finish_or_pick_rhs(preview, param_file, &category, lhs, status_message);
    }
    if let Some((category, lhs, rhs)) = picked_rhs {
        create_new_entry(preview, param_file, category, lhs, rhs, status_message);
    }
    if !open {
        preview.add_wizard = AddEntryWizard::Inactive;
    }
}

fn stored_entry_name(
    kind: FlipListKind,
    preview: &FlipPreviewState,
    name: &str,
    subindex: Option<u64>,
) -> String {
    preview.store_flip_name(kind, name, subindex)
}

fn finish_or_pick_rhs(
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    category: &str,
    lhs: String,
    status_message: &mut String,
) {
    let kind = FlipListKind::for_category(category);
    let lhs = stored_entry_name(kind, preview, &lhs, None);
    if kind.allows_pair() {
        preview.add_wizard = AddEntryWizard::PickRhs {
            category: category.to_string(),
            lhs,
            filter: String::new(),
        };
    } else {
        create_new_entry(
            preview,
            param_file,
            category.to_string(),
            lhs,
            None,
            status_message,
        );
    }
}

fn create_new_entry(
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    category: String,
    lhs: String,
    rhs: Option<String>,
    status_message: &mut String,
) {
    let kind = FlipListKind::for_category(&category);
    let lhs = stored_entry_name(kind, preview, &lhs, None);
    let rhs = rhs.map(|name| stored_entry_name(kind, preview, &name, None));
    let template = FlipPrc::from_param_file(param_file)
        .unwrap_or_default()
        .list(&category)
        .last()
        .cloned();
    let entry = FlipEntry {
        lhs_name: lhs.clone(),
        rhs_name: rhs.clone(),
        trans: template.as_ref().map(|t| t.trans).unwrap_or_default(),
        rot: template.as_ref().map(|t| t.rot).unwrap_or_default(),
        scale: template.as_ref().map(|t| t.scale).unwrap_or(false),
    };
    if append_flip_entry(param_file, &category, &entry) {
        let count = FlipPrc::from_param_file(param_file)
            .map(|f| f.list(&category).len())
            .unwrap_or(1);
        preview.selected_entry = Some((category.clone(), count.saturating_sub(1)));
        preview.apply_pending = true;
        preview.param_dirty = true;
        preview.add_wizard = AddEntryWizard::Inactive;
        *status_message = match rhs {
            Some(rhs) => format!("Added {lhs} ↔ {rhs} to {category}"),
            None => format!("Added {lhs} to {category}"),
        };
    } else {
        *status_message = format!("Could not add to {category}");
    }
}

fn show_animation_bar(ui: &mut egui::Ui, preview: &mut FlipPreviewState) {
    let final_frame_index = preview.final_frame_index();
    let has_anims = preview.has_enabled_anim();

    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.label("Speed");
            ui.add(
                egui::DragValue::new(&mut preview.playback_speed)
                    .min_decimals(2)
                    .speed(0.01)
                    .range(0.25..=2.0),
            );
            ui.checkbox(&mut preview.should_loop, "Loop");
            if ui
                .checkbox(&mut preview.whole_frames, "Whole frames")
                .on_hover_text(
                    "Play only whole frames instead of interpolating between them. Useful for animations with hard cuts.",
                )
                .changed()
                && preview.whole_frames
            {
                preview.current_frame = preview.current_frame.floor();
                preview.whole_frame_remainder = 0.0;
                preview.apply_pending = true;
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 220.0).max(0.0);
            let step = if preview.is_playing && !preview.whole_frames {
                0.0
            } else {
                1.0
            };
            let response = ui.add_enabled(
                has_anims || final_frame_index > 0.0,
                egui::Slider::new(&mut preview.current_frame, 0.0..=final_frame_index.max(1.0))
                    .step_by(step)
                    .show_value(false),
            );
            if response.hovered() {
                ui.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                        preview.current_frame = (preview.current_frame - 1.0).ceil().max(0.0);
                        preview.apply_pending = true;
                    } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                        preview.current_frame =
                            (preview.current_frame + 1.0).floor().min(final_frame_index);
                        preview.apply_pending = true;
                    } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowLeft) {
                        preview.current_frame = 0.0;
                        preview.apply_pending = true;
                    } else if i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowRight) {
                        preview.current_frame = final_frame_index;
                        preview.apply_pending = true;
                    } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                        preview.playback_speed += 0.01;
                    } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                        preview.playback_speed -= 0.01;
                    }
                });
            }
            if response.changed() {
                preview.apply_pending = true;
            }

            let size = [60.0, 30.0];
            if preview.is_playing {
                if ui.add_sized(size, egui::Button::new("Pause")).clicked() {
                    preview.is_playing = false;
                    preview.last_frame_time = None;
                }
            } else if ui
                .add_enabled(has_anims, egui::Button::new("Play").min_size(size.into()))
                .clicked()
            {
                preview.is_playing = true;
                preview.last_frame_time = None;
            }

            let mut frame_value = egui::DragValue::new(&mut preview.current_frame)
                .range(0.0..=final_frame_index.max(0.0));
            if preview.whole_frames {
                frame_value = frame_value.min_decimals(0).max_decimals(0).speed(1.0);
            }
            if ui.add_sized([60.0, 20.0], frame_value).changed() {
                preview.apply_pending = true;
            }
            ui.label(format!("/ {final_frame_index}"));
        });
    });
}

fn folder_display_name(folder: &AnimFolder) -> String {
    folder
        .path
        .components()
        .rev()
        .take(4)
        .fold(std::path::PathBuf::new(), |acc, part| {
            std::path::Path::new(&part).join(acc)
        })
        .to_string_lossy()
        .to_string()
}

fn group_type_name(group_type: ssbh_data::anim_data::GroupType) -> &'static str {
    match group_type {
        ssbh_data::anim_data::GroupType::Transform => "Transform",
        ssbh_data::anim_data::GroupType::Visibility => "Visibility",
        ssbh_data::anim_data::GroupType::Material => "Material",
        ssbh_data::anim_data::GroupType::Camera => "Camera",
    }
}

fn show_anim_list(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    current_file_path: Option<&Path>,
    status_message: &mut String,
) {
    let motion_path = current_file_path.and_then(suggested_motion_folder);
    let motion_added = motion_path.as_ref().is_some_and(|path| {
        preview
            .anim_folders
            .iter()
            .any(|folder| folder.path == *path)
    });

    if preview.anim_folders.is_empty() {
        ui.label(
            "No matching animations found for this folder. Add the matching animation folder.",
        );
        if let Some(path) = &motion_path {
            if path.exists()
                && ui
                    .button("Add Motion Folder")
                    .on_hover_text(path.display().to_string())
                    .clicked()
            {
                preview.load_anim_folder(path.clone());
                *status_message = format!("Added {}", path.display());
            }
        }
        if ui.button("Add Folder to Workspace…").clicked() {
            let mut dialog = FileDialog::new();
            if let Some(path) = &motion_path {
                dialog = dialog.set_directory(path);
            }
            if let Some(folder) = dialog.pick_folder() {
                let display = folder.display().to_string();
                preview.load_anim_folder(folder);
                *status_message = format!("Added {display}");
            }
        }
        return;
    }

    if ui.button("Add Slot").clicked() {
        preview.anim_slots.push(AnimSlot::new());
    }

    let mut slot_to_remove = None;
    let mut update = false;
    let slot_count = preview.anim_slots.len();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("anim_slot_scroll")
        .show(ui, |ui| {
            for slot in (0..slot_count).rev() {
                update |= show_anim_slot(ui, preview, slot, &mut slot_to_remove);
            }
        });
    if let Some(slot) = slot_to_remove {
        if slot < preview.anim_slots.len() {
            preview.anim_slots.remove(slot);
            update = true;
        }
    }
    if update {
        preview.apply_pending = true;
    }

    if !motion_added {
        if let Some(path) = &motion_path {
            if path.exists()
                && ui
                    .button("Add Motion Folder")
                    .on_hover_text(path.display().to_string())
                    .clicked()
            {
                preview.load_anim_folder(path.clone());
                *status_message = format!("Added {}", path.display());
            }
        }
    }
    if ui.button("Add Folder to Workspace…").clicked() {
        let mut dialog = FileDialog::new();
        if let Some(path) = &motion_path {
            dialog = dialog.set_directory(path);
        } else if let Some(parent) = current_file_path.and_then(|p| p.parent()) {
            dialog = dialog.set_directory(parent);
        }
        if let Some(folder) = dialog.pick_folder() {
            let display = folder.display().to_string();
            preview.load_anim_folder(folder);
            *status_message = format!("Added {display}");
        }
    }
}

fn show_anim_slot(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    slot: usize,
    slot_to_remove: &mut Option<usize>,
) -> bool {
    let mut update = false;
    let id = ui.make_persistent_id("anim_slot").with(slot);
    let name = preview.anim_slots[slot]
        .animation
        .and_then(|index| preview.anim_at(index))
        .map(|(name, _)| name.as_str())
        .unwrap_or("Select an animation...")
        .to_string();

    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut preview.anim_slots[slot].is_enabled,
                        format!("Slot {slot}"),
                    )
                    .changed()
                {
                    update = true;
                }
                if anim_combo_box(ui, preview, slot, id.with("anim"), &name) {
                    update = true;
                }
                if ui.button("Remove").clicked() {
                    *slot_to_remove = Some(slot);
                }
            });
        })
        .body(|ui| {
            if let Some(index) = preview.anim_slots[slot].animation {
                if let Some((_, anim)) = preview.anim_at(index) {
                    for group in &anim.groups {
                        egui::CollapsingHeader::new(group_type_name(group.group_type))
                            .id_salt(id.with(group_type_name(group.group_type)))
                            .default_open(false)
                            .show(ui, |ui| {
                                for node in &group.nodes {
                                    match node.tracks.as_slice() {
                                        [_] => {
                                            ui.label(&node.name);
                                        }
                                        _ => {
                                            egui::CollapsingHeader::new(&node.name)
                                                .default_open(true)
                                                .show(ui, |ui| {
                                                    for track in &node.tracks {
                                                        ui.label(&track.name);
                                                    }
                                                });
                                        }
                                    }
                                }
                            });
                    }
                }
            }
        });
    update
}

fn anim_combo_box(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    slot: usize,
    id: egui::Id,
    name: &str,
) -> bool {
    let mut changed = false;
    let all_animations = preview.all_anim_indices();
    let combo_width = 200.0;
    let (button_rect, combo_response) = ui.allocate_exact_size(
        egui::vec2(combo_width, ui.spacing().interact_size.y),
        egui::Sense::click(),
    );
    let popup_id = id.with("popup");
    let open_id = id.with("popup_open");
    let mut is_open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(open_id).unwrap_or(false));

    if ui.is_rect_visible(button_rect) {
        let visuals = if is_open {
            &ui.visuals().widgets.open
        } else {
            ui.style().interact(&combo_response)
        };
        ui.painter().rect(
            button_rect.expand(visuals.expansion),
            visuals.corner_radius,
            visuals.weak_bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );

        let icon_size = egui::Vec2::splat(ui.spacing().icon_width);
        let icon_rect = egui::Align2::RIGHT_CENTER.align_size_within_rect(
            icon_size,
            button_rect.shrink2(ui.spacing().button_padding),
        );
        let icon_rect = egui::Rect::from_center_size(
            icon_rect.center(),
            egui::vec2(icon_rect.width() * 0.7, icon_rect.height() * 0.45),
        );
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                icon_rect.left_top(),
                icon_rect.right_top(),
                icon_rect.center_bottom(),
            ],
            visuals.fg_stroke.color,
            egui::Stroke::NONE,
        ));

        let text_galley = egui::WidgetText::from(name).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (icon_rect.left() - button_rect.left() - ui.spacing().button_padding.x * 2.0).max(8.0),
            egui::TextStyle::Button,
        );
        let text_pos = egui::Align2::LEFT_CENTER.align_size_within_rect(
            text_galley.size(),
            button_rect.shrink2(ui.spacing().button_padding),
        );
        ui.painter()
            .galley(text_pos.min, text_galley, visuals.text_color());
    }

    if combo_response.clicked() {
        is_open = !is_open;
    }

    let popup_height = (ui.ctx().viewport_rect().bottom() - button_rect.bottom() - 8.0).max(200.0);
    if is_open {
        let popup_response = egui::Area::new(popup_id)
            .kind(egui::UiKind::Menu)
            .order(egui::Order::Foreground)
            .fixed_pos(button_rect.left_bottom())
            .default_width(combo_width)
            .constrain(true)
            .movable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(combo_width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    egui::ScrollArea::vertical()
                        .max_height(popup_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for folder_index in (0..preview.anim_folders.len()).rev() {
                                let header_id =
                                    ui.make_persistent_id(id.with(("folder", folder_index)));
                                let title = folder_display_name(&preview.anim_folders[folder_index]);
                                let anim_count = preview.anim_folders[folder_index].anims.len();
                                egui::CollapsingHeader::new(title)
                                    .id_salt(header_id)
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for anim_index in 0..anim_count {
                                            let available = AnimIndex {
                                                folder_index,
                                                anim_index,
                                            };
                                            let label = preview.anim_folders[folder_index].anims
                                                [anim_index]
                                                .0
                                                .clone();
                                            let selected =
                                                preview.anim_slots[slot].animation == Some(available);
                                            if ui.selectable_label(selected, label).clicked() {
                                                preview.anim_slots[slot].animation = Some(available);
                                                preview.current_frame = 0.0;
                                                changed = true;
                                                is_open = false;
                                            }
                                        }
                                    });
                            }
                        });
                });
            });

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            is_open = false;
        } else if ui.input(|i| i.pointer.any_click()) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                if !button_rect.contains(pos) && !popup_response.response.rect.contains(pos) {
                    is_open = false;
                }
            }
        }
    }

    ui.ctx().data_mut(|d| d.insert_temp(open_id, is_open));

    if combo_response.hovered() {
        let step = ui.ctx().input(|i| {
            for e in i.events.iter().rev() {
                if let egui::Event::MouseWheel { delta, .. } = e {
                    return delta.y.signum() as i32;
                }
            }
            0
        });
        if step != 0 && !all_animations.is_empty() {
            let current_index = preview.anim_slots[slot]
                .animation
                .and_then(|current| {
                    all_animations.iter().position(|anim| {
                        anim.folder_index == current.folder_index
                            && anim.anim_index == current.anim_index
                    })
                });
            let new_index = match current_index {
                Some(current_idx) if step > 0 => {
                    if current_idx > 0 {
                        current_idx - 1
                    } else {
                        all_animations.len() - 1
                    }
                }
                Some(current_idx) => {
                    if current_idx + 1 < all_animations.len() {
                        current_idx + 1
                    } else {
                        0
                    }
                }
                None if step > 0 => all_animations.len() - 1,
                None => 0,
            };
            if current_index != Some(new_index) {
                preview.anim_slots[slot].animation = Some(all_animations[new_index]);
                preview.current_frame = 0.0;
                changed = true;
            }
        }
    }

    changed
}

fn show_mesh_visibility(
    ui: &mut egui::Ui,
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    status_message: &mut String,
) {
    ui.heading("Meshes");
    ui.label(format!("{} objects", preview.mesh_entries.len()));
    let picking_mesh = match &preview.add_wizard {
        AddEntryWizard::PickLhs { category, .. }
        | AddEntryWizard::PickRhs { category, .. }
            if FlipListKind::for_category(category) == FlipListKind::MeshPairs =>
        {
            true
        }
        _ => false,
    };
    if picking_mesh {
        ui.colored_label(egui::Color32::LIGHT_BLUE, "Click Use to pick a mesh for the new entry.");
    }
    ui.horizontal(|ui| {
        if ui.button("Show all").clicked() {
            preview.mesh_vis_command = MeshVisCommand::ShowAll;
        }
        if ui.button("Hide extras").clicked() {
            preview.mesh_vis_command = MeshVisCommand::HideExpressions;
        }
    });
    ui.separator();
    let mut picked_mesh = None;
    let mut scrolled_inspect = false;
    let inspected_mesh = preview.inspected_mesh.clone();
    let should_scroll_mesh = preview.scroll_to_inspected_mesh;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("mesh_visibility_scroll")
        .show(ui, |ui| {
            let mut changed = false;
            for (name, sub, visible) in preview.mesh_entries.iter_mut() {
                let inspected = inspected_mesh
                    .as_ref()
                    .is_some_and(|(n, s)| n == name && *s == *sub);
                let label = if *sub == 0 {
                    name.clone()
                } else {
                    format!("{name} [{sub}]")
                };
                let text = if inspected {
                    egui::RichText::new(label)
                        .color(egui::Color32::from_rgb(80, 180, 255))
                        .strong()
                } else {
                    egui::RichText::new(label)
                };
                ui.horizontal(|ui| {
                    let response = ui.checkbox(visible, text);
                    if inspected && should_scroll_mesh {
                        response.scroll_to_me(Some(egui::Align::Center));
                        scrolled_inspect = true;
                    }
                    if response.changed() {
                        changed = true;
                    }
                    if picking_mesh && ui.small_button("Use").clicked() {
                        picked_mesh = Some(name.clone());
                    }
                });
            }
            if changed {
                preview.mesh_vis_command = MeshVisCommand::ApplyList;
            }
        });
    if scrolled_inspect {
        preview.scroll_to_inspected_mesh = false;
    }
    if let Some(name) = picked_mesh {
        apply_wizard_mesh_pick(preview, param_file, name, status_message);
    }
}

fn apply_wizard_mesh_pick(
    preview: &mut FlipPreviewState,
    param_file: &mut ParamFile,
    name: String,
    status_message: &mut String,
) {
    match preview.add_wizard.clone() {
        AddEntryWizard::PickLhs { category, .. } => {
            finish_or_pick_rhs(preview, param_file, &category, name, status_message);
        }
        AddEntryWizard::PickRhs { category, lhs, .. } => {
            create_new_entry(preview, param_file, category, lhs, Some(name), status_message);
        }
        _ => {}
    }
}
