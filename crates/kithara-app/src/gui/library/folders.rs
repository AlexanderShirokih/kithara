use std::path::PathBuf;

use kithara::platform::{thread, tokio::sync::mpsc::UnboundedSender};
use rfd::FileDialog;
use tracing::debug;

use super::explorer::Found;

/// The folders added to Music Folders, in the order added.
#[derive(Default, fieldwork::Fieldwork)]
#[fieldwork(opt_in)]
pub(super) struct MusicFolders {
    #[field(get = list, vis = "pub(super)")]
    folders: Vec<PathBuf>,
}

impl MusicFolders {
    /// Adds `folder`; one already added keeps its place.
    pub(super) fn add(&mut self, folder: PathBuf) {
        if !self.folders.contains(&folder) {
            self.folders.push(folder);
        }
    }
}

/// Asks for a folder to add; Explorer takes the answer on a later tick.
#[derive(Clone)]
pub(in crate::gui) struct FolderPicker {
    found: UnboundedSender<Found>,
}

impl FolderPicker {
    pub(super) const fn new(found: UnboundedSender<Found>) -> Self {
        Self { found }
    }

    /// Opens the system folder dialog on its own thread.
    pub(in crate::gui) fn open(&self) {
        let picker = self.clone();
        drop(thread::spawn_named(
            "kithara-app-folder-picker",
            move || {
                picker.picked(FileDialog::new().pick_folder());
            },
        ));
    }

    /// Hands what the dialog answered to Explorer; `None` is a cancelled dialog.
    pub(in crate::gui::library) fn picked(&self, folder: Option<PathBuf>) {
        let Some(folder) = folder else {
            return;
        };
        if self.found.send(Found::Picked(folder)).is_err() {
            debug!("the library closed before the picked folder arrived");
        }
    }
}
