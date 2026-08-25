use crate::flip_ui::{show_flip_preview, show_tab_bar};
use crate::hash_crack::{self, CrackHit};
use crate::param_file::ParamFile;
use crate::param_types::*;
use crate::update::{self, LatestReleaseInfo, UpdateDownload};
use crate::viewport::{FlipPreviewState, MainTab};
use eframe::egui;
use rfd::FileDialog;
use std::collections::{HashSet, HashMap};
use std::path::{Path, PathBuf};
use std::alloc::GlobalAlloc;
use std::thread::JoinHandle;

// Debug logging
#[cfg(debug_assertions)]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        println!("[DEBUG] {}", format!($($arg)*));
    };
}

#[cfg(not(debug_assertions))]
macro_rules! debug_log {
    ($($arg:tt)*) => {};
}

// Simple debug logging without timing for now

// Performance configuration
const MAX_WORKER_THREADS: usize = 16; // Use more CPU cores
const CHUNK_SIZE: usize = 1000; // Process data in larger chunks
const MEMORY_BUFFER_SIZE: usize = 1024 * 1024; // 1MB buffer for operations
const VIRTUAL_SCROLLING_ENABLED: bool = true; // Enable virtual scrolling
const MAX_VISIBLE_ITEMS: usize = 100; // Maximum items to render at once
const TREE_DEPTH_LIMIT: usize = 8; // Maximum tree depth to render

/// Result of an autocomplete-enabled text input interaction for a single frame.
#[derive(Default)]
struct AcResult {
    committed: bool, // User accepted the value (pressed Enter or clicked away)
    cancelled: bool, // User pressed Escape to cancel editing
}

pub struct PrcEditorApp {
    param_file: ParamFile,
    selected_node: Option<String>, // Path to selected node
    previous_selected_node: Option<String>, // Track previous selection for cleanup
    expanded_nodes: HashSet<String>, // Set of expanded node paths
    status_message: String,
    tree_width: f32,
    show_label_editor: bool,
    label_editor_filter: String,
    editing_value: Option<(String, String)>, // (node_path, current_edit_value)
    request_edit_focus: bool, // Request focus on the editing text box on the next frame it appears
    new_label_input: String, // For adding new labels
    new_hash_input: String, // For adding labels to existing hashes
    hash_guess_input: String, // Try a name against hashed values in the open file
    label_page: usize, // Current page in label editor
    labels_per_page: usize, // Number of labels per page
    label_list_cache: Vec<(u64, String)>, // Sorted, filtered labels for the editor
    label_list_cache_key: Option<String>, // Filter string the cache was built from
    clipboard: Option<String>, // Copied node path
    clipboard_data: Option<ParamNode>, // Actual copied node data
    cut_mode: bool, // Whether the clipboard operation was cut (vs copy)
    // show_shortcuts_help removed - shortcuts are now always visible
    current_file_path: Option<PathBuf>, // Full path of the currently open param file (for in-place save)
    param_labels_path: Option<String>, // Path to the ParamLabels.csv file
    tree_items: Vec<String>, // Flattened list of visible tree items for navigation
    selected_index: Option<usize>, // Index in tree_items for keyboard navigation
    undo_stack: Vec<UndoAction>, // Stack of undo actions
    redo_stack: Vec<UndoAction>, // Stack of redo actions
    tree_items_dirty: bool, // Flag to track when tree items need rebuilding
    
    // Performance optimizations
    node_cache: HashMap<String, ParamNode>, // Cache for frequently accessed nodes
    last_tree_rebuild_frame: u64, // Track when tree was last rebuilt
    frame_count: u64, // Current frame counter
    cached_tree_items: Vec<String>, // Cached tree items to avoid rebuilding
    tree_rebuild_cooldown: u64, // Minimum frames between tree rebuilds
    
    // Autocomplete state
    autocomplete_suggestions: Vec<String>, // Current filtered suggestions
    autocomplete_selected_index: Option<usize>, // Currently selected suggestion in dropdown
    autocomplete_active: bool, // Whether autocomplete dropdown is currently showing
    autocomplete_context_id: String, // ID to track which text input is showing autocomplete
    
    // Performance settings
    enable_virtual_scrolling: bool, // Enable virtual scrolling for large trees
    enable_node_caching: bool, // Enable node caching
    max_tree_depth: usize, // Maximum tree depth to render
    max_visible_items: usize, // Maximum visible items in lists
    flip_preview: FlipPreviewState,
    crack_job: Option<JoinHandle<CrackJobResult>>,
    release_info: LatestReleaseInfo,
    update_download: UpdateDownload,
    update_status_message: Option<String>,
    auto_download_updates: bool,
}

struct CrackJobResult {
    hits: Vec<CrackHit>,
    leftover: Vec<u64>,
    unknown_count: usize,
    model_name_count: usize,
}

#[derive(Clone)]
enum UndoAction {
    DeleteNode {
        path: String,
        node: ParamNode,
        parent_path: String,
        index: usize,
    },
    AddNode {
        path: String,
    },
    UpdateValue {
        path: String,
        old_value: ParamValue,
        new_value: ParamValue,
    },
    UpdateKey {
        path: String,
        old_name: String,
        old_hash: u64,
        new_name: String,
        new_hash: u64,
    },
}

impl PrcEditorApp {
    pub fn new() -> Self {
        // Configure thread pool for maximum performance
        rayon::ThreadPoolBuilder::new()
            .num_threads(MAX_WORKER_THREADS)
            .stack_size(8 * 1024 * 1024) // 8MB stack per thread
            .build_global()
            .unwrap_or_else(|_| { debug_log!("Failed to configure thread pool, using default"); });
        
        let mut app = Self {
            param_file: ParamFile::new(),
            selected_node: None,
            previous_selected_node: None,
            expanded_nodes: HashSet::new(),
            status_message: "Ready".to_string(),
            tree_width: 300.0,
            show_label_editor: false,
            label_editor_filter: String::new(),
            editing_value: None,
            request_edit_focus: false,
            new_label_input: String::new(),
            new_hash_input: String::new(),
            hash_guess_input: String::new(),
            label_page: 1,
            labels_per_page: 10,
            label_list_cache: Vec::new(),
            label_list_cache_key: None,
            clipboard: None,
            clipboard_data: None,
            cut_mode: false,
            current_file_path: None,
            param_labels_path: None,
            tree_items: Vec::new(),
            selected_index: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            tree_items_dirty: true,
            autocomplete_suggestions: Vec::new(),
            autocomplete_selected_index: None,
            autocomplete_active: false,
            autocomplete_context_id: String::new(),
            // Performance optimizations
            node_cache: HashMap::new(),
            last_tree_rebuild_frame: 0,
            frame_count: 0,
            cached_tree_items: Vec::new(),
            tree_rebuild_cooldown: 5, // Rebuild tree at most every 5 frames
            
            // Performance settings
            enable_virtual_scrolling: true,
            enable_node_caching: true,
            max_tree_depth: 8,
            max_visible_items: 100,
            flip_preview: FlipPreviewState::default(),
            crack_job: None,
            release_info: update::check_for_updates(),
            update_download: UpdateDownload::default(),
            update_status_message: None,
            auto_download_updates: update::load_auto_download_updates(),
        };
        
        // Load ParamLabels.csv from AppData\Roaming\Smash Ultimate Labels, downloading it if needed.
        app.ensure_param_labels();
        
        app
    }

    const PARAM_LABELS_URL: &'static str =
        "https://raw.githubusercontent.com/CrusherD2/param-labels/master/ParamLabels.csv";

    fn smash_ultimate_labels_dir() -> Option<PathBuf> {
        let mut dir = dirs::config_dir()?;
        dir.push("Smash Ultimate Labels");
        Some(dir)
    }

    fn default_param_labels_path() -> Option<PathBuf> {
        let mut path = Self::smash_ultimate_labels_dir()?;
        path.push("ParamLabels.csv");
        Some(path)
    }

    fn ensure_param_labels(&mut self) {
        if let Some(path) = Self::default_param_labels_path() {
            if path.exists() {
                if self.load_labels_file(&path) {
                    return;
                }
            } else if self.download_param_labels_file(&path) {
                return;
            }
        }

        if let Some(saved) = self.load_saved_labels_path() {
            let saved_path = PathBuf::from(&saved);
            if saved_path.exists() && self.load_labels_file(&saved_path) {
                return;
            }
        }

        self.status_message =
            "ParamLabels.csv could not be loaded. Use Labels > Download or Load Labels...".to_string();
    }

    fn load_labels_file(&mut self, path: &Path) -> bool {
        match std::fs::read_to_string(path) {
            Ok(content) if !content.trim().is_empty() => {
                let path_string = path.to_string_lossy().to_string();
                self.param_labels_path = Some(path_string.clone());
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("ParamLabels.csv");
                self.load_labels_from_content(&content, file_name);
                self.save_labels_path(&path_string);
                true
            }
            _ => false,
        }
    }

    fn download_param_labels_file(&mut self, path: &Path) -> bool {
        self.status_message = format!("Downloading ParamLabels.csv to {}…", path.display());
        match Self::fetch_param_labels_csv(path) {
            Ok(()) => self.load_labels_file(path),
            Err(e) => {
                self.status_message = e;
                false
            }
        }
    }

