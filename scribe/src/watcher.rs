use crate::config::FileTypeConfig;
use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

pub async fn watch_directory(
    watch_dir: PathBuf,
    tx: mpsc::Sender<PathBuf>,
    file_types: FileTypeConfig,
) -> Result<()> {
    let (notify_tx, mut notify_rx) = mpsc::channel(100);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = notify_tx.blocking_send(event);
            }
        },
        Config::default(),
    )?;

    watcher.watch(&watch_dir, RecursiveMode::Recursive)?;

    info!("Started watching directory: {:?}", watch_dir);

    while let Some(event) = notify_rx.recv().await {
        debug!("File system event: {:?}", event.kind);

        match event.kind {
            EventKind::Create(_) => {
                for path in event.paths {
                    info!("File created: {:?}", path);
                    process_path(&path, &tx, &file_types).await;
                }
            }
            EventKind::Modify(_) => {
                for path in event.paths {
                    info!("File modified: {:?}", path);
                    process_path(&path, &tx, &file_types).await;
                }
            }
            _ => {
                debug!("Ignoring event type: {:?}", event.kind);
            }
        }
    }

    Ok(())
}

async fn process_path(path: &Path, tx: &mpsc::Sender<PathBuf>, file_types: &FileTypeConfig) {
    if !path.is_file() {
        debug!("Path is not a file: {:?}", path);
        return;
    }

    if !is_supported_file(path, file_types) {
        debug!("File type not supported: {:?}", path);
        return;
    }

    if is_output_file(path) {
        debug!("Ignoring output file: {:?}", path);
        return;
    }

    let output_path = get_output_path(path);
    if output_path.exists() {
        debug!("File already processed (output exists): {:?}", path);
        return;
    }

    info!("Queueing file for processing: {:?}", path);
    if let Err(e) = tx.send(path.to_path_buf()).await {
        warn!("Failed to send file path to processor: {}", e);
    } else {
        info!("File successfully queued: {:?}", path);
    }
}

fn is_supported_file(path: &Path, file_types: &FileTypeConfig) -> bool {
    if let Some(extension) = path.extension() {
        let ext = extension.to_string_lossy().to_lowercase();
        file_types.audio_extensions.contains(&ext.to_string())
            || file_types.video_extensions.contains(&ext.to_string())
            || file_types.image_extensions.contains(&ext.to_string())
    } else {
        false
    }
}

fn is_output_file(path: &Path) -> bool {
    if let Some(stem) = path.file_stem() {
        stem.to_string_lossy().ends_with("-scribe")
    } else {
        false
    }
}

fn get_output_path(input_path: &Path) -> PathBuf {
    let parent = input_path.parent().unwrap_or(Path::new("."));
    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    parent.join(format!("{}-scribe.json", stem))
}
