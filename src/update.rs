use chrono::{DateTime, Utc};
use eframe::egui::{self, Context, ScrollArea, Window};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

const GITHUB_OWNER: &str = "CrusherD2";
const GITHUB_REPO: &str = "prc-editor-rust";
const RELEASES_URL: &str = "https://github.com/CrusherD2/prc-editor-rust/releases";
const ISSUES_URL: &str = "https://github.com/CrusherD2/prc-editor-rust/issues";

pub struct LatestReleaseInfo {
    pub update_check_time: DateTime<Utc>,
    pub new_release: Option<NewRelease>,
    pub should_show_update: bool,
    pub check_failed: bool,
}

pub struct NewRelease {
    pub tag: String,
    pub release_notes: Option<String>,
    pub download_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum UpdateDownloadStatus {
    Idle,
    Downloading { tag: String },
    Completed { tag: String, path: PathBuf },
    Failed { message: String },
}

#[derive(Clone)]
pub struct UpdateDownload {
    status: Arc<Mutex<UpdateDownloadStatus>>,
}

impl Default for UpdateDownload {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(UpdateDownloadStatus::Idle)),
        }
    }
}

impl UpdateDownload {
    pub fn status(&self) -> UpdateDownloadStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn start(&self, tag: String, url: String) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        match &*status {
            UpdateDownloadStatus::Downloading { tag: current } if current == &tag => return,
            UpdateDownloadStatus::Completed {
                tag: current,
                path,
            } if current == &tag && path.exists() => {
                return;
            }
            _ => {}
        }

        if let Some(path) = update_download_path(&tag) {
            if path.is_file() {
                *status = UpdateDownloadStatus::Completed { tag, path };
                return;
            }
        }

        *status = UpdateDownloadStatus::Downloading { tag: tag.clone() };
        drop(status);

        let status = Arc::clone(&self.status);
        thread::spawn(move || {
            let result = download_update(&tag, &url);
            let mut status = status.lock().unwrap_or_else(|e| e.into_inner());
            *status = match result {
                Ok(path) => UpdateDownloadStatus::Completed { tag, path },
                Err(message) => UpdateDownloadStatus::Failed { message },
            };
        });
    }

    pub fn start_from_release(&self, release: &NewRelease) {
        if let Some(url) = &release.download_url {
            self.start(release.tag.clone(), url.clone());
        }
    }
}

pub fn check_for_updates() -> LatestReleaseInfo {
    check_for_updates_inner(false)
}

/// Always query GitHub, ignoring the once-per-day cache.
pub fn check_for_updates_now() -> LatestReleaseInfo {
    check_for_updates_inner(true)
}

fn check_for_updates_inner(force: bool) -> LatestReleaseInfo {
    let previous_update_check_time: Option<DateTime<Utc>> =
        std::fs::read_to_string(last_update_check_file())
            .unwrap_or_default()
            .parse()
            .ok();

    let update_check_time = Utc::now();
    let should_check_for_update =
        force || should_check_for_release(previous_update_check_time, update_check_time);

    let mut check_failed = false;
    let (new_release_tag, download_url, release_notes) = if should_check_for_update {
        match get_latest_release() {
            Some(release) => {
                let url = windows_exe_download_url(&release);
                let notes = release.body.filter(|body| !body.trim().is_empty());
                (Some(release.tag_name), url, notes)
            }
            None => {
                check_failed = true;
                (None, None, None)
            }
        }
    } else {
        (None, None, None)
    };

    let current_tag = env!("CARGO_PKG_VERSION");
    let (should_show_update, new_release) = match new_release_tag {
        Some(new_tag) => {
            let should_show_update = is_new_version(current_tag, &new_tag);
            let new_release = if should_show_update {
                Some(NewRelease {
                    tag: new_tag,
                    release_notes,
                    download_url,
                })
            } else {
                None
            };
            (should_show_update, new_release)
        }
        None => (false, None),
    };

    let info = LatestReleaseInfo {
        update_check_time,
        new_release,
        should_show_update,
        check_failed,
    };
    save_update_check_time(&info);
    info
}