    fn fetch_param_labels_csv(path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
        }
        let response = ureq::get(Self::PARAM_LABELS_URL)
            .timeout(std::time::Duration::from_secs(60))
            .call()
            .map_err(|e| format!("Download failed: {e}"))?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes)
            .map_err(|e| format!("Download failed: {e}"))?;
        if bytes.is_empty() {
            return Err("Downloaded ParamLabels.csv was empty.".to_string());
        }
        std::fs::write(path, bytes)
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(())
    }

    fn load_param_labels(&mut self) {
        self.ensure_param_labels();
        if self.param_labels_path.is_none() {
            self.prompt_for_labels_file();
        }
    }
    
    fn load_labels_from_content(&mut self, csv_content: &str, file_path: &str) {
        match self.param_file.hash_labels.load_from_csv(csv_content) {
            Ok(count) => {
                self.status_message = format!("Loaded {} param labels from {}", count, file_path);
                // Rebuild the tree to apply the new labels to field names
                self.param_file.rebuild_tree_with_labels();
                self.mark_tree_dirty(); // Mark tree items as dirty when labels are loaded
            }
            Err(e) => {
                self.status_message = format!("Error loading labels from {}: {}", file_path, e);
            }
        }
    }
    
    fn prompt_for_labels_file(&mut self) {
        let mut dialog = FileDialog::new()
            .add_filter("CSV files", &["csv"])
            .add_filter("All files", &["*"])
            .set_title("Select ParamLabels.csv file")
            .set_file_name("ParamLabels.csv");

        if let Some(labels_dir) = Self::smash_ultimate_labels_dir() {
            let _ = std::fs::create_dir_all(&labels_dir);
            dialog = dialog.set_directory(&labels_dir);
        }

        if let Some(file_path) = dialog.pick_file() {
            match std::fs::read_to_string(&file_path) {
                Ok(csv_content) => {
                    // Store the full path for future read/write operations
                    let path_string = file_path.to_string_lossy().to_string();
                    self.param_labels_path = Some(path_string.clone());
                    let file_name = file_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("selected file");
                    self.load_labels_from_content(&csv_content, file_name);
                    
                    // Save this path for next time
                    self.save_labels_path(&path_string);
                }
                Err(e) => {
                    self.status_message = format!("Error reading selected file: {}", e);
                    // Keep prompting until a valid file is selected
                    self.prompt_for_labels_file();
                }
            }
        } else {
            // Make ParamLabels.csv required - keep prompting until selected
            self.status_message = "ParamLabels.csv is required to use this editor. Please select a valid file.".to_string();
            // We could add a delay here or show a more prominent dialog
        }
    }

    fn show_menu_bar(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                let has_labels = self.param_labels_path.is_some();
                let open_button = ui.add_enabled(has_labels, egui::Button::new("Open"));
                if open_button.clicked() {
                    self.open_file_dialog();
                    ui.close();
                }
                if !has_labels && open_button.hovered() {
                    open_button.on_hover_text("Load ParamLabels.csv first");
                }
                
                ui.separator();
                
                let has_file = self.param_file.get_root().is_some();
                if ui.add_enabled(has_file, egui::Button::new("Save")).clicked() {
                    self.save_file();
                    ui.close();
                }
                
                if ui.add_enabled(has_file, egui::Button::new("Save As...")).clicked() {
                    self.save_file_dialog();
                    ui.close();
                }
            });

            ui.menu_button("Labels", |ui| {
                if ui.button("Load Labels...").clicked() {
                    self.prompt_for_labels_file();
                    ui.close();
                }
                
                if ui.button("Change Location...").clicked() {
                    self.prompt_for_labels_file();
                    ui.close();
                }
                
                ui.separator();
                
                // Show current labels file path
                if let Some(path) = &self.param_labels_path {
                    let filename = Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    ui.label(format!("Current: {}", filename));
                    if ui.small_button("📁").on_hover_text("Show full path").clicked() {
                        self.status_message = format!("ParamLabels.csv location: {}", path);
                    }
                } else {
                    ui.label("No labels file loaded");
                }
                
                ui.separator();
                
                if ui.button("Edit").clicked() {
                    self.show_label_editor = true;
                    self.label_page = 1;
                    self.invalidate_label_list_cache();
                    ui.close();
                }
                
                if ui.button("Save").clicked() {
                    if let Some(path) = &self.param_labels_path {
                        match self.param_file.hash_labels.save_to_csv(path) {
                            Ok(0) => {
                                self.status_message = format!(
                                    "No new labels to append; existing ParamLabels.csv was left unchanged ({path})"
                                );
                            }
                            Ok(n) => {
                                self.status_message = format!(
                                    "Appended {n} new labels to the end of {path}"
                                );
                            }
                            Err(e) => {
                                self.status_message = format!("Error saving labels: {}", e);
                            }
                        }
                    } else {
                        self.status_message = "No labels file path set - use 'Load Labels...' first".to_string();
                    }
                    ui.close();
                }
                
                if ui.button("Download").clicked() {
                    self.download_labels();
                    ui.close();
                }

                let has_file = self.param_file.get_root().is_some();
                if ui
                    .add_enabled(
                        has_file && self.crack_job.is_none(),
                        egui::Button::new("Crack hashes in this file"),
                    )
                    .on_hover_text("Test ParamLabels words, the loaded model names, and short brute-force against unresolved 0x hashes")
                    .clicked()
                {
                    self.crack_open_file();
                    ui.close();
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Check for Updates").clicked() {
                    let info = update::check_for_updates_now();
                    update::apply_manual_update_check(
                        info,
                        &mut self.release_info,
                        &self.update_download,
                        self.auto_download_updates,
                        &mut self.update_status_message,
                    );
                    ui.close();
                }

                if ui
                    .checkbox(
                        &mut self.auto_download_updates,
                        "Automatically Download Updates",
                    )
                    .on_hover_text(
                        "When a new version is available, download it next to this program. Close PRC Editor and run the new file to update.",
                    )
                    .changed()
                {
                    update::save_auto_download_updates(self.auto_download_updates);
                }

                ui.separator();
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                if ui.button("GitHub Releases").clicked() {
                    update::open_releases_page();
                    ui.close();
                }
                if ui.button("Report Issue").clicked() {
                    update::open_issues_page();
                    ui.close();
                }
            });
        });
    }

    fn show_main_content(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("parameter_tree")
            .resizable(true)
            .default_size(self.tree_width)
            .min_size(200.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Parameter Tree");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Add a child to the selected category (struct field or list item)
                        if let Some(selected_path) = self.selected_node.clone() {
                            if let Some(selected_node) = self.find_node_by_path(&selected_path) {
                                if selected_node.is_expandable() {
                                    let button_text = match &selected_node.value {
                                        ParamValue::Struct(_) => "+ Add Field",
                                        ParamValue::List(_) => "+ Add Item",
                                        _ => "",
                                    };
                                    if !button_text.is_empty() && ui.button(button_text).clicked() {
                                        self.add_child_to_path(&selected_path);
                                    }
                                }
                            }
                        }
                    });
                });
                ui.separator();
                
                // Build children for expanded nodes (lazy loading for performance)
                if self.param_file.get_root().is_some() {
                    self.ensure_expanded_children_built();
                }
                
                // Smart tree rebuilding - only rebuild when necessary and with cooldown
                if self.param_file.get_root().is_some() && self.should_rebuild_tree() {
                    self.build_tree_items();
                    self.last_tree_rebuild_frame = self.frame_count;
                    self.tree_items_dirty = false;
                }
                
                // Make the scroll area use all available space
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])  // Don't shrink in either direction
                    .show(ui, |ui| {
                    if self.param_labels_path.is_none() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(20.0);
                            ui.colored_label(egui::Color32::YELLOW, "⚠ ParamLabels.csv Required");
                            ui.add_space(10.0);
                            ui.label("This editor requires ParamLabels.csv to function properly.");
                            ui.add_space(5.0);
                            if ui.button("Download ParamLabels.csv").clicked() {
                                self.ensure_param_labels();
                            }
                            if ui.button("Load from file...").clicked() {
                                self.prompt_for_labels_file();
                            }
                        });
                    } else if self.param_file.get_root().is_some() {
                        // Need to avoid borrowing conflict - get the root again inside show_tree_node
                        self.show_tree_root(ui);
                    } else if self.status_message.contains("Error") {
                        ui.colored_label(egui::Color32::LIGHT_RED, "Failed to parse file");
                        ui.label("Check console for details");
                        if ui.button("Try another file").clicked() {
                            self.open_file_dialog();
                        }
                    } else {
                        ui.label("No file loaded");
                        if ui.button("Open file").clicked() {
                            self.open_file_dialog();
                        }
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Parameter Details");
            ui.separator();
            
            // Main content area with shortcuts overlay
            
            if let Some(selected_path) = self.selected_node.clone() {
                self.show_parameter_details(ui, &selected_path);
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.label("Select a parameter to view details");
                });
            }
            
            // Add shortcuts box as overlay in absolute bottom-right corner
            let shortcuts_box_width = 280.0;
            let shortcuts_box_height = 200.0;
            
            // Use the UI's clip rect to get the actual drawable area and move closer to corner
            let clip_rect = ui.clip_rect();
            let shortcuts_pos = egui::pos2(
                clip_rect.max.x - shortcuts_box_width - 5.0, // Move 5 pixels away from right edge (inward)
                clip_rect.max.y - shortcuts_box_height - 5.0 // Move 5 pixels away from bottom edge (inward)
            );
            
            // Draw the shortcuts box as overlay (non-interactive background element)
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(shortcuts_pos, egui::vec2(shortcuts_box_width, shortcuts_box_height))),
                |ui| {
                    // Background frame
                    let frame = egui::Frame::default()
                        .fill(egui::Color32::from_rgba_unmultiplied(40, 40, 40, 200))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(80, 80, 80, 150)))
                        .corner_radius(8.0)
                        .inner_margin(12.0);
                    
                    frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgba_unmultiplied(200, 200, 200, 255),
                                egui::RichText::new("Keyboard Shortcuts").size(14.0).strong()
                            );
                            ui.add_space(8.0);
                            
                            let shortcuts = [
                                ("↑↓←→", "Navigate tree"),
                                ("Enter", "Expand/collapse"),
                                ("F2", "Rename node"),
                                ("Del", "Delete node"),
                                ("Ctrl+C", "Copy node"),
                                ("Ctrl+X", "Cut node"),
                                ("Ctrl+V", "Paste node"),
                                ("Ctrl+P", "Paste to parent"),
                                ("Ctrl+D", "Duplicate node"),
                                ("Ctrl+S", "Save file"),
                                ("Ctrl+Z", "Undo"),
                                ("Ctrl+Y", "Redo"),
                            ];
                            
                            for (key, desc) in shortcuts {
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        egui::Color32::from_rgba_unmultiplied(150, 200, 255, 255),
                                        egui::RichText::new(key).size(11.0).monospace()
                                    );
                                    ui.colored_label(
                                        egui::Color32::from_rgba_unmultiplied(180, 180, 180, 255),
                                        egui::RichText::new(desc).size(11.0)
                                    );
                                });
                            }
                        });
                    });
                }
            );
        });
    }

    fn show_tree_root(&mut self, ui: &mut egui::Ui) {
        // Need to avoid borrowing conflicts, so use a workaround with cloning just the node structure
        // This is only for the root display, so performance impact is minimal
        if let Some(root) = self.param_file.get_root().cloned() {
            self.show_tree_node_virtual(ui, &root, "root".to_string(), 0);
        }
    }

    fn show_tree_node(&mut self, ui: &mut egui::Ui, node: &ParamNode, path: String) {
        let is_expanded = self.expanded_nodes.contains(&path);
        let is_selected = self.selected_node.as_ref() == Some(&path);
        let is_keyboard_selected = self.selected_index
            .and_then(|idx| self.tree_items.get(idx))
            .map(|selected_path| selected_path == &path)
            .unwrap_or(false);

        // Virtual scrolling: Only render if this node is likely to be visible
        if VIRTUAL_SCROLLING_ENABLED && node.children.len() > MAX_VISIBLE_ITEMS {
            // For large nodes, show a summary instead of all children
            let response = ui.horizontal(|ui| {
                let icon = if is_expanded { "▼" } else { "▶" };
                let _ = ui.button(icon);
                
                let type_icon = match &node.value {
                    ParamValue::Struct(_) => "📁",
                    ParamValue::List(_) => "📋",
                    _ => "📄",
                };
                ui.label(type_icon);
                
                let label = if node.name.is_empty() || node.name.starts_with("0x") {
                    format!("0x{:X}", node.hash)
                } else {
                    if node.name.len() > 25 {
                        format!("{}...", &node.name[..22])
                    } else {
                        node.name.clone()
                    }
                };
                
                let display_text = format!("{} ({} items)", label, node.children.len());
                let label_response = ui.selectable_label(is_selected || is_keyboard_selected, display_text);
                
                if is_keyboard_selected && !is_selected {
                    let rect = label_response.rect;
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, egui::Color32::YELLOW), egui::StrokeKind::Inside);
                }
                
                label_response
            }).inner;
            
            if response.clicked() {
                self.update_selection(path.clone());
            }
            
            // Show virtual scrolling interface for large nodes
            if is_expanded && node.is_expandable() {
                ui.indent(egui::Id::new(format!("{}_indent", path)), |ui| {
                    self.show_virtual_scroll_children(ui, node, &path);
                });
            }
            return;
        }

        // Create the tree node header
        let response = if node.is_expandable() {
            let icon = if is_expanded { "▼" } else { "▶" };
            ui.horizontal(|ui| {
                if ui.button(icon).clicked() {
                    if is_expanded {
                        self.expanded_nodes.remove(&path);
                    } else {
                        self.expanded_nodes.insert(path.clone());
                    }
                    self.mark_tree_dirty();
                }
                
                let type_icon = match &node.value {
                    ParamValue::Struct(_) => "📁",
                    ParamValue::List(_) => "📋",
                    _ => "📄",
                };
                
                ui.label(type_icon);
                
                let label = if node.name.is_empty() || node.name.starts_with("0x") {
                    format!("0x{:X}", node.hash)
                } else {
                    if node.name.len() > 25 {
                        format!("{}...", &node.name[..22])
                    } else {
                        node.name.clone()
                    }
                };
                
                let label_response = ui.selectable_label(is_selected || is_keyboard_selected, label);
                
                if is_keyboard_selected && !is_selected {
                    let rect = label_response.rect;
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, egui::Color32::YELLOW), egui::StrokeKind::Inside);
                }
                
                label_response
            }).inner
        } else {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label("📄");
                
                let label = if node.name.is_empty() || node.name.starts_with("0x") {
                    format!("0x{:X}", node.hash)
                } else {
                    if node.name.len() > 20 {
                        format!("{}...", &node.name[..17])
                    } else {
                        node.name.clone()
                    }
                };
                
                let display_text = format!("{} ({})", label, node.get_type_name());
                let label_response = ui.selectable_label(is_selected || is_keyboard_selected, display_text);
                
                if is_keyboard_selected && !is_selected {
                    let rect = label_response.rect;
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, egui::Color32::YELLOW), egui::StrokeKind::Inside);
                }
                
                label_response
            }).inner
        };

        if response.clicked() {
            self.update_selection(path.clone());
        }

        // Show children if expanded (with depth limit)
        if is_expanded && node.is_expandable() {
            ui.indent(egui::Id::new(format!("{}_indent", path)), |ui| {
                for (i, child) in node.children.iter().enumerate() {
                    let child_path = format!("{}[{}]", path, i);
                    self.show_tree_node(ui, child, child_path);
                }
            });
        }
    }

    fn show_parameter_details(&mut self, ui: &mut egui::Ui, selected_path: &str) {
        debug_log!("Showing parameter details for: {}", selected_path);
        
        // Performance optimization: Only show details for selected nodes
        if !self.selected_node.as_ref().map_or(false, |path| path == selected_path) {
            return;
        }
        
        // Extract the node information first to avoid borrowing conflicts
        let node_info = if let Some(node) = self.find_node_by_path(selected_path) {
            Some((
                node.name.clone(),
                node.hash,
                node.get_type_name(),
                node.get_value_string_with_labels(&self.param_file.hash_labels),
                matches!(node.value, ParamValue::Struct(_)),
                matches!(node.value, ParamValue::List(_)),
                if let ParamValue::Struct(s) = &node.value { s.fields.len() } else { 0 },
                if let ParamValue::List(l) = &node.value { l.values.len() } else { 0 },
                node.clone() // Clone the node for editor functions
            ))
        } else {
            None
        };

        if let Some((node_name, node_hash, node_type_name, node_value_string, is_struct, is_list, struct_field_count, list_item_count, node_clone)) = node_info {
            ui.heading(&format!("Parameter: {}", if node_name.is_empty() { format!("0x{:X}", node_hash) } else { node_name.clone() }));
            ui.separator();
            
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("param_details")
                    .num_columns(2)
                    .striped(true)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Name:");
                        
                        // Make name editable
                        let name_edit_path = format!("{}_name", selected_path);
                        let is_editing_name = self.editing_value.as_ref()
                            .map(|(path, _)| path == &name_edit_path)
                            .unwrap_or(false);
                        
                        if is_editing_name {
                            let mut edit_name = self.editing_value.as_ref().unwrap().1.clone();
                            let response = ui.text_edit_singleline(&mut edit_name);
                            
                            if self.request_edit_focus {
                                response.request_focus();
                                self.request_edit_focus = false;
                            }
                            
                            if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                // Get the current node name to check if it actually changed
                                let current_name = if let Some(current_node) = self.find_node_by_path(selected_path) {
                                    current_node.name.clone()
                                } else {
                                    String::new()
                                };
                                
                                // Only proceed if the name actually changed
                                if edit_name != current_name {
                                    // Check for duplicate names and add failsafe
                                    let final_name = self.ensure_unique_name(selected_path, &edit_name);
                                    
                                    // Generate hash for new name
                                    let new_hash = self.param_file.hash_labels.add_label_and_save(&final_name, self.param_labels_path.as_deref());
                                    
                                    // Update the node name and hash
                                    if self.update_node_key_with_undo(selected_path, final_name.clone(), new_hash) {
                                        let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                                        let message = if final_name != edit_name {
                                            format!("Node renamed to '{}' (was duplicate, added suffix) (hash: 0x{:X}) and saved to {}", final_name, new_hash, path_display)
                                        } else {
                                            format!("Node renamed to '{}' (hash: 0x{:X}) and saved to {}", final_name, new_hash, path_display)
                                        };
                                        self.status_message = message;
                                        // Rebuild tree to show updated name
                                        self.param_file.rebuild_tree_with_labels();
                                    } else {
                                        self.status_message = "Failed to update node name".to_string();
                                    }
                                } else {
                                    // Name didn't change, just show a message
                                    self.status_message = "Name unchanged".to_string();
                                }
                                self.editing_value = None;
                            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.editing_value = None;
                            } else {
                                self.editing_value = Some((name_edit_path.clone(), edit_name));
                            }
                        } else {
                            let response = ui.add(
                                egui::Label::new(egui::RichText::new(&node_name).strong())
                                    .sense(egui::Sense::click())
                            );
                            
                            if response.clicked() {
                                self.set_editing_value(Some((name_edit_path, node_name.clone())));
                            }
                            
                            if response.hovered() {
                                response.on_hover_text("Click to rename");
                            }
                        }
                        ui.end_row();
                        
                        ui.strong("Hash:");
                        ui.monospace(format!("0x{:X}", node_hash));
                        ui.end_row();
                        
                        ui.strong("Type:");
                        ui.label(node_type_name);
                        ui.end_row();
                        
                        ui.strong("Value:");
                        ui.monospace(node_value_string);
                        ui.end_row();
                        
                        if is_struct {
                            ui.strong("Fields:");
                            ui.label(format!("{} fields", struct_field_count));
                            ui.end_row();
                        } else if is_list {
                            ui.strong("Items:");
                            ui.label(format!("{} items", list_item_count));
                            ui.end_row();
                        }
                    });
                
                ui.add_space(10.0);
                
                // Show editing interface based on parameter type
                if is_struct {
                    self.show_struct_editor(ui, &node_clone, selected_path);
                    
                    // Add Bones section only when viewing the root node
                    if selected_path == "root" {
                        self.show_bones_section(ui, selected_path);
                    }
                } else if is_list {
                    self.show_list_editor(ui, &node_clone, selected_path);
                } else {
                    self.show_value_editor(ui, &node_clone, selected_path);
                }
            });
        } else {
            ui.label(format!("Could not find node at path: {}", selected_path));
        }
    }
    
    /// Check if this is a root-level parameter (direct child of root)
    fn is_root_level_parameter(&self, path: &str) -> bool {
        if let Some(root) = self.param_file.get_root() {
            for (i, _child) in root.children.iter().enumerate() {
                if format!("root[{}]", i) == path {
                    return true;
                }
            }
        }
        false
    }
    
    /// Show the Bones section that lists all bone names from children
    fn show_bones_section(&mut self, ui: &mut egui::Ui, selected_path: &str) {
        // Collect all bone names from children
        let bone_names = self.collect_bone_names_from_children(selected_path);
        
        if !bone_names.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.heading("Bones");
                ui.label(format!("({} unique bones)", bone_names.len()));
            });
            ui.add_space(5.0);
            
            let mut new_editing_value = self.editing_value.clone();
            let mut new_status_message = None;
            
            egui::ScrollArea::vertical().id_salt("bones_scroll_area").max_height(300.0).show(ui, |ui| {
                egui::Grid::new("bones_list")
                    .num_columns(3)
                    .striped(true)
                    .spacing([15.0, 6.0])
                    .min_col_width(120.0)
                    .show(ui, |ui| {
                        ui.strong("Bone Name");
                        ui.strong("Usage Count");
                        ui.strong("Actions");
                        ui.end_row();
                        
                        for bone_name in &bone_names {
                            // Count how many times this bone is used
                            let usage_count = self.count_bone_usage(selected_path, bone_name);
                            
                            // Bone name column - editable
                            let bone_edit_path = format!("{}_bone_{}", selected_path, bone_name);
                            let is_editing_bone = new_editing_value.as_ref()
                                .map(|(path, _)| path == &bone_edit_path)
                                .unwrap_or(false);
                            
                            if is_editing_bone {
                                let mut edit_bone = new_editing_value.as_ref().unwrap().1.clone();
                                let response = ui.text_edit_singleline(&mut edit_bone);
                                
                                if self.request_edit_focus {
                                    response.request_focus();
                                    self.request_edit_focus = false;
                                }
                                
                                if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    if edit_bone != *bone_name {
                                        // Rename this bone across all children
                                        if self.rename_bone_globally(selected_path, bone_name, &edit_bone) {
                                            let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                                            new_status_message = Some(format!("Renamed bone '{}' to '{}' across all children and saved to {}", bone_name, edit_bone, path_display));
                                            // Rebuild tree to show updated names
                                            self.param_file.rebuild_tree_with_labels();
                                        } else {
                                            new_status_message = Some("Failed to rename bone".to_string());
                                        }
                                    } else {
                                        new_status_message = Some("Bone name unchanged".to_string());
                                    }
                                    new_editing_value = None;
                                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    new_editing_value = None;
                                } else {
                                    new_editing_value = Some((bone_edit_path.clone(), edit_bone));
                                }
                            } else {
                                let response = ui.add(
                                    egui::Label::new(egui::RichText::new(bone_name).strong())
                                        .sense(egui::Sense::click())
                                );
                                
                                                            if response.clicked() {
                                new_editing_value = Some((bone_edit_path.clone(), bone_name.clone()));
                            }
                            
                            if response.hovered() {
                                response.on_hover_text("Click to rename bone globally");
                            }
                        }
                        
                        // Usage count column
                        ui.label(format!("{}", usage_count));
                        
                        // Actions column
                        ui.horizontal(|ui| {
                            if ui.small_button("✏").on_hover_text("Rename Bone").clicked() {
                                new_editing_value = Some((bone_edit_path.clone(), bone_name.clone()));
                            }
                            if ui.small_button("📋").on_hover_text("Duplicate Bone").clicked() {
                                if self.duplicate_bone_globally(selected_path, bone_name) {
                                    let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                                    new_status_message = Some(format!("Duplicated bone '{}' to all children and saved to {}", bone_name, path_display));
                                    // Rebuild tree to show updated bones
                                    self.param_file.rebuild_tree_with_labels();
                                } else {
                                    new_status_message = Some("Failed to duplicate bone".to_string());
                                }
                            }
                        });
                            
                            ui.end_row();
                        }
                    });
            });
            
            self.set_editing_value(new_editing_value);
            if let Some(msg) = new_status_message {
                self.status_message = msg;
            }
        }
    }
    
    /// Collect all unique bone names from children of the selected parameter
    fn collect_bone_names_from_children(&self, selected_path: &str) -> Vec<String> {
        let mut bone_names = std::collections::HashSet::new();
        
        if let Some(node) = self.find_node_by_path(selected_path) {
            if selected_path == "root" {
                // For root, collect bones from the entire file structure
                self.collect_bone_names_from_entire_file(node, &mut bone_names);
            } else {
                // Look for bones field in children
                for child in &node.children {
                    if child.name == "bones" {
                        // Collect bone names from the bones list
                        self.collect_bone_names_from_bones_field(child, &mut bone_names);
                    }
                }
            }
        }
        
        let mut result: Vec<String> = bone_names.into_iter().collect();
        result.sort(); // Sort alphabetically
        result
    }
    
    /// Collect bone names from the entire file structure
    fn collect_bone_names_from_entire_file(&self, node: &ParamNode, bone_names: &mut std::collections::HashSet<String>) {
        // Recursively search through all children
        for child in &node.children {
            // Look for bones field in this child
            for grandchild in &child.children {
                if grandchild.name == "bones" {
                    self.collect_bone_names_from_bones_field(grandchild, bone_names);
                }
            }
            // Recursively search deeper
            self.collect_bone_names_from_entire_file(child, bone_names);
        }
    }
    

    
    /// Collect bone names from a bones field
    fn collect_bone_names_from_bones_field(&self, bones_node: &ParamNode, bone_names: &mut std::collections::HashSet<String>) {
        match &bones_node.value {
            ParamValue::List(list) => {
                for item in &list.values {
                    if let ParamValue::Struct(struct_data) = item {
                        // Look for "name" field in the struct
                        for (hash, value) in &struct_data.fields {
                            let field_name = self.param_file.hash_labels.hash_to_string(*hash);
                            if field_name == "name" {
                                if let ParamValue::Hash(name_hash) = value {
                                    let bone_name = self.param_file.hash_labels.hash_to_string(*name_hash);
                                    if !bone_name.is_empty() && !bone_name.starts_with("0x") {
                                        bone_names.insert(bone_name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    
    /// Count how many times a bone is used across all children
    fn count_bone_usage(&self, selected_path: &str, bone_name: &str) -> usize {
        let mut count = 0;
        
        if let Some(node) = self.find_node_by_path(selected_path) {
            if selected_path == "root" {
                // For root, count bones from the entire file structure
                count = self.count_bone_usage_in_entire_file(node, bone_name);
            } else {
                for child in &node.children {
                    if child.name == "bones" {
                        count += self.count_bone_usage_in_bones_field(child, bone_name);
                    }
                }
            }
        }
        
        count
    }
    
    /// Count bone usage in the entire file structure
    fn count_bone_usage_in_entire_file(&self, node: &ParamNode, bone_name: &str) -> usize {
        let mut count = 0;
        
        // Recursively search through all children
        for child in &node.children {
            // Look for bones field in this child
            for grandchild in &child.children {
                if grandchild.name == "bones" {
                    count += self.count_bone_usage_in_bones_field(grandchild, bone_name);
                }
            }
            // Recursively search deeper
            count += self.count_bone_usage_in_entire_file(child, bone_name);
        }
        
        count
    }
    

    
    /// Count bone usage in a bones field
    fn count_bone_usage_in_bones_field(&self, bones_node: &ParamNode, bone_name: &str) -> usize {
        let mut count = 0;
        
        match &bones_node.value {
            ParamValue::List(list) => {
                for item in &list.values {
                    if let ParamValue::Struct(struct_data) = item {
                        // Look for "name" field in the struct
                        for (hash, value) in &struct_data.fields {
                            let field_name = self.param_file.hash_labels.hash_to_string(*hash);
                            if field_name == "name" {
                                if let ParamValue::Hash(name_hash) = value {
                                    let current_bone_name = self.param_file.hash_labels.hash_to_string(*name_hash);
                                    if current_bone_name == bone_name {
                                        count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        
        count
    }
    
    /// Rename a bone globally across the entire file structure
    fn rename_bone_globally(&mut self, _selected_path: &str, old_name: &str, new_name: &str) -> bool {
        // Generate hash for new bone name
        let new_hash = self.param_file.hash_labels.add_label_and_save(new_name, self.param_labels_path.as_deref());
        
        // Get the old hash for comparison
        let old_hash = self.param_file.hash_labels.string_to_hash40(old_name);
        
        // Update the entire underlying data structure
        if let Some(root) = &mut self.param_file.root {
            let updated = Self::update_bone_names_in_param_value_static(&mut root.value, old_hash, new_hash);
            if updated {
                // Rebuild the tree to reflect all changes
                self.param_file.rebuild_tree_with_labels();
            }
            updated
        } else {
            false
        }
    }
    
    /// Static version of update_bone_names_in_param_value to avoid borrowing conflicts
    fn update_bone_names_in_param_value_static(value: &mut ParamValue, old_hash: u64, new_hash: u64) -> bool {
        let mut updated = false;
        
        match value {
            ParamValue::List(list) => {
                for item in &mut list.values {
                    if let ParamValue::Struct(struct_data) = item {
                        // Look for "name" field in the struct
                        for (_hash, field_value) in &mut struct_data.fields {
                            if let ParamValue::Hash(name_hash) = field_value {
                                // Only update if the current hash matches the old hash
                                if *name_hash == old_hash {
                                    *name_hash = new_hash;
                                    updated = true;
                                }
                            }
                        }
                    }
                }
            }
            ParamValue::Struct(struct_data) => {
                for (_, field_value) in &mut struct_data.fields {
                    if Self::update_bone_names_in_param_value_static(field_value, old_hash, new_hash) {
                        updated = true;
                    }
                }
            }
            _ => {}
        }
        
        updated
    }
    
    /// Duplicate a bone globally across all children
    fn duplicate_bone_globally(&mut self, _selected_path: &str, bone_name: &str) -> bool {
        // Find the bone template to duplicate
        let bone_template = self.find_bone_template(bone_name);
        if bone_template.is_none() {
            return false;
        }
        
        // Duplicate the bone to all children that have bones
        if let Some(root) = &mut self.param_file.root {
            return Self::duplicate_bone_in_param_value_static(&mut root.value, bone_template.unwrap());
        }
        
        false
    }
    
    /// Find a bone template by name from the entire file structure
    fn find_bone_template(&self, bone_name: &str) -> Option<ParamValue> {
        if let Some(root) = self.param_file.get_root() {
            return Self::find_bone_template_static(&root.value, bone_name, &self.param_file.hash_labels);
        }
        None
    }
    
    /// Static version to find bone template
    fn find_bone_template_static(value: &ParamValue, bone_name: &str, hash_labels: &crate::hash_labels::HashLabels) -> Option<ParamValue> {
        match value {
            ParamValue::List(list) => {
                for item in &list.values {
                    if let ParamValue::Struct(struct_data) = item {
                        // Look for "name" field in the struct
                        for (hash, field_value) in &struct_data.fields {
                            let field_name = hash_labels.hash_to_string(*hash);
                            if field_name == "name" {
                                if let ParamValue::Hash(name_hash) = field_value {
                                    let current_bone_name = hash_labels.hash_to_string(*name_hash);
                                    if current_bone_name == bone_name {
                                        // Found the bone template, return a copy
                                        return Some(item.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ParamValue::Struct(struct_data) => {
                for (_, field_value) in &struct_data.fields {
                    if let Some(template) = Self::find_bone_template_static(field_value, bone_name, hash_labels) {
                        return Some(template);
                    }
                }
            }
            _ => {}
        }
        None
    }
    
    /// Static version to duplicate bone in param value
    fn duplicate_bone_in_param_value_static(value: &mut ParamValue, bone_template: ParamValue) -> bool {
        let mut duplicated = false;
        
        match value {
            ParamValue::List(list) => {
                // Check if this is a bones list by looking for a "name" field in the first item
                if let Some(ParamValue::Struct(first_struct)) = list.values.first() {
                    let mut has_name_field = false;
                    for (_hash, _) in &first_struct.fields {
                        // We need to check if this is a "name" field, but we don't have hash_labels here
                        // For now, we'll assume any struct in a list is a bone if it has fields
                        has_name_field = true;
                        break;
                    }
                    
                    if has_name_field {
                        // This is a bones list, add the duplicate
                        list.values.push(bone_template.clone());
                        duplicated = true;
                    }
                }
            }
            ParamValue::Struct(struct_data) => {
                for (_, field_value) in &mut struct_data.fields {
                    if Self::duplicate_bone_in_param_value_static(field_value, bone_template.clone()) {
                        duplicated = true;
                    }
                }
            }
            _ => {}
        }
        
        duplicated
    }
    

    
    fn show_struct_editor(&mut self, ui: &mut egui::Ui, node: &ParamNode, _selected_path: &str) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Fields");
            // Check if this struct has bones/connections/collisions fields
            let has_bulk_fields = node.children.iter().any(|c| ["bones", "connections", "collisions"].contains(&c.name.as_str()));
            if has_bulk_fields {
                if ui.button("Enable").clicked() {
                    self.bulk_set_enable_disable(_selected_path, node, true);
                }
                if ui.button("Disable").clicked() {
                    self.bulk_set_enable_disable(_selected_path, node, false);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ Add Field").clicked() {
                    self.add_child_to_path(_selected_path);
                }
            });
        });
        ui.add_space(5.0);
        
        let mut new_editing_value = self.editing_value.clone();
        let mut new_status_message = None;
        
        // Performance optimization: Use virtual scrolling for large structs
        let total_fields = node.children.len();
        let visible_fields = if VIRTUAL_SCROLLING_ENABLED && total_fields > MAX_VISIBLE_ITEMS {
            total_fields.min(MAX_VISIBLE_ITEMS)
        } else {
            total_fields
        };
        
        egui::ScrollArea::vertical().id_salt("struct_fields_scroll_area").max_height(400.0).show(ui, |ui| {
            egui::Grid::new("struct_fields")
                .num_columns(5)
                .striped(true)
                .spacing([15.0, 6.0])
                .min_col_width(120.0)
                .show(ui, |ui| {
                    ui.strong("Key");
                    ui.strong("Hash");
                    ui.strong("Type");
                    ui.strong("Value");
                    ui.strong("Actions");
                    ui.end_row();
                    
                    for (i, child) in node.children.iter().take(visible_fields).enumerate() {
                        let child_path = format!("{}[{}]", _selected_path, i);
                        
                        // Key/Name column - editable
                        let key_edit_path = format!("{}_key", child_path);
                        let is_editing_key = new_editing_value.as_ref()
                            .map(|(path, _)| path == &key_edit_path)
                            .unwrap_or(false);
                        
                        if is_editing_key {
                            let edit_key = new_editing_value.as_ref().unwrap().1.clone();
                            let autocomplete_id = format!("key_edit_{}", child_path);
                            
                            // Use autocomplete text input for key editing
                            let mut edit_key_mut = edit_key;
                            let ac = self.show_autocomplete_text_input(ui, &mut edit_key_mut, &autocomplete_id, 10);
                            
                            // Always update the editing value
                            new_editing_value = Some((key_edit_path.clone(), edit_key_mut.clone()));
                            
                            if ac.committed {
                                // Generate hash for new key name
                                let new_hash = self.param_file.hash_labels.add_label_and_save(&edit_key_mut, self.param_labels_path.as_deref());
                                
                                // Actually update the node using the new method with undo tracking
                                if self.update_node_key_with_undo(&child_path, edit_key_mut.clone(), new_hash) {
                                    let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                                    new_status_message = Some(format!("Key renamed to '{}' (hash: 0x{:X}) and saved to {}", edit_key_mut, new_hash, path_display));
                                    // Refresh tree to show updated keys
                                    // self.refresh_tree();
                                } else {
                                    new_status_message = Some("Failed to update key".to_string());
                                }
                                new_editing_value = None;
                            } else if ac.cancelled {
                                new_editing_value = None;
                            }
                        } else {
                            let display_name = if child.name.len() > 15 {
                                format!("{}...", &child.name[..12])
                            } else {
                                child.name.clone()
                            };
                            
                            let response = ui.add(
                                egui::Label::new(egui::RichText::new(display_name).strong())
                                    .sense(egui::Sense::click())
                            );
                            
                            if response.clicked() {
                                new_editing_value = Some((key_edit_path, child.name.clone()));
                            }
                            
                            if response.hovered() {
                                response.on_hover_text("Click to rename key");
                            }
                        }
                        
                        // Hash column (read-only)
                        ui.monospace(format!("0x{:X}", child.hash));
                        
                        // Type column with dropdown
                        egui::ComboBox::from_id_salt(format!("type_{}", i))
                            .selected_text(child.get_type_name())
                            .show_ui(ui, |ui| {
                                let types = ["bool", "sbyte", "byte", "short", "ushort", "int", "uint", "float", "hash40", "string", "list", "struct"];
                                for type_name in types {
                                    if ui.selectable_label(false, type_name).clicked() {
                                        new_status_message = Some(format!("Type changed to {}", type_name));
                                    }
                                }
                            });
                        
                        // Value column â€” bools use a checkbox like the original PRC editor
                        let is_editing = new_editing_value.as_ref()
                            .map(|(path, _)| path == &child_path)
                            .unwrap_or(false);
                        
                        if let ParamValue::Bool(val) = child.value {
                            let mut checked = val;
                            if ui.checkbox(&mut checked, "").changed() {
                                if self.update_node_value_with_undo(&child_path, ParamValue::Bool(checked)) {
                                    new_status_message = Some(format!("{} set to {}", child.name, checked));
                                }
                            }
                        } else if is_editing {
                            let edit_value = new_editing_value.as_ref().unwrap().1.clone();
                            let autocomplete_id = format!("struct_value_edit_{}", child_path);
                            
                            // Use autocomplete text input for value editing
                            let mut edit_value_mut = edit_value;
                            let ac = self.show_autocomplete_text_input(ui, &mut edit_value_mut, &autocomplete_id, 10);
                            
                            // Always update the editing value
                            new_editing_value = Some((child_path.clone(), edit_value_mut.clone()));
                            
                            if ac.committed {
                                // If it's a Hash40 value and looks like a label, generate hash
                                if matches!(child.value, ParamValue::Hash(_)) && !edit_value_mut.starts_with("0x") {
                                    let hash = self.param_file.hash_labels.add_label_and_save(&edit_value_mut, self.param_labels_path.as_deref());
                                    
                                    // Actually update the hash value using the new method with undo tracking
                                    if self.update_node_value_with_undo(&child_path, ParamValue::Hash(hash)) {
                                        let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                                        new_status_message = Some(format!("Hash40 value set to '{}' (0x{:X}) and saved to {}", edit_value_mut, hash, path_display));
                                        // Refresh tree to show updated values
                                        // self.refresh_tree();
                                    } else {
                                        new_status_message = Some("Failed to update hash value".to_string());
                                    }
                                } else {
                                    // Try to parse the value based on the current type
                                    let parsed_value = match &child.value {
                                        ParamValue::Bool(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<bool>() {
                                                Some(ParamValue::Bool(val))
                                            } else if edit_value_mut.to_lowercase() == "true" {
                                                Some(ParamValue::Bool(true))
                                            } else if edit_value_mut.to_lowercase() == "false" {
                                                Some(ParamValue::Bool(false))
                                            } else { None }
                                        }
                                        ParamValue::I8(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<i8>() {
                                                Some(ParamValue::I8(val))
                                            } else { None }
                                        }
                                        ParamValue::U8(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<u8>() {
                                                Some(ParamValue::U8(val))
                                            } else { None }
                                        }
                                        ParamValue::I16(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<i16>() {
                                                Some(ParamValue::I16(val))
                                            } else { None }
                                        }
                                        ParamValue::U16(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<u16>() {
                                                Some(ParamValue::U16(val))
                                            } else { None }
                                        }
                                        ParamValue::I32(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<i32>() {
                                                Some(ParamValue::I32(val))
                                            } else { None }
                                        }
                                        ParamValue::U32(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<u32>() {
                                                Some(ParamValue::U32(val))
                                            } else { None }
                                        }
                                        ParamValue::F32(_) => {
                                            if let Ok(val) = edit_value_mut.parse::<f32>() {
                                                Some(ParamValue::F32(val))
                                            } else { None }
                                        }
                                        ParamValue::String(_) => {
                                            Some(ParamValue::String(edit_value_mut.clone()))
                                        }
                                        ParamValue::Hash(_) => {
                                            if let Ok(val) = u64::from_str_radix(&edit_value_mut.trim_start_matches("0x"), 16) {
                                                Some(ParamValue::Hash(val))
                                            } else { None }
                                        }
                                        _ => None,
                                    };
                                    
                                    if let Some(new_value) = parsed_value {
                                        if self.update_node_value_with_undo(&child_path, new_value.clone()) {
                                            new_status_message = Some(format!("Value updated to: {}", edit_value_mut));
                                            // Refresh tree to show updated values
                                            // self.refresh_tree();
                                        } else {
                                            new_status_message = Some("Failed to update value".to_string());
                                        }
                                    } else {
                                        new_status_message = Some(format!("Invalid value for type: {}", edit_value_mut));
                                    }
                                }
                                new_editing_value = None;
                            } else if ac.cancelled {
                                new_editing_value = None;
                            }
                        } else {
                            let value_str = child.get_value_string_with_labels(&self.param_file.hash_labels);
                            let display_value = if value_str.len() > 25 {
                                format!("{}...", &value_str[..22])
                            } else {
                                value_str.clone()
                            };
                            
                            let response = ui.add(
                                egui::Label::new(egui::RichText::new(display_value).monospace())
                                    .sense(egui::Sense::click())
                            );
                            
                            if response.clicked() {
                                new_editing_value = Some((child_path.clone(), value_str));
                            }
                            
                            if response.hovered() {
                                response.on_hover_text("Click to edit");
                            }
                        }
                        
                        // Actions column
                        ui.horizontal(|ui| {
                            if ui.small_button("✏").on_hover_text("Edit Value").clicked() {
                                let value_str = child.get_value_string_with_labels(&self.param_file.hash_labels);
                                new_editing_value = Some((child_path.clone(), value_str));
                            }
                            if ui.small_button("🔄").on_hover_text("Rename Key").clicked() {
                                let key_edit_path = format!("{}_key", child_path);
                                new_editing_value = Some((key_edit_path, child.name.clone()));
                            }
                            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                new_status_message = Some(format!("Delete field: {}", child.name));
                            }
                        });
                        
                        ui.end_row();
                    }
                });
        });
        
        self.set_editing_value(new_editing_value);
        if let Some(msg) = new_status_message {
            self.status_message = msg;
        }
    }
    
    fn show_list_editor(&mut self, ui: &mut egui::Ui, node: &ParamNode, _selected_path: &str) {
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Items");
            // If this list is named bones/connections/collisions, show the buttons
            if ["bones", "connections", "collisions"].contains(&node.name.as_str()) {
                if ui.button("Enable").clicked() {
                    // For lists, set all bools in this list to true
                    let mut all_bool_paths = Vec::new();
                    self.collect_bool_paths_from_value(&node.value, _selected_path, &mut all_bool_paths);
                    if all_bool_paths.is_empty() {
                        self.status_message = format!("No boolean fields found in {}", node.name);
                        return;
                    }
                    let mut set_any = false;
                    for (field_path, val) in all_bool_paths {
                        if !val {
                            self.update_node_value_with_undo(&field_path, ParamValue::Bool(true));
                            set_any = true;
                        }
                    }
                    if set_any {
                        self.status_message = format!("Enabled all boolean fields in {}", node.name);
                    } else {
                        self.status_message = format!("All boolean fields in {} already enabled", node.name);
                    }
                }
                if ui.button("Disable").clicked() {
                    // For lists, set all bools in this list to false
                    let mut all_bool_paths = Vec::new();
                    self.collect_bool_paths_from_value(&node.value, _selected_path, &mut all_bool_paths);
                    if all_bool_paths.is_empty() {
                        self.status_message = format!("No boolean fields found in {}", node.name);
                        return;
                    }
                    let mut set_any = false;
                    for (field_path, val) in all_bool_paths {
                        if val {
                            self.update_node_value_with_undo(&field_path, ParamValue::Bool(false));
                            set_any = true;
                        }
                    }
                    if set_any {
                        self.status_message = format!("Disabled all boolean fields in {}", node.name);
                    } else {
                        self.status_message = format!("All boolean fields in {} already disabled", node.name);
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ Add Item").clicked() {
                    self.add_child_to_path(_selected_path);
                }
            });
        });
        ui.add_space(5.0);
        
        let mut new_editing_value = self.editing_value.clone();
        let mut new_status_message = None;
        
        egui::ScrollArea::vertical().id_salt("list_items_scroll_area").max_height(400.0).show(ui, |ui| {
            egui::Grid::new("list_items")
                .num_columns(4)
                .striped(true)
                .spacing([15.0, 6.0])
                .min_col_width(80.0)
                .show(ui, |ui| {
                    ui.strong("Index");
                    ui.strong("Type");
                    ui.strong("Value");
                    ui.strong("Actions");
                    ui.end_row();
                    
                    for (i, child) in node.children.iter().enumerate() {
                        let child_path = format!("{}[{}]", _selected_path, i);
                        
                        // Index column
                        ui.label(i.to_string());
                        
                        // Type column with dropdown
                        egui::ComboBox::from_id_salt(format!("list_type_{}", i))
                            .selected_text(child.get_type_name())
                            .show_ui(ui, |ui| {
                                let types = ["bool", "sbyte", "byte", "short", "ushort", "int", "uint", "float", "hash40", "string", "list", "struct"];
                                for type_name in types {
                                    if ui.selectable_label(false, type_name).clicked() {
                                        new_status_message = Some(format!("Item {} type changed to {}", i, type_name));
                                    }
                                }
                            });
                        
                        // Value column
                        let is_editing = new_editing_value.as_ref()
                            .map(|(path, _)| path == &child_path)
                            .unwrap_or(false);
                        
                        if let ParamValue::Bool(val) = child.value {
                            let mut checked = val;
                            if ui.checkbox(&mut checked, "").changed() {
                                if self.update_node_value_with_undo(&child_path, ParamValue::Bool(checked)) {
                                    new_status_message = Some(format!("Item {} set to {}", i, checked));
                                }
                            }
                        } else if is_editing {
                            let mut edit_value = new_editing_value.as_ref().unwrap().1.clone();
                            let response = ui.text_edit_singleline(&mut edit_value);
                            
                            if self.request_edit_focus {
                                response.request_focus();
                                self.request_edit_focus = false;
                            }
                            
                            if response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                new_status_message = Some(format!("Item {} value edited to: {}", i, edit_value));
                                new_editing_value = None;
                            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                new_editing_value = None;
                            } else {
                                new_editing_value = Some((child_path.clone(), edit_value));
                            }
                        } else {
                            let value_str = child.get_value_string_with_labels(&self.param_file.hash_labels);
                            let display_value = if value_str.len() > 25 {
                                format!("{}...", &value_str[..22])
                            } else {
                                value_str.clone()
                            };
                            
                            let response = ui.add(
                                egui::Label::new(egui::RichText::new(display_value).monospace())
                                    .sense(egui::Sense::click())
                            );
                            
                            if response.clicked() {
                                new_editing_value = Some((child_path.clone(), value_str));
                            }
                            
                            if response.hovered() {
                                response.on_hover_text("Click to edit");
                            }
                        }
                        
                        // Actions column
                        ui.horizontal(|ui| {
                            if ui.small_button("✏").on_hover_text("Edit").clicked() {
                                let value_str = child.get_value_string_with_labels(&self.param_file.hash_labels);
                                new_editing_value = Some((child_path.clone(), value_str));
                            }
                            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                new_status_message = Some(format!("Delete item {}", i));
                            }
                        });
                        
                        ui.end_row();
                    }
                });
        });
        
        self.set_editing_value(new_editing_value);
        if let Some(msg) = new_status_message {
            self.status_message = msg;
        }
    }
    
    fn show_value_editor(&mut self, ui: &mut egui::Ui, node: &ParamNode, selected_path: &str) {
        ui.separator();
        ui.heading("Edit Value");
        ui.add_space(5.0);
        
        let mut new_editing_value = self.editing_value.clone();
        let mut new_status_message = None;
        
        egui::Grid::new("value_editor")
            .num_columns(3)
            .striped(false)
            .spacing([15.0, 8.0])
            .show(ui, |ui| {
                ui.strong("Type:");
                
                // Type dropdown
                egui::ComboBox::from_id_salt("value_type")
                    .selected_text(node.get_type_name())
                    .show_ui(ui, |ui| {
                        let types = ["bool", "sbyte", "byte", "short", "ushort", "int", "uint", "float", "hash40", "string", "list", "struct"];
                        for type_name in types {
                            if ui.selectable_label(false, type_name).clicked() {
                                new_status_message = Some(format!("Type changed to {}", type_name));
                            }
                        }
                    });
                
                ui.label(""); // Empty third column
                ui.end_row();
                
                ui.strong("Value:");
                
                // Value editor
                let is_editing = new_editing_value.as_ref()
                    .map(|(path, _)| path == selected_path)
                    .unwrap_or(false);
                
                if let ParamValue::Bool(val) = node.value {
                    let mut checked = val;
                    if ui.checkbox(&mut checked, "").changed() {
                        if self.update_node_value_with_undo(selected_path, ParamValue::Bool(checked)) {
                            new_status_message = Some(format!("Value set to {}", checked));
                        }
                    }
                } else if is_editing {
                    let edit_value = new_editing_value.as_ref().unwrap().1.clone();
                    let autocomplete_id = format!("value_edit_{}", selected_path);
                    
                    // Use autocomplete text input instead of regular text input
                    let mut edit_value_mut = edit_value;
                    let ac = self.show_autocomplete_text_input(ui, &mut edit_value_mut, &autocomplete_id, 10);
                    
                    // Always update the editing value
                    new_editing_value = Some((selected_path.to_string(), edit_value_mut.clone()));
                    
                    // Check if we should exit editing mode
                    if ac.committed {
                        new_status_message = Some(format!("Value saved: {}", edit_value_mut));
                        new_editing_value = None;
                    } else if ac.cancelled {
                        new_editing_value = None;
                    }
                } else {
                    let value_str = node.get_value_string_with_labels(&self.param_file.hash_labels);
                    let response = ui.add(
                        egui::Label::new(egui::RichText::new(&value_str).monospace())
                            .sense(egui::Sense::click())
                    );
                    
                    if response.clicked() {
                        new_editing_value = Some((selected_path.to_string(), value_str));
                    }
                    
                    if response.hovered() {
                        response.on_hover_text("Click to edit");
                    }
                }
                
                // Edit button (bools are toggled by the checkbox)
                if !matches!(node.value, ParamValue::Bool(_)) && ui.button("Edit").clicked() {
                    let value_str = node.get_value_string_with_labels(&self.param_file.hash_labels);
                    new_editing_value = Some((selected_path.to_string(), value_str));
                }
                
                ui.end_row();
            });
        
        self.set_editing_value(new_editing_value);
        if let Some(msg) = new_status_message {
            self.status_message = msg;
        }
    }
    
    /// Delete a node at the given path
    fn delete_node(&mut self, path: &str) -> bool {
        // Cannot delete root
        if path == "root" {
            return false;
        }
        
        let indices = match self.param_file.parse_node_path(path) {
            Some(indices) => indices,
            None => return false,
        };
        
        if indices.is_empty() {
            return false; // Cannot delete root
        }
        
        // Get the node to delete for undo purposes
        let node_to_delete = match self.find_node_by_path(path) {
            Some(node) => node.clone(),
            None => return false,
        };
        
        // Get parent path and index to delete
        let parent_indices = &indices[..indices.len() - 1];
        let delete_index = indices[indices.len() - 1];
        let parent_path = if parent_indices.is_empty() {
            "root".to_string()
        } else {
            format!("root{}", parent_indices.iter().map(|i| format!("[{}]", i)).collect::<String>())
        };
        
        // Delete from the underlying data structure
        if let Some(root) = &mut self.param_file.root {
            if Self::delete_from_param_value(&mut root.value, parent_indices, delete_index, 0) {
                // Record undo action
                self.push_undo_action(UndoAction::DeleteNode {
                    path: path.to_string(),
                    node: node_to_delete,
                    parent_path,
                    index: delete_index,
                });
                
                // Also delete from the display tree
                Self::delete_from_display_tree(&mut self.param_file.root, parent_indices, delete_index, 0);
                return true;
            }
        }
        
        false
    }
    
    /// Delete from the underlying ParamValue structure
    fn delete_from_param_value(
        value: &mut ParamValue,
        parent_indices: &[usize],
        delete_index: usize,
        depth: usize
    ) -> bool {
        if depth == parent_indices.len() {
            // We're at the parent, delete the child
            match value {
                ParamValue::Struct(ref mut s) => {
                    // Find the hash key at the given index
                    if let Some((hash_to_remove, _)) = s.fields.get_index(delete_index) {
                        let hash_to_remove = *hash_to_remove;
                        s.fields.shift_remove(&hash_to_remove);
                        return true;
                    }
                }
                ParamValue::List(ref mut l) => {
                    if delete_index < l.values.len() {
                        l.values.remove(delete_index);
                        return true;
                    }
                }
                _ => {}
            }
            return false;
        }
        
        // Continue recursing
        let current_index = parent_indices[depth];
        match value {
            ParamValue::Struct(ref mut s) => {
                if let Some((_, field_value)) = s.fields.get_index_mut(current_index) {
                    return Self::delete_from_param_value(field_value, parent_indices, delete_index, depth + 1);
                }
            }
            ParamValue::List(ref mut l) => {
                if current_index < l.values.len() {
                    return Self::delete_from_param_value(&mut l.values[current_index], parent_indices, delete_index, depth + 1);
                }
            }
            _ => {}
        }
        
        false
    }
    
    /// Delete from the display tree
    fn delete_from_display_tree(
        node: &mut Option<ParamNode>,
        parent_indices: &[usize],
        delete_index: usize,
        depth: usize
    ) -> bool {
        if let Some(current_node) = node {
            if depth == parent_indices.len() {
                // We're at the parent, delete the child
                if delete_index < current_node.children.len() {
                    current_node.children.remove(delete_index);
                    return true;
                }
                return false;
            }
            
            // Continue recursing
            let current_index = parent_indices[depth];
            if current_index < current_node.children.len() {
                let mut child_option = Some(std::mem::replace(&mut current_node.children[current_index], ParamNode::new("temp".to_string(), 0, ParamValue::Bool(false))));
                let result = Self::delete_from_display_tree(&mut child_option, parent_indices, delete_index, depth + 1);
                if let Some(updated_child) = child_option {
                    current_node.children[current_index] = updated_child;
                }
                return result;
            }
        }
        false
    }
    
    /// Paste a node into the target path
    fn paste_node_into(&mut self, target_path: &str, node_to_paste: ParamNode) -> bool {
        // Get the target node to determine how to paste
        if let Some(target_node) = self.find_node_by_path(target_path) {
            match &target_node.value {
                // If target is a struct, add source as a new field (regardless of source type)
                ParamValue::Struct(_) => {
                    return self.add_node_with_undo(target_path, node_to_paste);
                }
                // If target is a list, add source as a new item (regardless of source type)
                ParamValue::List(_) => {
                    return self.add_node_with_undo(target_path, node_to_paste);
                }
                _ => {
                    // For other cases (primitive values), replace the value using undo tracking
                    return self.update_node_value_with_undo(target_path, node_to_paste.value);
                }
            }
        }
        
        false
    }
    

    
    /// Add a node to the underlying ParamValue structure
    fn add_to_param_value(
        value: &mut ParamValue,
        indices: &[usize],
        node_to_add: ParamNode,
        depth: usize
    ) -> bool {
        if depth == indices.len() {
            // We're at the target, add the node
            match value {
                ParamValue::Struct(ref mut s) => {
                    // For structs, we need to generate a unique hash if there's a collision
                    let mut hash = node_to_add.hash;
                    let mut counter = 1;
                    while s.fields.contains_key(&hash) {
                        // Generate a new hash by adding a counter
                        hash = node_to_add.hash.wrapping_add(counter);
                        counter += 1;
                        if counter > 1000 {
                            return false; // Prevent infinite loop
                        }
                    }
                    s.fields.insert(hash, node_to_add.value);
                    return true;
                }
                ParamValue::List(ref mut l) => {
                    l.values.push(node_to_add.value);
                    return true;
                }
                _ => return false, // Can only add to structs and lists
            }
        }
        
        // Continue recursing
        let current_index = indices[depth];
        match value {
            ParamValue::Struct(ref mut s) => {
                if let Some((_, field_value)) = s.fields.get_index_mut(current_index) {
                    return Self::add_to_param_value(field_value, indices, node_to_add, depth + 1);
                }
            }
            ParamValue::List(ref mut l) => {
                if current_index < l.values.len() {
                    return Self::add_to_param_value(&mut l.values[current_index], indices, node_to_add, depth + 1);
                }
            }
            _ => {}
        }
        
        false
    }
    
    /// Add a node to the display tree
    #[allow(dead_code)]
    fn add_to_display_tree(
        node: &mut Option<ParamNode>,
        indices: &[usize],
        node_to_add: ParamNode,
        depth: usize
    ) -> bool {
        if let Some(current_node) = node {
            if depth == indices.len() {
                // We're at the target, add the node
                current_node.children.push(node_to_add);
                return true;
            }
            
            // Continue recursing
            let current_index = indices[depth];
            if current_index < current_node.children.len() {
                let mut child_option = Some(std::mem::replace(&mut current_node.children[current_index], ParamNode::new("temp".to_string(), 0, ParamValue::Bool(false))));
                let result = Self::add_to_display_tree(&mut child_option, indices, node_to_add, depth + 1);
                if let Some(updated_child) = child_option {
                    current_node.children[current_index] = updated_child;
                }
                return result;
            }
        }
        false
    }
    
    /// Ensure children are built for expanded nodes and selected node (lazy loading)
    fn ensure_expanded_children_built(&mut self) {
        if self.param_file.root.is_some() {
            let expanded_nodes = self.expanded_nodes.clone();
            let selected_node = self.selected_node.clone();
            let hash_labels = &self.param_file.hash_labels;
            if let Some(root) = self.param_file.root.as_mut() {
                Self::ensure_children_for_path_static(root, "root", 0, &expanded_nodes, &selected_node, hash_labels);
            }
        }
    }
    
    /// Recursively ensure children are built for expanded nodes and selected node (static version)
    fn ensure_children_for_path_static(
        node: &mut ParamNode, 
        path: &str, 
        depth: usize,
        expanded_nodes: &HashSet<String>,
        selected_node: &Option<String>,
        hash_labels: &crate::hash_labels::HashLabels
    ) {
        // Limit depth to prevent infinite recursion or excessive memory usage
        if depth > 50 {
            return;
        }
        
        let is_expanded = expanded_nodes.contains(path);
        let is_selected = selected_node.as_ref().map_or(false, |selected| selected == path);
        
        // Build children if the node is expanded OR selected (so parameter details can show content)
        if (is_expanded || is_selected) && node.is_expandable() {
            // Build children for this node if needed
            node.ensure_children_built(hash_labels);
            
            // Recursively ensure children for expanded child nodes
            for (i, child) in node.children.iter_mut().enumerate() {
                let child_path = format!("{}[{}]", path, i);
                Self::ensure_children_for_path_static(child, &child_path, depth + 1, expanded_nodes, selected_node, hash_labels);
            }
        }
    }

    fn find_node_by_path(&self, path: &str) -> Option<&ParamNode> {
        let root = self.param_file.get_root()?;
        
        if path == "root" {
            return Some(root);
        }
        
        // Parse path like "root[0][1][2]" to navigate to the correct node
        let mut current_node = root;
        let path_parts: Vec<&str> = path.split("[").skip(1).collect(); // Skip "root" part
        
        for part in path_parts {
            let index_str = part.trim_end_matches(']');
            if let Ok(index) = index_str.parse::<usize>() {
                if index < current_node.children.len() {
                    current_node = &current_node.children[index];
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        
        Some(current_node)
    }
    
    /// Optimized version of find_node_by_path that uses caching
    fn find_node_by_path_cached(&mut self, path: &str) -> Option<&ParamNode> {
        // Check cache first
        if self.node_cache.contains_key(path) {
            return self.node_cache.get(path);
        }
        
        // Find node and cache it
        if let Some(node) = self.find_node_by_path(path) {
            self.node_cache.insert(path.to_string(), node.clone());
            return self.node_cache.get(path);
        }
        
        None
    }

    /// Refresh the tree display from the underlying data structure
    /// This should be called after making changes to ensure UI consistency
    #[allow(dead_code)]
    fn refresh_tree(&mut self) {
        if let Some(_) = self.param_file.get_root() {
            // Preserve expanded state and selection
            let expanded_nodes = self.expanded_nodes.clone();
            let selected_path = self.selected_node.clone();
            
            // Rebuild the tree from the underlying data structure
            self.param_file.rebuild_tree();
            
            // Restore expanded state
            self.expanded_nodes = expanded_nodes;
            
            // Try to restore selection if it still exists
            if let Some(path) = selected_path {
                if self.find_node_by_path(&path).is_some() {
                    self.selected_node = Some(path);
                } else {
                    self.selected_node = None;
                }
            }
        }
    }

    fn open_file_dialog(&mut self) {
        if self.param_labels_path.is_none() {
            self.ensure_param_labels();
        }
        if self.param_labels_path.is_none() {
            self.status_message = "Please load ParamLabels.csv first before opening parameter files".to_string();
            self.prompt_for_labels_file();
            return;
        }
        
        if let Some(file_path) = FileDialog::new()
            .add_filter("Param files", &["prc", "prcx", "stdat", "stdatx", "stprm", "stprmx"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            debug_log!("Loading file: {}", file_path.display());
            self.status_message = format!("Opening file: {}", file_path.display());
            
            match std::fs::read(&file_path) {
                Ok(data) => {
                    debug_log!("File read successfully, size: {} bytes", data.len());
                    
                    // Pre-allocate memory for large files
                    if data.len() > MEMORY_BUFFER_SIZE {
                        debug_log!("Large file detected, pre-allocating memory");
                        // Force garbage collection before processing large files
                        #[cfg(debug_assertions)]
                        {
                            let layout = std::alloc::Layout::from_size_align(1024 * 1024, 1).unwrap();
                            let ptr = unsafe { std::alloc::System.alloc_zeroed(layout) };
                            if !ptr.is_null() {
                                // Deallocate immediately to trigger GC
                                unsafe { std::alloc::System.dealloc(ptr, layout); }
                            }
                        }
                    }
                    
                    let filename = file_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown");
                    
                    match self.param_file.open(&data, filename) {
                        Ok(()) => {
                            debug_log!("File parsed successfully");
                            self.status_message = format!("Successfully opened: {}", filename);
                            self.current_file_path = Some(file_path.clone());
                            self.selected_node = None;
                            self.expanded_nodes.clear();
                            self.mark_tree_dirty(); // Mark tree items as dirty when new file is loaded
                            // Rebuild tree with labels if they're already loaded
                            if !self.param_file.hash_labels.is_empty() {
                                debug_log!("Rebuilding tree with labels");
                                self.param_file.rebuild_tree_with_labels();
                            }
                            self.on_param_file_opened();
                        }
                        Err(e) => {
                            debug_log!("File parse failed: {}", e);
                            self.status_message = format!("Error opening file: {}", e);
                            // Clear any partial data
                            self.param_file.root = None;
                        }
                    }
                }
                Err(e) => {
                    debug_log!("File read failed: {}", e);
                    self.status_message = format!("Error reading file: {}", e);
                }
            }
        }
    }

    fn on_param_file_opened(&mut self) {
        self.flip_preview.apply_pending = true;
        if crate::flip::is_flip_prc(&self.param_file, self.current_file_path.as_deref()) {
            if let Some(path) = self
                .current_file_path
                .as_ref()
                .and_then(|p| crate::flip::suggested_model_folder(p))
            {
                if path.exists() {
                    self.flip_preview.load_model_folder(path);
                }
            }
        }
    }

    /// Save directly to the currently open file (Ctrl+S). Falls back to the
    /// "Save As" dialog if we don't know where the file came from.
    fn save_file(&mut self) {
        let path = match &self.current_file_path {
            Some(path) => path.clone(),
            None => {
                self.save_file_dialog();
                return;
            }
        };
        
        self.status_message = format!("Saving file: {}", path.display());
        match self.param_file.save(path.to_str().unwrap_or("output.prc")) {
            Ok(()) => {
                self.status_message = format!("Successfully saved: {}", path.display());
            }
            Err(e) => {
                self.status_message = format!("Error saving file: {}", e);
            }
        }
    }
    
    fn save_file_dialog(&mut self) {
        if let Some(file_path) = FileDialog::new()
            .add_filter("Param files", &["prc", "prcx", "stdat", "stdatx", "stprm", "stprmx"])
            .add_filter("All files", &["*"])
            .set_file_name(self.param_file.get_filename())
            .save_file()
        {
            self.status_message = format!("Saving file: {}", file_path.display());
            
            match self.param_file.save(file_path.to_str().unwrap_or("output.prc")) {
                Ok(()) => {
                    self.status_message = format!("Successfully saved: {}", file_path.display());
                    // Remember this location so subsequent Ctrl+S saves in place here.
                    self.current_file_path = Some(file_path);
                }
                Err(e) => {
                    self.status_message = format!("Error saving file: {}", e);
                }
            }
        }
    }

    fn download_labels(&mut self) {
        if let Some(path) = Self::default_param_labels_path() {
            let _ = self.download_param_labels_file(&path);
        } else {
            self.status_message =
                "Could not find AppData/Roaming to store ParamLabels.csv".to_string();
        }
    }

    fn unresolved_in_open_file(&self) -> Vec<u64> {
        let mut hashes = std::collections::HashSet::new();
        if let Some(root) = self.param_file.get_root() {
            hash_crack::collect_hashes(&root.value, &mut hashes);
        }
        hash_crack::unresolved_hashes(&self.param_file.hash_labels, &hashes)
    }

    fn extra_crack_names(&self) -> Vec<String> {
        let mut names = self.flip_preview.dictionary_names();
        let mut folders = Vec::new();
        if let Some(path) = &self.current_file_path {
            if let Some(motion) = crate::flip::suggested_motion_folder(path) {
                folders.push(motion);
            } else if let Some(parent) = path.parent() {
                folders.push(parent.to_path_buf());
            }
        }
        if let Some(model_path) = &self.flip_preview.model_path {
            if !folders.iter().any(|folder| folder == model_path) {
                folders.push(model_path.clone());
            }
        }
        names.extend(FlipPreviewState::anim_names_from_folders(&folders));
        names.sort();
        names.dedup();
        names
    }

    fn apply_crack_hits(&mut self, hits: &[CrackHit]) -> usize {
        if hits.is_empty() {
            return 0;
        }
        if let Some(path) = self.param_labels_path.clone() {
            self.param_file.hash_labels.set_persist_path(Some(path));
        }
        for hit in hits {
            self.param_file
                .hash_labels
                .add_label_for_hash(hit.hash, &hit.label);
        }
        self.param_file.rebuild_tree_with_labels();
        self.invalidate_label_list_cache();
        self.flip_preview.apply_pending = true;
        self.tree_items_dirty = true;
        hits.len()
    }

    fn crack_open_file(&mut self) {
        if self.crack_job.is_some() {
            return;
        }
        let targets = self.unresolved_in_open_file();
        if targets.is_empty() {
            self.status_message = "No unresolved hashes in this file.".to_string();
            return;
        }
        let extra = self.extra_crack_names();
        let model_name_count = extra.len();
        let known: Vec<String> = self
            .param_file
            .hash_labels
            .get_all_labels()
            .values()
            .cloned()
            .collect();
        let unknown_count = targets.len();
        let model_note = if model_name_count == 0 {
            " (no Flip Preview model loaded)"
        } else {
            ""
        };
        self.status_message = format!(
            "Cracking {unknown_count} hashed values against {model_name_count} model names{model_note}…"
        );
        self.crack_job = Some(std::thread::spawn(move || {
            let hits = hash_crack::crack_hashes(&known, &targets, &extra);
            let leftover: Vec<u64> = targets
                .iter()
                .copied()
                .filter(|hash| !hits.iter().any(|hit| hit.hash == *hash))
                .collect();
            CrackJobResult {
                hits,
                leftover,
                unknown_count,
                model_name_count,
            }
        }));
    }

    fn poll_crack_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.crack_job.take() else {
            return;
        };
        if !job.is_finished() {
            self.crack_job = Some(job);
            ctx.request_repaint();
            return;
        }
        match job.join() {
            Ok(result) => self.finish_crack_result(result),
            Err(_) => self.status_message = "Hash cracker failed.".to_string(),
        }
    }

    fn finish_crack_result(&mut self, result: CrackJobResult) {
        let names: Vec<String> = result
            .hits
            .iter()
            .map(|hit| format!("{} ({})", hit.label, hit.source))
            .collect();
        let model_hits: Vec<&str> = result
            .hits
            .iter()
            .filter(|hit| hit.source == "model")
            .map(|hit| hit.label.as_str())
            .collect();
        let leftover_text = if result.leftover.is_empty() {
            String::new()
        } else {
            format!(
                " Still hashed: {}.",
                hash_crack::format_leftover(&result.leftover)
            )
        };
        let model_text = if result.model_name_count == 0 {
            " No Flip Preview model was loaded, so bones/meshes/materials were not checked."
                .to_string()
        } else if model_hits.is_empty() {
            format!(
                " Checked {} names from the loaded model; none matched a leftover hash.",
                result.model_name_count
            )
        } else {
            let shown = model_hits
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let more = if model_hits.len() > 8 { "…" } else { "" };
            format!(
                " Matched from model ({} names): {shown}{more}.",
                result.model_name_count
            )
        };
        let n = self.apply_crack_hits(&result.hits);
        let unknown_count = result.unknown_count;
        if n == 0 {
            self.status_message = format!(
                "Could not reverse {unknown_count} hashes.{model_text}{leftover_text} These are probably PRC list/field names (not bones/meshes). Hash40 of length 14+ cannot be brute-forced; try Guess with a snake_case name of that exact length."
            );
        } else {
            let shown = names.iter().take(12).cloned().collect::<Vec<_>>().join(", ");
            let more = if names.len() > 12 { "…" } else { "" };
            let path_display = self
                .param_labels_path
                .as_deref()
                .unwrap_or("ParamLabels.csv");
            self.status_message = format!(
                "Cracked {n} of {unknown_count}: {shown}{more}. Saved to {path_display}.{model_text}{leftover_text}"
            );
        }
    }

    fn try_hash_guess(&mut self) {
        let guess = self.hash_guess_input.clone();
        let targets = self.unresolved_in_open_file();
        if targets.is_empty() {
            self.status_message = "No unresolved hashes in this file to test.".to_string();
            return;
        }
        if let Some(hit) = hash_crack::try_guess(&targets, &guess) {
            let label = hit.label.clone();
            let hash = hit.hash;
            self.apply_crack_hits(&[hit]);
            let path_display = self
                .param_labels_path
                .as_deref()
                .unwrap_or("ParamLabels.csv");
            self.status_message = format!(
                "Guess '{label}' matches 0x{hash:X} and was saved to {path_display}"
            );
            self.hash_guess_input.clear();
        } else {
            let hash = crate::hash_labels::HashLabels::hash40(guess.trim());
            let len = crate::hash_labels::HashLabels::hash40_length(hash);
            self.status_message = format!(
                "'{}' → 0x{:X} (length {len}) did not match any unresolved hash in this file.",
                guess.trim(),
                hash
            );
        }
    }
    
    /// Build a flattened list of visible tree items for keyboard navigation
    fn build_tree_items(&mut self) {
        debug_log!("Building tree items");
        self.tree_items.clear();
        
        if let Some(root) = self.param_file.get_root() {
            self.collect_visible_items(&root.clone(), "root".to_string(), 0);
        }
        
        debug_log!("Tree items built: {} items", self.tree_items.len());
        
        // Update selected_index to match selected_node
        if let Some(selected_path) = &self.selected_node {
            self.selected_index = self.tree_items.iter().position(|item| item == selected_path);
        } else if !self.tree_items.is_empty() {
            // If no selection, select the first item
            self.selected_index = Some(0);
            self.selected_node = self.tree_items.first().cloned();
        }
    }
    
    /// Recursively collect visible tree items
    fn collect_visible_items(&mut self, node: &ParamNode, path: String, _depth: usize) {
        self.tree_items.push(path.clone());
        
        // Only collect children if this node is expanded
        if node.is_expandable() && self.expanded_nodes.contains(&path) {
            for (i, child) in node.children.iter().enumerate() {
                let child_path = format!("{}[{}]", path, i);
                self.collect_visible_items(child, child_path, _depth + 1);
            }
        }
    }
    

    
    /// Show virtual scrolling interface for large nodes
    fn show_virtual_scroll_children(&mut self, ui: &mut egui::Ui, node: &ParamNode, path: &str) {
        let total_items = node.children.len();
        let visible_range = 0..total_items.min(MAX_VISIBLE_ITEMS);
        
        // Show navigation controls
        ui.horizontal(|ui| {
            ui.label(format!("Showing {} of {} items", visible_range.len(), total_items));
            if ui.button("Show All").clicked() {
                // Temporarily disable virtual scrolling for this node
                // This could be stored in a separate set
            }
        });
        
        // Show visible children
        for i in visible_range {
            let child = &node.children[i];
            let child_path = format!("{}[{}]", path, i);
            
            // Simplified child display for performance
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label("📄");
                
                let label = if child.name.is_empty() || child.name.starts_with("0x") {
                    format!("0x{:X}", child.hash)
                } else {
                    if child.name.len() > 20 {
                        format!("{}...", &child.name[..17])
                    } else {
                        child.name.clone()
                    }
                };
                
                let display_text = format!("{} ({})", label, child.get_type_name());
                let is_selected = self.selected_node.as_ref() == Some(&child_path);
                let label_response = ui.selectable_label(is_selected, display_text);
                
                if label_response.clicked() {
                    self.update_selection(child_path);
                }
            });
        }
        
        if total_items > MAX_VISIBLE_ITEMS {
            ui.label(format!("... and {} more items", total_items - MAX_VISIBLE_ITEMS));
        }
    }
    
    /// Navigate up in the tree
    fn navigate_up(&mut self) {
        if let Some(current_index) = self.selected_index {
            if current_index > 0 {
                self.selected_index = Some(current_index - 1);
                if let Some(new_path) = self.tree_items.get(current_index - 1) {
                    self.selected_node = Some(new_path.clone());
                }
            }
        } else if !self.tree_items.is_empty() {
            self.selected_index = Some(0);
            self.selected_node = self.tree_items.first().cloned();
        }
    }
    
    /// Navigate down in the tree
    fn navigate_down(&mut self) {
        if let Some(current_index) = self.selected_index {
            if current_index + 1 < self.tree_items.len() {
                self.selected_index = Some(current_index + 1);
                if let Some(new_path) = self.tree_items.get(current_index + 1) {
                    self.selected_node = Some(new_path.clone());
                }
            }
        } else if !self.tree_items.is_empty() {
            self.selected_index = Some(0);
            self.selected_node = self.tree_items.first().cloned();
        }
    }
    
    /// Navigate left (collapse current node or go to parent)
    fn navigate_left(&mut self) {
        if let Some(selected_path) = &self.selected_node.clone() {
            // If current node is expanded, collapse it
            if self.expanded_nodes.contains(selected_path) {
                self.expanded_nodes.remove(selected_path);
                // Rebuild tree items since visibility changed
                self.mark_tree_dirty();
            } else {
                // Go to parent node
                if let Some(parent_path) = self.get_parent_path(selected_path) {
                    self.selected_node = Some(parent_path.clone());
                    self.selected_index = self.tree_items.iter().position(|item| item == &parent_path);
                }
            }
        }
    }
    
    /// Navigate right (expand current node or go to first child)
    fn navigate_right(&mut self) {
        if let Some(selected_path) = &self.selected_node.clone() {
            if let Some(node) = self.find_node_by_path(selected_path) {
                if node.is_expandable() {
                    if !self.expanded_nodes.contains(selected_path) {
                        // Expand the node
                        self.expanded_nodes.insert(selected_path.clone());
                        // Rebuild tree items since visibility changed
                        self.mark_tree_dirty();
                    } else if !node.children.is_empty() {
                        // Go to first child
                        let first_child_path = format!("{}[0]", selected_path);
                        self.selected_node = Some(first_child_path.clone());
                        self.selected_index = self.tree_items.iter().position(|item| item == &first_child_path);
                    }
                }
            }
        }
    }
    
    /// Get the parent path of a given path
    fn get_parent_path(&self, path: &str) -> Option<String> {
        if path == "root" {
            return None;
        }
        
        // Find the last '[' and remove everything from there
        if let Some(last_bracket) = path.rfind('[') {
            Some(path[..last_bracket].to_string())
        } else {
            None
        }
    }
    
    /// Push an action to the undo stack and clear redo stack
    fn push_undo_action(&mut self, action: UndoAction) {
        self.undo_stack.push(action);
        self.redo_stack.clear(); // Clear redo stack when new action is performed
        
        // Limit undo stack size to prevent memory issues
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
    }
    
    /// Perform undo operation
    fn undo(&mut self) -> bool {
        if let Some(action) = self.undo_stack.pop() {
            match action.clone() {
                UndoAction::DeleteNode { path, node, parent_path, index } => {
                    // Restore the deleted node
                    if self.restore_node_at_index(&parent_path, node, index) {
                        self.redo_stack.push(UndoAction::AddNode { path });
                        self.status_message = "Undid delete operation".to_string();
                        self.build_tree_items();
                        return true;
                    }
                }
                UndoAction::AddNode { path } => {
                    // Remove the added node
                    if let Some(node) = self.find_node_by_path(&path).cloned() {
                        if let Some(parent_path) = self.get_parent_path(&path) {
                            if let Some(index) = self.get_node_index_in_parent(&path) {
                                if self.delete_node(&path) {
                                    self.redo_stack.push(UndoAction::DeleteNode { 
                                        path: path.clone(), 
                                        node, 
                                        parent_path, 
                                        index 
                                    });
                                    self.status_message = "Undid add operation".to_string();
                                    self.build_tree_items();
                                    return true;
                                }
                            }
                        }
                    }
                }
                UndoAction::UpdateValue { path, old_value, new_value } => {
                    // Restore the old value
                    if self.param_file.update_node_value(&path, old_value.clone()) {
                        self.redo_stack.push(UndoAction::UpdateValue { 
                            path, 
                            old_value: new_value, 
                            new_value: old_value 
                        });
                        self.status_message = "Undid value change".to_string();
                        self.param_file.rebuild_tree_with_labels();
                        return true;
                    }
                }
                UndoAction::UpdateKey { path, old_name, old_hash, new_name, new_hash } => {
                    // Restore the old key
                    if self.param_file.update_node_key(&path, old_name.clone(), old_hash) {
                        self.redo_stack.push(UndoAction::UpdateKey { 
                            path, 
                            old_name: new_name, 
                            old_hash: new_hash, 
                            new_name: old_name, 
                            new_hash: old_hash 
                        });
                        self.status_message = "Undid key change".to_string();
                        self.param_file.rebuild_tree_with_labels();
                        return true;
                    }
                }
            }
        }
        false
    }
    
    /// Perform redo operation
    fn redo(&mut self) -> bool {
        if let Some(action) = self.redo_stack.pop() {
            match action.clone() {
                UndoAction::DeleteNode { path, node, parent_path, index } => {
                    // Re-delete the node
                    if self.delete_node(&path) {
                        self.undo_stack.push(UndoAction::DeleteNode { path, node, parent_path, index });
                        self.status_message = "Redid delete operation".to_string();
                        self.build_tree_items();
                        return true;
                    }
                }
                UndoAction::AddNode { path: _ } => {
                    // This would require re-adding the node, which is complex
                    // For now, just indicate it's not supported
                    self.status_message = "Redo add operation not yet supported".to_string();
                    return false;
                }
                UndoAction::UpdateValue { path, old_value, new_value } => {
                    // Re-apply the new value
                    if self.param_file.update_node_value(&path, new_value.clone()) {
                        self.undo_stack.push(UndoAction::UpdateValue { 
                            path, 
                            old_value: old_value, 
                            new_value: new_value 
                        });
                        self.status_message = "Redid value change".to_string();
                        self.param_file.rebuild_tree_with_labels();
                        return true;
                    }
                }
                UndoAction::UpdateKey { path, old_name, old_hash, new_name, new_hash } => {
                    // Re-apply the new key
                    if self.param_file.update_node_key(&path, new_name.clone(), new_hash) {
                        self.undo_stack.push(UndoAction::UpdateKey { 
                            path, 
                            old_name, 
                            old_hash, 
                            new_name, 
                            new_hash 
                        });
                        self.status_message = "Redid key change".to_string();
                        self.param_file.rebuild_tree_with_labels();
                        return true;
                    }
                }
            }
        }
        false
    }
    
    /// Get the index of a node within its parent
    fn get_node_index_in_parent(&self, path: &str) -> Option<usize> {
        if let Some(parent_path) = self.get_parent_path(path) {
            if let Some(_parent_node) = self.find_node_by_path(&parent_path) {
                // Extract the index from the path
                if let Some(last_bracket) = path.rfind('[') {
                    if let Some(close_bracket) = path.rfind(']') {
                        let index_str = &path[last_bracket + 1..close_bracket];
                        return index_str.parse::<usize>().ok();
                    }
                }
            }
        }
        None
    }
    
    /// Restore a node at a specific index in its parent
    fn restore_node_at_index(&mut self, parent_path: &str, node: ParamNode, index: usize) -> bool {
        let parent_indices = match self.param_file.parse_node_path(parent_path) {
            Some(indices) => indices,
            None => return false,
        };
        
        // Add to the underlying data structure at the specific index
        if let Some(root) = &mut self.param_file.root {
            if Self::restore_to_param_value(&mut root.value, &parent_indices, node.clone(), index, 0) {
                // Rebuild the display tree to show the restored node
                self.param_file.rebuild_tree_with_labels();
                return true;
            }
        }
        
        false
    }
    
    /// Restore a node to the underlying ParamValue structure at a specific index
    fn restore_to_param_value(
        value: &mut ParamValue,
        indices: &[usize],
        node_to_restore: ParamNode,
        target_index: usize,
        depth: usize
    ) -> bool {
        if depth == indices.len() {
            // We're at the target parent, restore the node at the specific index
            match value {
                ParamValue::Struct(ref mut s) => {
                    // For structs, we need to insert at the correct position
                    // This is complex with IndexMap, so we'll rebuild it
                    let mut new_fields = indexmap::IndexMap::new();
                    let mut current_index = 0;
                    
                    // Copy existing fields, inserting the restored node at the target index
                    for (hash, field_value) in s.fields.iter() {
                        if current_index == target_index {
                            new_fields.insert(node_to_restore.hash, node_to_restore.value.clone());
                            current_index += 1;
                        }
                        new_fields.insert(*hash, field_value.clone());
                        current_index += 1;
                    }
                    
                    // If target index is at the end
                    if target_index >= s.fields.len() {
                        new_fields.insert(node_to_restore.hash, node_to_restore.value);
                    }
                    
                    s.fields = new_fields;
                    return true;
                }
                ParamValue::List(ref mut l) => {
                    if target_index <= l.values.len() {
                        l.values.insert(target_index, node_to_restore.value);
                        return true;
                    }
                }
                _ => return false,
            }
        }
        
        // Continue recursing
        let current_index = indices[depth];
        match value {
            ParamValue::Struct(ref mut s) => {
                if let Some((_, field_value)) = s.fields.get_index_mut(current_index) {
                    return Self::restore_to_param_value(field_value, indices, node_to_restore, target_index, depth + 1);
                }
            }
            ParamValue::List(ref mut l) => {
                if current_index < l.values.len() {
                    return Self::restore_to_param_value(&mut l.values[current_index], indices, node_to_restore, target_index, depth + 1);
                }
            }
            _ => {}
        }
        
        false
    }

    /// Update a node's value with undo tracking
    fn update_node_value_with_undo(&mut self, path: &str, new_value: ParamValue) -> bool {
        // Get the old value for undo
        if let Some(old_value) = self.param_file.get_node_value(path) {
            if self.param_file.update_node_value(path, new_value.clone()) {
                // Record undo action
                self.push_undo_action(UndoAction::UpdateValue {
                    path: path.to_string(),
                    old_value,
                    new_value,
                });
                return true;
            }
        }
        false
    }
    
    /// Update a node's key with undo tracking
    fn update_node_key_with_undo(&mut self, path: &str, new_name: String, new_hash: u64) -> bool {
        // Get the old key for undo
        if let Some(node) = self.find_node_by_path(path) {
            let old_name = node.name.clone();
            let old_hash = node.hash;
            
            if self.param_file.update_node_key(path, new_name.clone(), new_hash) {
                // Record undo action
                self.push_undo_action(UndoAction::UpdateKey {
                    path: path.to_string(),
                    old_name,
                    old_hash,
                    new_name,
                    new_hash,
                });
                return true;
            }
        }
        false
    }
    
    /// Add a child to the selected category on the parameter tree.
    /// Lists clone the last item so a new `flip_bones` entry keeps the same fields.
    /// Structs get a new default bool field.
    fn add_child_to_path(&mut self, path: &str) -> bool {
        let parent = match self.find_node_by_path(path) {
            Some(node) => node.clone(),
            None => {
                self.status_message = "No category selected".to_string();
                return false;
            }
        };

        let added = match &parent.value {
            ParamValue::Struct(_) => {
                let field_name = self.generate_sequential_name(path, "new_field");
                let field_hash = self.param_file.hash_labels.add_label_and_save(&field_name, self.param_labels_path.as_deref());
                let new_node = ParamNode::new(field_name.clone(), field_hash, ParamValue::Bool(false));
                if self.add_node_with_undo(path, new_node) {
                    let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                    self.status_message = format!("Added new field '{}' (hash: 0x{:X}) and saved to {}", field_name, field_hash, path_display);
                    true
                } else {
                    self.status_message = "Failed to add new field".to_string();
                    false
                }
            }
            ParamValue::List(list) => {
                let index = list.values.len();
                let value = list.values.last().cloned().unwrap_or(ParamValue::Bool(false));
                let new_node = ParamNode::new(format!("[{}]", index), index as u64, value);
                if self.add_node_with_undo(path, new_node) {
                    self.status_message = format!("Added new item to {} at position {}", parent.name, index);
                    true
                } else {
                    self.status_message = "Failed to add new item".to_string();
                    false
                }
            }
            _ => {
                self.status_message = "Select a list or struct category to add a child".to_string();
                false
            }
        };

        if added {
            self.expanded_nodes.insert(path.to_string());
            self.mark_tree_dirty();
        }
        added
    }

    /// Add a node with undo tracking
    fn add_node_with_undo(&mut self, target_path: &str, node_to_add: ParamNode) -> bool {
        let target_indices = match self.param_file.parse_node_path(target_path) {
            Some(indices) => indices,
            None => return false,
        };
        
        // Get the current size to calculate the new index
        let new_index = if let Some(target_node) = self.find_node_by_path(target_path) {
            match &target_node.value {
                ParamValue::Struct(s) => s.fields.len(),
                ParamValue::List(l) => l.values.len(),
                _ => return false,
            }
        } else {
            return false;
        };
        
        // Add to the underlying data structure
        if let Some(root) = &mut self.param_file.root {
            if Self::add_to_param_value(&mut root.value, &target_indices, node_to_add.clone(), 0) {
                // Calculate the path where the node was added
                let added_path = format!("{}[{}]", target_path, new_index);
                
                // Record undo action
                self.push_undo_action(UndoAction::AddNode {
                    path: added_path,
                });
                
                // Rebuild the display tree to show the new node
                self.param_file.rebuild_tree_with_labels();
                self.mark_tree_dirty(); // Mark tree items as dirty when node is added
                return true;
            }
        }
        
        false
    }

    /// Generate a sequential name for a new node to avoid duplicates
    fn generate_sequential_name(&self, parent_path: &str, _base_name: &str) -> String {
        // Get the parent node to check existing children
        if let Some(parent_node) = self.find_node_by_path(parent_path) {
            // Find the highest numeric name among all children
            let mut max_number = 0;
            
            for child in &parent_node.children {
                // Try to parse the child name as a number
                if let Ok(number) = child.name.parse::<u32>() {
                    max_number = max_number.max(number);
                }
                
                // Also try to extract numbers from names like "[18]" 
                let trimmed = child.name.trim_start_matches('[').trim_end_matches(']');
                if let Ok(number) = trimmed.parse::<u32>() {
                    max_number = max_number.max(number);
                }
            }
            
            // Return the next sequential number with brackets
            format!("[{}]", max_number + 1)
        } else {
            // Fallback if we can't find the parent
            "[1]".to_string()
        }
    }
    
    /// Ensure a name is unique by adding _copy suffix if needed
    fn ensure_unique_name(&self, node_path: &str, desired_name: &str) -> String {
        // Get the parent path to check for siblings
        if let Some(parent_path) = self.get_parent_path(node_path) {
            if let Some(parent_node) = self.find_node_by_path(&parent_path) {
                // Check if the desired name conflicts with any sibling (excluding self)
                let current_node_index = self.get_node_index_in_parent(node_path);
                
                let name_conflicts = parent_node.children.iter().enumerate().any(|(i, child)| {
                    // Don't compare with self
                    if let Some(current_index) = current_node_index {
                        if i == current_index {
                            return false;
                        }
                    }
                    child.name == desired_name
                });
                
                if name_conflicts {
                    // Add _# suffix starting from _2
                    let mut copy_counter = 2;
                    loop {
                        let copy_name = format!("{}_{}", desired_name, copy_counter);
                        
                        let copy_exists = parent_node.children.iter().enumerate().any(|(i, child)| {
                            // Don't compare with self
                            if let Some(current_index) = current_node_index {
                                if i == current_index {
                                    return false;
                                }
                            }
                            child.name == copy_name
                        });
                        
                        if !copy_exists {
                            return copy_name;
                        }
                        
                        copy_counter += 1;
                        if copy_counter > 100 {
                            break; // Prevent infinite loop
                        }
                    }
                }
            }
        }
        
        // No conflict or couldn't check, return original name
        desired_name.to_string()
    }
    
    /// Generate a name for pasting, preserving original names when possible
    fn generate_paste_name(&self, parent_path: &str, original_name: &str) -> String {
        // If the original name is not a number or bracketed number, try to preserve it
        let is_numeric = original_name.parse::<u32>().is_ok();
        let is_bracketed_numeric = {
            let trimmed = original_name.trim_start_matches('[').trim_end_matches(']');
            original_name.starts_with('[') && original_name.ends_with(']') && trimmed.parse::<u32>().is_ok()
        };
        
        if !is_numeric && !is_bracketed_numeric {
            // Check if this name already exists in the parent
            if let Some(parent_node) = self.find_node_by_path(parent_path) {
                let name_exists = parent_node.children.iter().any(|child| child.name == original_name);
                if !name_exists {
                    // Original name doesn't exist, we can use it as-is
                    return original_name.to_string();
                }
                
                // If it exists, try adding _# suffix for text names starting from _2
                let mut copy_counter = 2;
                loop {
                    let copy_name = format!("{}_{}", original_name, copy_counter);
                    
                    let copy_exists = parent_node.children.iter().any(|child| child.name == copy_name);
                    if !copy_exists {
                        return copy_name;
                    }
                    
                    copy_counter += 1;
                    if copy_counter > 100 {
                        break; // Prevent infinite loop
                    }
                }
            }
        }
        
        // For numeric names or when name conflicts exist, generate sequential name
        self.generate_sequential_name(parent_path, original_name)
    }
    
    /// Find the Smash Ultimate Blender plugin directory
    fn find_blender_addon_directory() -> Option<PathBuf> {
        // Get the user's AppData/Roaming directory
        if let Some(mut appdata_dir) = dirs::config_dir() {
            // On Windows, this gives us AppData/Roaming
            // On other platforms, we'll try to find Blender config
            
            #[cfg(target_os = "windows")]
            {
                appdata_dir.push("Blender Foundation");
                appdata_dir.push("Blender");
            }
            
            #[cfg(not(target_os = "windows"))]
            {
                // On Linux/Mac, Blender config is usually in ~/.config/blender/
                appdata_dir.push("blender");
            }
            
            // Try to find any Blender version directory
            if let Ok(entries) = std::fs::read_dir(&appdata_dir) {
                let mut blender_versions: Vec<_> = entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                    .filter_map(|entry| {
                        let name = entry.file_name().to_string_lossy().to_string();
                        // Look for version patterns like "4.2", "3.6", etc.
                        if name.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                            Some((name, entry.path()))
                        } else {
                            None
                        }
                    })
                    .collect();
                
                // Sort by version (newest first)
                blender_versions.sort_by(|a, b| b.0.cmp(&a.0));
                
                // Try each version directory
                for (_, version_path) in blender_versions {
                    let mut addon_path = version_path;
                    addon_path.push("scripts");
                    addon_path.push("addons");
                    addon_path.push("smash_ultimate_blender");
                    addon_path.push("dependencies");
                    addon_path.push("pyprc");
                    
                    if addon_path.exists() {
                        return Some(addon_path);
                    }
                }
            }
        }
        
        None
    }
    
    /// Get the path to the configuration file
    fn get_config_path() -> std::path::PathBuf {
        // Store config in the same directory as the executable
        let mut config_path = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        config_path.pop(); // Remove the executable name
        config_path.push("prc_editor_config.txt");
        config_path
    }
    
    /// Save the ParamLabels.csv path to a config file
    fn save_labels_path(&mut self, path: &str) {
        let config_path = Self::get_config_path();
        let _ = std::fs::write(&config_path, path);
        self.param_file.hash_labels.set_persist_path(Some(path.to_string()));
    }
    
    /// Load the saved ParamLabels.csv path from the config file
    fn load_saved_labels_path(&self) -> Option<String> {
        let config_path = Self::get_config_path();
        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                let path = content.trim().to_string();
                if !path.is_empty() {
                    Some(path)
                } else {
                    None
                }
            }
            Err(_) => None, // Config file doesn't exist yet
        }
    }
    
    /// Filter labels based on user input (case-insensitive)
    fn filter_labels(&self, input: &str, max_suggestions: usize) -> Vec<String> {
        if input.is_empty() {
            return Vec::new();
        }
        
        let input_lower = input.to_lowercase();
        let mut suggestions = Vec::new();
        
        // Collect all labels that start with the input
        for (_, label) in self.param_file.hash_labels.get_all_labels() {
            if label.to_lowercase().starts_with(&input_lower) {
                suggestions.push(label.clone());
                if suggestions.len() >= max_suggestions {
                    break;
                }
            }
        }
        
        // If we haven't reached max suggestions, add labels that contain the input
        if suggestions.len() < max_suggestions {
            for (_, label) in self.param_file.hash_labels.get_all_labels() {
                let label_lower = label.to_lowercase();
                if label_lower.contains(&input_lower) && !label_lower.starts_with(&input_lower) {
                    suggestions.push(label.clone());
                    if suggestions.len() >= max_suggestions {
                        break;
                    }
                }
            }
        }
        
        // Sort suggestions by length first (shorter matches first), then alphabetically
        suggestions.sort_by(|a, b| {
            let len_cmp = a.len().cmp(&b.len());
            if len_cmp == std::cmp::Ordering::Equal {
                a.cmp(b)
            } else {
                len_cmp
            }
        });
        
        suggestions
    }
    
    /// Show a text input with autocomplete dropdown
    /// Returns Some(selected_value) if a selection was made, None otherwise
    /// Updates the current editing target. When editing starts on a new field,
    /// requests keyboard focus so the text box is immediately usable (no need to
    /// click it a second time).
    fn set_editing_value(&mut self, value: Option<(String, String)>) {
        let new_path = value.as_ref().map(|(p, _)| p.as_str());
        let old_path = self.editing_value.as_ref().map(|(p, _)| p.as_str());
        if new_path.is_some() && new_path != old_path {
            self.request_edit_focus = true;
        }
        self.editing_value = value;
    }
    
    /// Renders a single-line text box with a floating autocomplete dropdown.
    ///
    /// The suggestion list is drawn in a foreground overlay so it does not
    /// change the height of the table row that owns the text box.
    ///
    /// `input_value` is edited in place (including when a suggestion is applied).
    /// Returns an [`AcResult`] describing whether the edit was committed (Enter /
    /// clicked away) or cancelled (Escape) this frame so the caller can react.
    fn show_autocomplete_text_input(
        &mut self, 
        ui: &mut egui::Ui, 
        input_value: &mut String,
        context_id: &str,
        max_suggestions: usize
    ) -> AcResult {
        let mut committed = false;
        let mut cancelled = false;
        // Set when the user interacts with the suggestion list (click / Tab) so we
        // don't treat the resulting focus loss as a "click away" commit.
        let mut suggestion_applied = false;
        
        // Keep the allocated cell as a single-line text box. The dropdown is
        // drawn later as a floating Area so it cannot stretch this row.
        let text_response = ui.text_edit_singleline(input_value);
        
        // Automatically focus the box the first frame it appears, so the user
        // can start typing immediately instead of having to click it again.
        if self.request_edit_focus {
            text_response.request_focus();
            self.request_edit_focus = false;
        }
        
        let has_focus = text_response.has_focus();
        
        // Update suggestions as the user types.
        if has_focus && text_response.changed() {
            self.autocomplete_suggestions = self.filter_labels(input_value, max_suggestions);
            self.autocomplete_active = !self.autocomplete_suggestions.is_empty();
            self.autocomplete_context_id = context_id.to_string();
            self.autocomplete_selected_index = if self.autocomplete_active { Some(0) } else { None };
        }
        
        let dropdown_open = self.autocomplete_active
            && self.autocomplete_context_id == context_id
            && !self.autocomplete_suggestions.is_empty();
        
        // Read the relevant key presses for this frame.
        let (mut press_up, mut press_down, mut press_tab, mut press_esc) = (false, false, false, false);
        ui.input(|i| {
            press_up = i.key_pressed(egui::Key::ArrowUp);
            press_down = i.key_pressed(egui::Key::ArrowDown);
            press_tab = i.key_pressed(egui::Key::Tab);
            press_esc = i.key_pressed(egui::Key::Escape);
        });
        
        // Keyboard navigation / actions while the dropdown is open.
        if has_focus && dropdown_open {
            if press_down {
                let next = self.autocomplete_selected_index
                    .map(|s| (s + 1).min(self.autocomplete_suggestions.len() - 1))
                    .unwrap_or(0);
                self.autocomplete_selected_index = Some(next);
            } else if press_up {
                let prev = self.autocomplete_selected_index
                    .map(|s| s.saturating_sub(1))
                    .unwrap_or(0);
                self.autocomplete_selected_index = Some(prev);
            } else if press_tab {
                // Tab accepts the highlighted suggestion and keeps editing.
                if let Some(sel) = self.autocomplete_selected_index {
                    if sel < self.autocomplete_suggestions.len() {
                        *input_value = self.autocomplete_suggestions[sel].clone();
                        suggestion_applied = true;
                    }
                }
                self.autocomplete_active = false;
                text_response.request_focus();
            }
        }
        
        // Escape: close the dropdown first, otherwise cancel the edit entirely.
        if has_focus && press_esc {
            if dropdown_open {
                self.autocomplete_active = false;
                suggestion_applied = true; // keep editing; don't commit this frame
                text_response.request_focus();
            } else {
                cancelled = true;
            }
        }
        
        // Draw the dropdown as a floating overlay so it sits in front of the
        // table instead of expanding the row (e.g. rhs_name).
        let mut dropdown_rect = None;
        if dropdown_open {
            let popup_id = ui.make_persistent_id(("autocomplete_popup", context_id));
            let popup_pos = text_response.rect.left_bottom();
            let min_width = text_response.rect.width().max(180.0);
            let suggestions = self.autocomplete_suggestions.clone();
            let selected_index = self.autocomplete_selected_index;
            let ctx = ui.ctx().clone();
            
            let area_response = egui::Area::new(popup_id)
                .order(egui::Order::Foreground)
                .fixed_pos(popup_pos)
                .constrain(true)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| {
                            // Pin width so the scrollbar sits on the right edge of
                            // the popup, not next to the longest suggestion.
                            ui.set_width(min_width);
                            ui.set_max_height(160.0);

                            egui::ScrollArea::vertical()
                                .max_height(160.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    for (i, suggestion) in suggestions.iter().enumerate() {
                                        let is_selected = selected_index == Some(i);
                                        let label_response = ui.add_sized(
                                            [ui.available_width(), 0.0],
                                            egui::Button::selectable(is_selected, suggestion.as_str()),
                                        );

                                        if label_response.clicked() {
                                            *input_value = suggestion.clone();
                                            suggestion_applied = true;
                                            self.autocomplete_active = false;
                                            text_response.request_focus();
                                        }

                                        if label_response.hovered() {
                                            self.autocomplete_selected_index = Some(i);
                                        }
                                    }
                                });
                        });
                });
            dropdown_rect = Some(area_response.response.rect);
        }
        
        // Is the pointer currently interacting with the dropdown? Pressing a
        // suggestion takes focus away from the text box, which would otherwise
        // look like a "click away" commit on the press frame (before the click
        // completes on release) and discard the suggestion. Guard against that.
        let pointer_in_dropdown = dropdown_rect.map_or(false, |rect| {
            ui.input(|i| {
                i.pointer
                    .interact_pos()
                    .map_or(false, |pos| rect.contains(pos))
            })
        });
        
        // Commit when the user presses Enter or clicks outside the box (both
        // cause it to lose focus), unless they just used the suggestion list.
        if text_response.lost_focus() && !suggestion_applied && !cancelled && !pointer_in_dropdown {
            committed = true;
        }
        
        if committed || cancelled {
            self.autocomplete_active = false;
        }
        
        AcResult { committed, cancelled }
    }
    
    fn invalidate_label_list_cache(&mut self) {
        self.label_list_cache_key = None;
        self.label_list_cache.clear();
    }

    fn ensure_label_list_cache(&mut self) {
        if self.label_list_cache_key.as_deref() == Some(self.label_editor_filter.as_str()) {
            return;
        }

        let filter = self.label_editor_filter.clone();
        let mut labels: Vec<(u64, String)> = self.param_file.hash_labels
            .get_labels_filtered(&filter)
            .into_iter()
            .map(|(hash, label)| (hash, label.clone()))
            .collect();
        labels.sort_by(|a, b| a.1.cmp(&b.1));
        self.label_list_cache = labels;
        self.label_list_cache_key = Some(filter);
    }

    fn show_label_editor_window(&mut self, ctx: &egui::Context) {
        if !self.show_label_editor {
            return;
        }
        
        let mut open = true; // Track if window should stay open
        
        egui::Window::new("Label Editor")
            .default_size([800.0, 420.0])
            .min_size([520.0, 320.0])
            .vscroll(false)
            .open(&mut open) // This adds the close button (X)
            .show(ctx, |ui| {
                ui.heading("Parameter Labels");
                ui.separator();
                
                // Add new label section
                ui.horizontal(|ui| {
                    ui.label("Add new label:");
                    ui.text_edit_singleline(&mut self.new_label_input);
                    
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        if !self.new_label_input.is_empty() {
                            let hash = self.param_file.hash_labels.add_label_and_save(&self.new_label_input, self.param_labels_path.as_deref());
                            let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                            self.status_message = format!("Added label '{}' with hash 0x{:X} and saved to {}", self.new_label_input, hash, path_display);
                            self.new_label_input.clear();
                            self.invalidate_label_list_cache();
                        }
                    }
                    if ui.button("Generate Hash").clicked() {
                        if !self.new_label_input.is_empty() {
                            let hash = self.param_file.hash_labels.add_label_and_save(&self.new_label_input, self.param_labels_path.as_deref());
                            let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                            self.status_message = format!("Added label '{}' with hash 0x{:X} and saved to {}", self.new_label_input, hash, path_display);
                            self.new_label_input.clear();
                            self.invalidate_label_list_cache();
                        }
                    }
                });
                
                ui.separator();
                
                // Add label to existing hash section
                ui.horizontal(|ui| {
                    ui.label("Add label to existing hash:");
                    ui.text_edit_singleline(&mut self.new_hash_input);
                    ui.text_edit_singleline(&mut self.new_label_input);
                    if ui.button("Set Label").clicked() {
                        if !self.new_hash_input.is_empty() && !self.new_label_input.is_empty() {
                            // Try to parse the hash
                            let hash_str = self.new_hash_input.trim_start_matches("0x");
                            if let Ok(hash) = u64::from_str_radix(hash_str, 16) {
                                // Add the label for this specific hash and save
                                match self.param_file.hash_labels.add_label_for_hash_and_save(hash, &self.new_label_input, self.param_labels_path.as_deref()) {
                                    Ok(()) => {
                                        let path_display = self.param_labels_path.as_deref().unwrap_or("ParamLabels.csv");
                                        self.status_message = format!("Added label '{}' for hash 0x{:X} and saved to {}", self.new_label_input, hash, path_display);
                                        self.invalidate_label_list_cache();
                                    }
                                    Err(e) => {
                                        self.status_message = format!("Added label but failed to save: {}", e);
                                    }
                                }
                                
                                self.new_hash_input.clear();
                                self.new_label_input.clear();
                            } else {
                                self.status_message = "Invalid hash format. Use hex format like 0x1133BC6DD8".to_string();
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Format: Hash (0x1133BC6DD8) + Label name");
                });

                ui.separator();
                ui.label("Unhashing uses ParamLabels as a dictionary. Hash40 is CRC32 plus the name length, so a full reverse is impossible — guessing the right-length name is not.");
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.param_file.get_root().is_some() && self.crack_job.is_none(),
                            egui::Button::new("Crack hashes in this file"),
                        )
                        .clicked()
                    {
                        self.crack_open_file();
                    }
                    ui.label("Guess a name:");
                    let guess_resp = ui.text_edit_singleline(&mut self.hash_guess_input);
                    let try_guess = ui.button("Try guess").clicked()
                        || (guess_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if try_guess {
                        self.try_hash_guess();
                    }
                });
                
                // Search input and pagination controls
                self.ensure_label_list_cache();
                let total_labels = self.label_list_cache.len();
                let per_page = self.labels_per_page.max(1);
                let total_pages = (total_labels + per_page - 1) / per_page;
                if self.label_page == 0 {
                    self.label_page = 1;
                }
                if self.label_page > total_pages.max(1) {
                    self.label_page = total_pages.max(1);
                }
                
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let search_response = ui.text_edit_singleline(&mut self.label_editor_filter);
                    if search_response.changed() {
                        self.label_page = 1;
                    }
                    if ui.button("Clear").clicked() {
                        self.label_editor_filter.clear();
                        self.label_page = 1;
                    }
                    
                    ui.separator();
                    
                    ui.label(format!("Page {} of {} ({} labels)", self.label_page, total_pages.max(1), total_labels));
                    
                    if ui.add_enabled(self.label_page > 1, egui::Button::new("◀ Prev")).clicked() {
                        self.label_page = self.label_page.saturating_sub(1);
                    }
                    if ui.add_enabled(self.label_page < total_pages, egui::Button::new("Next ▶")).clicked() {
                        self.label_page += 1;
                    }
                    
                    ui.separator();
                    ui.label("Per page:");
                    egui::ComboBox::from_id_salt("page_size")
                        .selected_text(self.labels_per_page.to_string())
                        .show_ui(ui, |ui| {
                            for &size in &[10, 25, 50, 100] {
                                if ui.selectable_label(self.labels_per_page == size, size.to_string()).clicked() {
                                    self.labels_per_page = size;
                                    self.label_page = 1;
                                }
                            }
                        });
                });
                
                ui.separator();
                
                // Labels list â€” only the current page, in a bounded scroll area
                let list_height = (ui.available_height() - 36.0).clamp(160.0, 280.0);
                let start_index = (self.label_page.saturating_sub(1)) * per_page;
                let page_labels: Vec<(u64, String)> = self.label_list_cache
                    .iter()
                    .skip(start_index)
                    .take(per_page)
                    .cloned()
                    .collect();
                
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    egui::Grid::new("labels_grid")
                        .num_columns(3)
                        .striped(true)
                        .spacing([15.0, 4.0])
                        .min_col_width(150.0)
                        .show(ui, |ui| {
                            ui.strong("Hash");
                            ui.strong("Label");
                            ui.strong("Actions");
                            ui.end_row();
                            
                            for (hash, label) in &page_labels {
                                ui.monospace(format!("0x{:X}", hash));
                                
                                let mut edit_label = label.clone();
                                if ui.text_edit_singleline(&mut edit_label).changed() {
                                    self.status_message = format!("Label editing: {}", edit_label);
                                }
                                
                                ui.horizontal(|ui| {
                                    if ui.small_button("Copy").clicked() {
                                        ui.ctx().copy_text(label.clone());
                                        self.status_message = format!("Copied: {}", label);
                                    }
                                    if ui.small_button("Delete").clicked() {
                                        self.status_message = format!("Delete label: {}", label);
                                    }
                                });
                                
                                ui.end_row();
                            }
                        });
                });
                
                ui.separator();
                
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        self.show_label_editor = false;
                        // Rebuild tree when editor is closed to show any updated labels
                        self.param_file.rebuild_tree_with_labels();
                        self.mark_tree_dirty();
                    }
                });
            });
            
        // Handle window close button (X)
        if !open {
            self.show_label_editor = false;
            // Rebuild tree when editor is closed to show any updated labels
            self.param_file.rebuild_tree_with_labels();
            self.mark_tree_dirty();
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        // When a text box is focused (renaming, editing a value, the label editor,
        // a filter box, etc.) Ctrl+C / Ctrl+X / Ctrl+V must operate on the text
        // inside that box, NOT on the selected tree node. egui's TextEdit handles
        // text clipboard natively, so we simply skip all node-level clipboard and
        // navigation shortcuts while any text box has focus.
        let text_box_focused = self.editing_value.is_some() || ctx.memory(|m| m.focused().is_some());
        
        // Try to handle clipboard operations using egui's events
        ctx.input_mut(|i| {
            // Check for copy/paste events that egui might have processed.
            // Only treat them as node copy/paste when no text box is focused.
            if !text_box_focused && !i.events.is_empty() {
                for event in &i.events {
                    match event {
                        egui::Event::Copy => {
                            if let Some(selected_path) = &self.selected_node {
                                self.clipboard = Some(selected_path.clone());
                                self.clipboard_data = self.find_node_by_path(selected_path).cloned();
                                self.cut_mode = false;
                                let has_data = self.clipboard_data.is_some();
                                self.status_message = format!("Copied node via event: {} (data: {})", selected_path, has_data);
                            } else {
                                self.status_message = "Copy event: No node selected".to_string();
                            }
                            return;
                        }
                        egui::Event::Paste(_text) => {
                            // Handle paste using our internal clipboard
                            if let (Some(clipboard_data), Some(selected_path)) = (self.clipboard_data.clone(), self.selected_node.clone()) {
                                if self.paste_node_into(&selected_path, clipboard_data) {
                                    let action = if self.cut_mode { "Moved" } else { "Pasted" };
                                    self.status_message = format!("{} node into {} via paste event", action, selected_path);
                                    
                                    // For cut operations, clear the clipboard since it's now moved
                                    if self.cut_mode {
                                        self.clipboard = None;
                                        self.clipboard_data = None;
                                        self.cut_mode = false;
                                    }
                                    
                                    // Rebuild tree items to show changes
                                    self.build_tree_items();
                                } else {
                                    self.status_message = format!("Failed to paste into {} via paste event", selected_path);
                                }
                            } else {
                                self.status_message = "Paste event: Nothing to paste".to_string();
                            }
                            return;
                        }
                        _ => {}
                    }
                }
            }
            

            
            // Try alternative shortcut detection using egui's shortcut system
            if !text_box_focused {
                // Try using egui's shortcut detection
                if i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::V)) ||
                   i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::V)) {
                    // Handle paste logic here
                    if let (Some(clipboard_data), Some(selected_path)) = (self.clipboard_data.clone(), self.selected_node.clone()) {
                        if self.paste_node_into(&selected_path, clipboard_data.clone()) {
                            let action = if self.cut_mode { "Moved" } else { "Pasted" };
                            let paste_type = match (&clipboard_data.value, self.find_node_by_path(&selected_path).map(|n| &n.value)) {
                                (ParamValue::Struct(_), Some(ParamValue::Struct(_))) => "fields",
                                (ParamValue::List(_), Some(ParamValue::List(_))) => "items",
                                _ => "node"
                            };
                            self.status_message = format!("{} {} into {} with Ctrl+V (shortcut)", action, paste_type, selected_path);
                            
                            if self.cut_mode {
                                self.clipboard = None;
                                self.clipboard_data = None;
                                self.cut_mode = false;
                            }
                            self.build_tree_items();
                        } else {
                            self.status_message = format!("Failed to paste into {} with Ctrl+V (shortcut)", selected_path);
                        }
                    } else {
                        self.status_message = "Ctrl+V (shortcut): Nothing to paste".to_string();
                    }
                }
                
                if i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Insert)) ||
                   i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::ALT, egui::Key::Insert)) {
                    // Handle paste logic here
                    if let (Some(clipboard_data), Some(selected_path)) = (self.clipboard_data.clone(), self.selected_node.clone()) {
                        if self.paste_node_into(&selected_path, clipboard_data.clone()) {
                            let action = if self.cut_mode { "Moved" } else { "Pasted" };
                            let paste_type = match (&clipboard_data.value, self.find_node_by_path(&selected_path).map(|n| &n.value)) {
                                (ParamValue::Struct(_), Some(ParamValue::Struct(_))) => "fields",
                                (ParamValue::List(_), Some(ParamValue::List(_))) => "items",
                                _ => "node"
                            };
                            self.status_message = format!("{} {} into {} with Shift+Insert (shortcut)", action, paste_type, selected_path);
                            
                            if self.cut_mode {
                                self.clipboard = None;
                                self.clipboard_data = None;
                                self.cut_mode = false;
                            }
                            self.build_tree_items();
                        } else {
                            self.status_message = format!("Failed to paste into {} with Shift+Insert (shortcut)", selected_path);
                        }
                    } else {
                        self.status_message = "Shift+Insert (shortcut): Nothing to paste".to_string();
                    }
                }
            }
            
            // Only handle node-level shortcuts when no text box is focused
            if !text_box_focused {
                let ctrl = i.modifiers.ctrl;
                
                // Arrow key navigation
                if i.key_pressed(egui::Key::ArrowUp) {
                    self.navigate_up();
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.navigate_down();
                }
                if i.key_pressed(egui::Key::ArrowLeft) {
                    self.navigate_left();
                }
                if i.key_pressed(egui::Key::ArrowRight) {
                    self.navigate_right();
                }
                
                // ENTER - Open data grid for selected node (expand/collapse)
                if i.key_pressed(egui::Key::Enter) {
                    if let Some(selected_path) = &self.selected_node {
                        if self.expanded_nodes.contains(selected_path) {
                            self.expanded_nodes.remove(selected_path);
                            self.status_message = "Collapsed node".to_string();
                        } else {
                            self.expanded_nodes.insert(selected_path.clone());
                            self.status_message = "Expanded node".to_string();
                        }
                    }
                }
                
                // DEL - Delete the node
                if i.key_pressed(egui::Key::Delete) {
                    if let Some(selected_path) = self.selected_node.clone() {
                        if self.delete_node(&selected_path) {
                            self.status_message = format!("Deleted node: {}", selected_path);
                            // Clear selection since the node no longer exists
                            self.selected_node = None;
                            self.selected_index = None;
                            // Rebuild tree items
                            self.build_tree_items();
                        } else {
                            self.status_message = format!("Failed to delete node: {}", selected_path);
                        }
                    }
                }
                
                // CTRL + C - Copy the node (try multiple approaches)
                if (ctrl && i.key_pressed(egui::Key::C)) || 
                   (ctrl && i.modifiers.shift && i.key_pressed(egui::Key::C)) ||
                   (ctrl && i.key_pressed(egui::Key::Insert)) {
                    if let Some(selected_path) = &self.selected_node {
                        self.clipboard = Some(selected_path.clone());
                        let node_data = self.find_node_by_path(selected_path).cloned();
                        

                        
                        self.clipboard_data = node_data;
                        self.cut_mode = false;
                        let shortcut = if i.key_pressed(egui::Key::Insert) { "Ctrl+Insert" } 
                                      else if i.modifiers.shift { "Ctrl+Shift+C" } 
                                      else { "Ctrl+C" };
                        self.status_message = format!("Copied node with {}: {}", shortcut, selected_path);
                    } else {
                        self.status_message = "No node selected to copy".to_string();
                    }
                }
                
                // CTRL + X - Cut the node
                if ctrl && i.key_pressed(egui::Key::X) {
                    if let Some(selected_path) = self.selected_node.clone() {
                        // First copy the node data
                        if let Some(node_data) = self.find_node_by_path(&selected_path).cloned() {
                        self.clipboard = Some(selected_path.clone());
                            self.clipboard_data = Some(node_data);
                            self.cut_mode = true;
                            
                            // Then delete the node from its current location
                            if self.delete_node(&selected_path) {
                                self.status_message = format!("Cut node: {}", selected_path);
                                // Clear selection since the node no longer exists
                                self.selected_node = None;
                                self.selected_index = None;
                                // Rebuild tree items
                                self.build_tree_items();
                            } else {
                                self.status_message = format!("Failed to cut node: {}", selected_path);
                                // Clear clipboard if cut failed
                                self.clipboard = None;
                                self.clipboard_data = None;
                                self.cut_mode = false;
                            }
                        } else {
                            self.status_message = format!("Could not find node to cut: {}", selected_path);
                        }
                    }
                }
                
                // Note: Ctrl+V paste is handled by the egui shortcut system above
                // Only handle the alternative paste shortcuts here that egui might not catch
                if (i.modifiers.shift && i.key_pressed(egui::Key::V) && !ctrl) ||  // Shift+V (when Ctrl+V is detected as Shift+V)
                   (i.modifiers.alt && i.key_pressed(egui::Key::Insert)) {  // Alt+Insert (when Shift+Insert is detected as Alt+Insert)
                    let shortcut = if i.modifiers.alt && i.key_pressed(egui::Key::Insert) { "Shift+Insert (detected as Alt+Insert)" }
                                  else { "Ctrl+V (detected as Shift+V)" };
                    
                    if let (Some(clipboard_data), Some(selected_path)) = (self.clipboard_data.clone(), self.selected_node.clone()) {
                        if self.paste_node_into(&selected_path, clipboard_data.clone()) {
                            let action = if self.cut_mode { "Moved" } else { "Pasted" };
                            let paste_type = match self.find_node_by_path(&selected_path).map(|n| &n.value) {
                                Some(ParamValue::Struct(_)) => "node into struct",
                                Some(ParamValue::List(_)) => "node into list",
                                _ => "node"
                            };
                            self.status_message = format!("{} {} {} with {}", action, paste_type, selected_path, shortcut);
                            
                            // For cut operations, clear the clipboard since it's now moved
                            if self.cut_mode {
                                self.clipboard = None;
                                self.clipboard_data = None;
                                self.cut_mode = false;
                            }
                            
                            // Rebuild tree items to show changes
                            self.build_tree_items();
                        } else {
                            self.status_message = format!("Failed to paste into {} with {}", selected_path, shortcut);
                        }
                    } else {
                        let has_clipboard = self.clipboard.is_some();
                        let has_data = self.clipboard_data.is_some();
                        let has_selection = self.selected_node.is_some();
                        self.status_message = format!("{}: Nothing to paste (clipboard: {}, data: {}, selection: {})", 
                            shortcut, has_clipboard, has_data, has_selection);
                    }
                }
                
                // CTRL + P - Paste the copied node into the parent
                if ctrl && i.key_pressed(egui::Key::P) {
                    if let (Some(clipboard_data), Some(selected_path)) = (self.clipboard_data.clone(), self.selected_node.clone()) {
                        if let Some(parent_path) = self.get_parent_path(&selected_path) {
                            // Generate a new name for the pasted node, preserving original names when possible
                            let mut new_clipboard_data = clipboard_data.clone();
                            let original_name = &clipboard_data.name;
                            let generated_name = self.generate_paste_name(&parent_path, original_name);
                            new_clipboard_data.name = generated_name.clone();
                            new_clipboard_data.hash = self.param_file.hash_labels.add_label_and_save(&new_clipboard_data.name, self.param_labels_path.as_deref());
                            

                            
                            if self.paste_node_into(&parent_path, new_clipboard_data) {
                                let action = if self.cut_mode { "Moved" } else { "Pasted" };
                                self.status_message = format!("{} node into parent of {}", action, selected_path);
                                
                                // For cut operations, clear the clipboard since it's now moved
                                if self.cut_mode {
                                    self.clipboard = None;
                                    self.clipboard_data = None;
                                    self.cut_mode = false;
                                }
                                
                                // Rebuild tree items to show changes
                                self.build_tree_items();
                            } else {
                                self.status_message = format!("Failed to paste into parent of {}", selected_path);
                            }
                        } else {
                            self.status_message = "Root node has no parent".to_string();
                        }
                    } else {
                        self.status_message = "Nothing to paste into parent".to_string();
                    }
                }
                
                // CTRL + D - Duplicate param on the same level
                if ctrl && i.key_pressed(egui::Key::D) {
                    if let Some(selected_path) = self.selected_node.clone() {
                        if let Some(node_to_duplicate) = self.find_node_by_path(&selected_path).cloned() {
                            if let Some(parent_path) = self.get_parent_path(&selected_path) {
                                // Generate a new name for the duplicated node
                                let mut new_node = node_to_duplicate.clone();
                                new_node.name = self.generate_sequential_name(&parent_path, &node_to_duplicate.name);
                                new_node.hash = self.param_file.hash_labels.add_label_and_save(&new_node.name, self.param_labels_path.as_deref());
                                
                                if self.paste_node_into(&parent_path, new_node) {
                                    self.status_message = format!("Duplicated node: {}", selected_path);
                                    // Rebuild tree items to show changes
                                    self.build_tree_items();
                                } else {
                                    self.status_message = format!("Failed to duplicate node: {}", selected_path);
                                }
                            } else {
                                self.status_message = "Cannot duplicate root node".to_string();
                            }
                        } else {
                            self.status_message = format!("Could not find node to duplicate: {}", selected_path);
                        }
                    }
                }
                
                // CTRL + S - Save changes to the current file (in place)
                if ctrl && i.key_pressed(egui::Key::S) {
                    if self.param_file.get_root().is_some() {
                        self.save_file();
                    } else {
                        self.status_message = "No file to save".to_string();
                    }
                }
                
                // CTRL + Z - Undo
                if ctrl && i.key_pressed(egui::Key::Z) {
                    if self.undo() {
                        // Undo was successful, status message is set by undo()
                    } else {
                        self.status_message = "Nothing to undo".to_string();
                    }
                }
                
                // CTRL + Y - Redo
                if ctrl && i.key_pressed(egui::Key::Y) {
                    if self.redo() {
                        // Redo was successful, status message is set by redo()
                    } else {
                        self.status_message = "Nothing to redo".to_string();
                    }
                }
                
                // F2 - Rename selected node
                if i.key_pressed(egui::Key::F2) {
                    if let Some(selected_path) = &self.selected_node {
                        if let Some(node) = self.find_node_by_path(selected_path) {
                            let name_edit_path = format!("{}_name", selected_path);
                            self.set_editing_value(Some((name_edit_path, node.name.clone())));
                            self.status_message = "Press Enter to confirm rename, Escape to cancel".to_string();
                        }
                    }
                }
                
                // F1 key removed - shortcuts are now always visible
            }
        });
    }

    /// Recursively collect all boolean field paths under a node
    fn collect_bool_paths_recursive(&self, node: &ParamNode, current_path: &str, paths: &mut Vec<(String, bool)>) {
        match &node.value {
            ParamValue::Bool(val) => {
                paths.push((current_path.to_string(), *val));
            },
            ParamValue::Struct(_) | ParamValue::List(_) => {
                for (i, child) in node.children.iter().enumerate() {
                    let child_path = format!("{}[{}]", current_path, i);
                    self.collect_bool_paths_recursive(child, &child_path, paths);
                }
            },
            _ => {}
        }
    }

    /// Recursively collect all boolean field paths from ParamValue data structure
    fn collect_bool_paths_from_value(&self, value: &ParamValue, current_path: &str, paths: &mut Vec<(String, bool)>) {
        match value {
            ParamValue::Bool(val) => {
                paths.push((current_path.to_string(), *val));
            },
            ParamValue::Struct(s) => {
                for (k, (_field_hash, field_value)) in s.fields.iter().enumerate() {
                    let field_path = format!("{}[{}]", current_path, k);
                    self.collect_bool_paths_from_value(field_value, &field_path, paths);
                }
            },
            ParamValue::List(l) => {
                for (k, item_value) in l.values.iter().enumerate() {
                    let item_path = format!("{}[{}]", current_path, k);
                    self.collect_bool_paths_from_value(item_value, &item_path, paths);
                }
            },
            _ => {}
        }
    }

    /// Recursively collect all boolean field paths from ParamValue, using display tree order for pathing
    fn collect_bool_paths_full(&self, value: &ParamValue, node: &ParamNode, current_path: &str, paths: &mut Vec<(String, bool)>) {
        match value {
            ParamValue::Bool(val) => {
                paths.push((current_path.to_string(), *val));
            },
            ParamValue::Struct(_) | ParamValue::List(_) => {
                for (i, child) in node.children.iter().enumerate() {
                    let child_path = format!("{}[{}]", current_path, i);
                    self.collect_bool_paths_full(&child.value, child, &child_path, paths);
                }
            },
            _ => {}
        }
    }

    /// Recursively collect all boolean field paths from ParamValue data structure, building paths correctly
    fn collect_bool_paths_from_raw_data(&self, value: &ParamValue, current_path: &str, paths: &mut Vec<(String, bool)>) {
        match value {
            ParamValue::Bool(val) => {
                paths.push((current_path.to_string(), *val));
            },
            ParamValue::Struct(s) => {
                for (k, (_field_hash, field_value)) in s.fields.iter().enumerate() {
                    let field_path = format!("{}[{}]", current_path, k);
                    self.collect_bool_paths_from_raw_data(field_value, &field_path, paths);
                }
            },
            ParamValue::List(l) => {
                for (k, item_value) in l.values.iter().enumerate() {
                    let item_path = format!("{}[{}]", current_path, k);
                    self.collect_bool_paths_from_raw_data(item_value, &item_path, paths);
                }
            },
            _ => {}
        }
    }

    /// Process parameters in batches to avoid memory buildup
    fn process_parameters_in_batches(&mut self, target: bool, operation_name: &str) {
        let field_names = ["bones", "connections", "collisions"];
        let mut processed_count = 0;
        let mut total_count = 0;
        let mut updates_to_perform = Vec::new();
        
        // First, count total parameters to process
        if let Some(root) = self.param_file.get_root() {
            for (_i, child) in root.children.iter().enumerate() {
                if field_names.contains(&child.name.as_str()) {
                    if let ParamValue::List(list) = &child.value {
                        total_count += list.values.len();
                    }
                }
            }
        }
        
        // Collect all the data we need first
        let mut data_to_process = Vec::new();
        if let Some(root_mut) = self.param_file.root.as_mut() {
            for (i, child) in root_mut.children.iter_mut().enumerate() {
                if field_names.contains(&child.name.as_str()) {
                    let list_path = format!("root[{}]", i);
                    
                    // Force expand and build this specific list
                    self.expanded_nodes.insert(list_path.clone());
                    child.ensure_children_built(&self.param_file.hash_labels);
                    
                    // Process each item in the list
                    if let ParamValue::List(list) = &child.value {
                        for (j, _item_value) in list.values.iter().enumerate() {
                            let item_path = format!("{}[{}]", list_path, j);
                            
                            // Build children for this specific item
                            if j < child.children.len() {
                                child.children[j].ensure_children_built(&self.param_file.hash_labels);
                                
                                // Store the data we need to process
                                data_to_process.push((item_path.clone(), child.children[j].value.clone()));
                                
                                // Clear children after processing to free memory
                                child.children[j].children.clear();
                                child.children[j].children_built = false;
                                
                                processed_count += 1;
                                self.status_message = format!("{}: Processed {}/{} parameters", operation_name, processed_count, total_count);
                            }
                        }
                    }
                    
                    // Clear the list's children after processing
                    child.children.clear();
                    child.children_built = false;
                    
                    // Remove from expanded nodes to free memory
                    self.expanded_nodes.remove(&list_path);
                }
            }
        }
        
        // Now process the collected data
        for (item_path, item_value) in data_to_process {
            let mut bool_paths = Vec::new();
            self.collect_bool_paths_from_raw_data(&item_value, &item_path, &mut bool_paths);
            
            // Add to updates list
            for (field_path, _val) in bool_paths {
                updates_to_perform.push(field_path);
            }
        }
        
        // Now perform all the updates
        for field_path in updates_to_perform {
            self.update_node_value_with_undo(&field_path, ParamValue::Bool(target));
        }
        
        let state = if target { "enabled" } else { "disabled" };
        self.status_message = format!("{}: Completed! Set all {} parameters to {}", operation_name, processed_count, state);
    }

    /// Helper to recursively set all bool fields in bones, connections, collisions lists to a specific value
    fn bulk_set_enable_disable(&mut self, _parent_path: &str, _node: &ParamNode, target: bool) {
        // Use batch processing for better memory management
        self.process_parameters_in_batches(target, if target { "Enable" } else { "Disable" });
    }

    /// Recursively set all boolean fields in the entire parameter tree to a specific value
    fn set_all_bools_in_tree(&mut self, node: &ParamNode, path: &str, target: bool) {
        // Work directly with the raw data structure
        let mut all_bool_paths = Vec::new();
        self.collect_bool_paths_from_raw_data(&node.value, path, &mut all_bool_paths);
        for (field_path, _val) in all_bool_paths {
            self.update_node_value_with_undo(&field_path, ParamValue::Bool(target));
        }
    }

    /// Clear children of a specific node to free memory
    fn clear_node_children(&mut self, path: &str) {
        if let Some(root) = self.param_file.root.as_mut() {
            let indices = if path == "root" {
                vec![]
            } else {
                let path_parts: Vec<&str> = path.split("[").skip(1).collect();
                let mut indices = Vec::new();
                for part in path_parts {
                    let index_str = part.trim_end_matches(']');
                    if let Ok(index) = index_str.parse::<usize>() {
                        indices.push(index);
                    }
                }
                indices
            };
            
            if let Some(target_node) = root.get_child_mut(&indices) {
                target_node.children.clear();
                target_node.children_built = false;
            }
        }
    }

    /// Update selection and clear previous node's children
    fn update_selection(&mut self, new_path: String) {
        // Clear children of previously selected node
        if let Some(prev_path) = self.previous_selected_node.clone() {
            if prev_path != new_path {
                self.clear_node_children(&prev_path);
            }
        }
        
        // Update selection tracking
        self.previous_selected_node = self.selected_node.clone();
        self.selected_node = Some(new_path);
        self.mark_tree_dirty();
    }

    /// Check if tree should be rebuilt based on cooldown and dirty state
    fn should_rebuild_tree(&self) -> bool {
        self.tree_items_dirty && 
        (self.frame_count - self.last_tree_rebuild_frame) >= self.tree_rebuild_cooldown
    }
    
    /// Mark tree as dirty to trigger rebuild
    fn mark_tree_dirty(&mut self) {
        self.tree_items_dirty = true;
        // Clear cache when tree structure changes
        self.node_cache.clear();
    }
    
    /// Get cached node or find and cache it
    fn get_cached_node(&mut self, path: &str) -> Option<&ParamNode> {
        // Check cache first
        if self.node_cache.contains_key(path) {
            return self.node_cache.get(path);
        }
        
        // Find node and cache it
        if let Some(node) = self.find_node_by_path(path) {
            self.node_cache.insert(path.to_string(), node.clone());
            return self.node_cache.get(path);
        }
        
        None
    }
    
    /// Clear node cache when structure changes
    fn clear_node_cache(&mut self) {
        self.node_cache.clear();
    }

    /// Optimized tree rendering with virtual scrolling for large trees
    fn show_tree_node_virtual(&mut self, ui: &mut egui::Ui, node: &ParamNode, path: String, depth: usize) {
        // Only render if node is visible (basic culling)
        if depth > 10 {
            return; // Limit depth to prevent excessive recursion
        }
        
        let is_expanded = self.expanded_nodes.contains(&path);
        let is_selected = self.selected_node.as_ref() == Some(&path);
        let is_keyboard_selected = self.selected_index
            .and_then(|idx| self.tree_items.get(idx))
            .map(|selected_path| selected_path == &path)
            .unwrap_or(false);
        
        // Create the tree node header with optimized rendering
        let response = if node.is_expandable() {
            let icon = if is_expanded { "▼" } else { "▶" };
            ui.horizontal(|ui| {
                if ui.button(icon).clicked() {
                    if is_expanded {
                        self.expanded_nodes.remove(&path);
                    } else {
                        self.expanded_nodes.insert(path.clone());
                    }
                    self.mark_tree_dirty();
                }
                
                let type_icon = match &node.value {
                    ParamValue::Struct(_) => "📁",
                    ParamValue::List(_) => "📋",
                    _ => "📄",
                };
                
                ui.label(type_icon);
                
                // Optimize label display - avoid string allocations
                let label = if node.name.is_empty() || node.name.starts_with("0x") {
                    format!("0x{:X}", node.hash)
                } else {
                    // Truncate long names for tree display
                    if node.name.len() > 25 {
                        format!("{}...", &node.name[..22])
                    } else {
                        node.name.clone()
                    }
                };
                
                let label_response = ui.selectable_label(is_selected || is_keyboard_selected, label);
                
                // Add visual indication for keyboard selection
                if is_keyboard_selected && !is_selected {
                    let rect = label_response.rect;
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, egui::Color32::YELLOW), egui::StrokeKind::Inside);
                }
                
                label_response
            }).inner
        } else {
            ui.horizontal(|ui| {
                ui.add_space(20.0); // Indent for leaf nodes
                ui.label("📄");
                
                let label = if node.name.is_empty() || node.name.starts_with("0x") {
                    format!("0x{:X}", node.hash)
                } else {
                    // Truncate long names for tree display
                    if node.name.len() > 20 {
                        format!("{}...", &node.name[..17])
                    } else {
                        node.name.clone()
                    }
                };
                
                // Simplified display for leaf nodes - just name and type
                let display_text = format!("{} ({})", label, node.get_type_name());
                
                let label_response = ui.selectable_label(is_selected || is_keyboard_selected, display_text);
                
                // Add visual indication for keyboard selection
                if is_keyboard_selected && !is_selected {
                    let rect = label_response.rect;
                    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, egui::Color32::YELLOW), egui::StrokeKind::Inside);
                }
                
                label_response
            }).inner
        };

        // Handle selection
        if response.clicked() {
            self.update_selection(path.clone());
        }

        // Show children if expanded - with depth limiting for performance
        if is_expanded && node.is_expandable() && depth < 8 {
            ui.indent(egui::Id::new(format!("{}_indent", path)), |ui| {
                for (i, child) in node.children.iter().enumerate() {
                    let child_path = format!("{}[{}]", path, i);
                    self.show_tree_node_virtual(ui, child, child_path, depth + 1);
                }
            });
        }
    }

    /// Optimize expensive operations by throttling them based on frame count
    fn should_perform_expensive_operation(&self, operation_type: &str) -> bool {
        // Different operations have different throttling requirements
        match operation_type {
            "tree_rebuild" => (self.frame_count - self.last_tree_rebuild_frame) >= self.tree_rebuild_cooldown,
            "node_lookup" => self.frame_count % 2 == 0, // Only every other frame
            "cache_update" => self.frame_count % 5 == 0, // Every 5 frames
            _ => true,
        }
    }
    
    /// Batch multiple operations to reduce per-frame overhead
    fn batch_operations<F>(&mut self, operation: F) where F: FnOnce(&mut Self) {
        // Perform the operation immediately but mark for potential batching
        operation(self);
    }
}

impl eframe::App for PrcEditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Increment frame counter for performance tracking
        self.frame_count += 1;
        
        // Performance optimization: Only rebuild tree when necessary
        if self.should_rebuild_tree() {
            self.build_tree_items();
            self.last_tree_rebuild_frame = self.frame_count;
            self.tree_items_dirty = false;
        }
        
        // Handle keyboard shortcuts
        self.handle_keyboard_shortcuts(&ctx);
        self.poll_crack_job(&ctx);

        if self.flip_preview.tab == MainTab::FlipPreview {
            ctx.request_repaint();
        }

        let wgpu_state = frame.wgpu_render_state();
        
        // Status bar at bottom using bottom panel - create this FIRST so main content knows about it
        egui::Panel::bottom("status_panel")
            .resizable(false)
            .min_size(25.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Status:");
                    ui.label(&self.status_message);
                    
                    // Show paste buttons for testing
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        
                        // Add paste button for testing
                        if let Some(_) = &self.clipboard_data {
                            if ui.button("Paste").clicked() {
                                if let Some(selected_path) = self.selected_node.clone() {
                                    if let Some(clipboard_data) = self.clipboard_data.clone() {
                                        if self.paste_node_into(&selected_path, clipboard_data.clone()) {
                                            let action = if self.cut_mode { "Moved" } else { "Pasted" };
                                            let paste_type = match self.find_node_by_path(&selected_path).map(|n| &n.value) {
                                                Some(ParamValue::Struct(_)) => "node into struct",
                                                Some(ParamValue::List(_)) => "node into list",
                                                _ => "node"
                                            };
                                            self.status_message = format!("{} {} {} via button", action, paste_type, selected_path);
                                            
                                            if self.cut_mode {
                                                self.clipboard = None;
                                                self.clipboard_data = None;
                                                self.cut_mode = false;
                                            }
                                            self.mark_tree_dirty();
                                        } else {
                                            self.status_message = format!("Failed to paste into {} via button", selected_path);
                                        }
                                    }
                                } else {
                                    self.status_message = "No node selected for paste".to_string();
                                }
                            }
                        }
                        
                        // Show clipboard status
                        if let Some(clipboard_path) = &self.clipboard {
                            let mode = if self.cut_mode { "Cut" } else { "Copy" };
                            let has_data = self.clipboard_data.is_some();
                            ui.label(&format!("Clipboard: {} {} (data: {})", mode, clipboard_path, has_data));
                        }
                        
                        // Show undo/redo stack info
                        ui.label(&format!("Undo: {} | Redo: {}", self.undo_stack.len(), self.redo_stack.len()));
                        
                        // Show labels count and file path
                        if let Some(path) = &self.param_labels_path {
                            let filename = std::path::Path::new(path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown");
                            ui.label(&format!("Labels: {} ({})", self.param_file.hash_labels.len(), filename));
                        } else {
                            ui.label(&format!("Labels: {} (no file)", self.param_file.hash_labels.len()));
                        }
                    });
                });
            });
        
        // Main content area - now it knows about the status bar space
        egui::CentralPanel::default().show(ui, |ui| {
            // Menu bar
            self.show_menu_bar(&ctx, ui);
            
            ui.separator();

            show_tab_bar(
                ui,
                &mut self.flip_preview,
                &self.param_file,
                self.current_file_path.as_deref(),
                self.param_file.get_root().is_some() && self.crack_job.is_none(),
            );
            ui.separator();

            match self.flip_preview.tab {
                MainTab::Editor => self.show_main_content(ui),
                MainTab::FlipPreview => {
                    show_flip_preview(
                        ui,
                        &mut self.flip_preview,
                        &mut self.param_file,
                        self.current_file_path.as_deref(),
                        &mut self.status_message,
                        wgpu_state,
                    );
                    if self.flip_preview.param_dirty {
                        self.mark_tree_dirty();
                        self.flip_preview.param_dirty = false;
                    }
                }
            }
            if self.flip_preview.request_crack {
                self.flip_preview.request_crack = false;
                self.crack_open_file();
            }
        });
        
        // Show label editor window if open
        self.show_label_editor_window(&ctx);
        update::show_update_windows(
            &ctx,
            &mut self.release_info,
            &self.update_download,
            self.auto_download_updates,
            &mut self.update_status_message,
        );
    }

    fn on_exit(&mut self) {
        update::save_update_check_time(&self.release_info);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Opaque dark fill. The eframe default is semi-transparent, which lets the
        // white Win32 window background flash through on startup.
        [12.0 / 255.0, 12.0 / 255.0, 12.0 / 255.0, 1.0]
    }
}