pub fn save_update_check_time(info: &LatestReleaseInfo) {
    let path = last_update_check_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, info.update_check_time.to_string());
}

fn is_new_version(current_tag: &str, new_tag: &str) -> bool {
    let Some(current_tag_version) = parse_version(current_tag) else {
        return false;
    };
    let Some(new_tag_version) = parse_version(new_tag) else {
        return false;
    };
    new_tag_version > current_tag_version
}

fn parse_version(tag: &str) -> Option<semver::Version> {
    let tag = tag.trim().trim_start_matches('v').trim_start_matches('V');
    semver::Version::parse(tag).ok()
}

fn should_check_for_release(
    previous_update_check_time: Option<DateTime<Utc>>,
    current_time: DateTime<Utc>,
) -> bool {
    if let Some(previous_update_check_time) = previous_update_check_time {
        current_time.date_naive() > previous_update_check_time.date_naive()
    } else {
        true
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    body: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn github_user_agent() -> String {
    format!("Prc-Editor/{}", env!("CARGO_PKG_VERSION"))
}

fn get_latest_release() -> Option<GithubRelease> {
    let url = format!(
        "https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest"
    );
    let response = ureq::get(&url)
        .set("User-Agent", &github_user_agent())
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn windows_exe_download_url(release: &GithubRelease) -> Option<String> {
    let preferred = ["Prc-Editor.exe", "prc-editor-rust.exe"];
    for name in preferred {
        if let Some(asset) = release
            .assets
            .iter()
            .find(|asset| asset.name.eq_ignore_ascii_case(name))
        {
            return Some(asset.browser_download_url.clone());
        }
    }
    release
        .assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
        .map(|asset| asset.browser_download_url.clone())
}

fn app_data_dir() -> Option<PathBuf> {
    let mut dir = dirs::data_local_dir()?;
    dir.push("Prc-Editor");
    Some(dir)
}

fn last_update_check_file() -> PathBuf {
    app_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("update_time.txt")
}

fn auto_download_file() -> PathBuf {
    app_data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("auto_download_updates.txt")
}

pub fn load_auto_download_updates() -> bool {
    std::fs::read_to_string(auto_download_file())
        .map(|text| text.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn save_auto_download_updates(enabled: bool) {
    let path = auto_download_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, if enabled { "true" } else { "false" });
}

fn update_download_path(tag: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let safe_tag = tag.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    Some(dir.join(format!("Prc-Editor_{safe_tag}.exe")))
}

fn download_update(tag: &str, url: &str) -> Result<PathBuf, String> {
    let path = update_download_path(tag).ok_or_else(|| {
        "Couldn't determine where to save the update next to this program.".to_owned()
    })?;

    if path.is_file() {
        return Ok(path);
    }

    let response = ureq::get(url)
        .set("User-Agent", &github_user_agent())
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .map_err(|e| format!("Failed to download update: {e}"))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes)
        .map_err(|e| format!("Failed to read update download: {e}"))?;
    if bytes.is_empty() {
        return Err("Downloaded update was empty.".to_string());
    }
    std::fs::write(&path, bytes).map_err(|e| format!("Failed to save update to {path:?}: {e}"))?;
    Ok(path)
}

fn open_path(path: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

pub fn show_update_windows(
    ctx: &Context,
    release_info: &mut LatestReleaseInfo,
    download: &UpdateDownload,
    auto_download: bool,
    status_message: &mut Option<String>,
) {
    if release_info.should_show_update {
        if auto_download {
            if let Some(release) = &release_info.new_release {
                download.start_from_release(release);
            }
        }
        new_release_window(ctx, release_info, download);
    }
    update_status_window(ctx, status_message);
}

fn new_release_window(
    ctx: &Context,
    release_info: &mut LatestReleaseInfo,
    download: &UpdateDownload,
) {
    let Some(new_release) = &release_info.new_release else {
        return;
    };

    let status = download.status();
    if matches!(status, UpdateDownloadStatus::Downloading { .. }) {
        ctx.request_repaint();
    }

    Window::new("New Release Available")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut release_info.should_show_update)
        .show(ctx, |ui| {
            ui.label("A new release of PRC Editor is available!");
            ui.label(format!(
                "The latest version is {}. The current version is {}.",
                new_release.tag,
                env!("CARGO_PKG_VERSION")
            ));

            ui.add_space(8.0);
            match &status {
                UpdateDownloadStatus::Downloading { .. } => {
                    ui.label("Downloading update...");
                }
                UpdateDownloadStatus::Completed { path, .. } => {
                    ui.label("Update downloaded. Close PRC Editor and run this file:");
                    ui.monospace(path.display().to_string());
                    ui.horizontal(|ui| {
                        if ui.button("Open Folder").clicked() {
                            if let Some(folder) = path.parent() {
                                open_path(&folder.display().to_string());
                            }
                        }
                    });
                }
                UpdateDownloadStatus::Failed { message } => {
                    ui.label(message);
                    if new_release.download_url.is_some() && ui.button("Try Again").clicked() {
                        download.start_from_release(new_release);
                    }
                }
                UpdateDownloadStatus::Idle => {
                    ui.horizontal(|ui| {
                        if new_release.download_url.is_some() {
                            if ui.button("Download Update").clicked() {
                                download.start_from_release(new_release);
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new("Download Update"))
                                .on_hover_text("No Windows download is attached to this release.");
                        }
                    });
                }
            }

            ui.add_space(4.0);
            ui.label("You can also download the new version from here:");
            if ui.hyperlink(RELEASES_URL).clicked() {
                open_path(RELEASES_URL);
            }
            ui.add_space(8.0);

            ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                if let Some(release_notes) = &new_release.release_notes {
                    ui.label(release_notes);
                }
            });
        });
}

fn update_status_window(ctx: &Context, message: &mut Option<String>) {
    let Some(text) = message.clone() else {
        return;
    };

    let mut open = true;
    let mut close = false;
    Window::new("Check for Updates")
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .resizable(false)
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(text);
            ui.add_space(8.0);
            if ui.button("OK").clicked() {
                close = true;
            }
        });

    if !open || close {
        *message = None;
    }
}

pub fn apply_manual_update_check(
    info: LatestReleaseInfo,
    release_info: &mut LatestReleaseInfo,
    download: &UpdateDownload,
    auto_download: bool,
    status_message: &mut Option<String>,
) {
    if info.should_show_update {
        if auto_download {
            if let Some(release) = &info.new_release {
                download.start_from_release(release);
            }
        }
        *release_info = info;
        *status_message = None;
    } else if info.check_failed {
        release_info.update_check_time = info.update_check_time;
        release_info.check_failed = true;
        *status_message = Some(
            "Couldn't check for updates. Check your internet connection and try again.".to_owned(),
        );
    } else {
        *release_info = info;
        *status_message = Some(format!(
            "You're using the latest version ({}).",
            env!("CARGO_PKG_VERSION")
        ));
    }
}

pub fn open_releases_page() {
    open_path(RELEASES_URL);
}

pub fn open_issues_page() {
    open_path(ISSUES_URL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_release_version_comparison() {
        assert!(!is_new_version("0.10.10", "0.10.9"));
        assert!(!is_new_version("0.11.1", "0.9.2"));
        assert!(is_new_version("0.10.9", "0.10.10"));
        assert!(is_new_version("0.9.2", "0.11.1"));
        assert!(is_new_version("0.0.1", "0.1.0"));
        assert!(is_new_version("1.0.0", "v1.1.0"));
        assert!(!is_new_version("1.1.0", "v1.0.0"));
        assert!(!is_new_version("1.1.0", "v1.1.0"));
    }
}
