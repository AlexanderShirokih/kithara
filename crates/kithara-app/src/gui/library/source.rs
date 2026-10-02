use kithara::ui::{
    error::UiDocError, ids::SourceUri, module::IconName, render::TableRow, text::TextDoc,
};

use super::PagesModule;

pub(in crate::gui) mod consts {
    /// The page the built-in sources draw, relative to the package root.
    pub(in crate::gui) const SOURCE_PAGE: &str = "modules/library/source-page.kmodule.ron";
}

/// A source's id and page module, known before the source is built.
#[derive(Clone, Copy, Debug)]
pub(in crate::gui) struct SourcePage {
    pub(in crate::gui) id: &'static str,
    /// Relative to the package root.
    pub(in crate::gui) page: &'static str,
}

/// Builds a registered source from the package's text catalog.
type Build = Box<dyn FnOnce(&TextDoc) -> Result<Box<dyn LibrarySource>, UiDocError>>;

/// A source to mount: its page and how to build it from the text catalog.
#[derive(fieldwork::Fieldwork)]
#[fieldwork(opt_in)]
pub(in crate::gui) struct Registration {
    #[field(get, vis = "pub(in crate::gui)")]
    page: SourcePage,
    build: Build,
}

impl Registration {
    pub(in crate::gui) fn new(
        page: SourcePage,
        build: impl FnOnce(&TextDoc) -> Result<Box<dyn LibrarySource>, UiDocError> + 'static,
    ) -> Self {
        Self {
            page,
            build: Box::new(build),
        }
    }

    /// Builds the source, its fixed labels worded by `text`.
    ///
    /// # Errors
    /// Returns [`UiDocError::UnknownTextKey`] when `text` lacks a label.
    pub(in crate::gui) fn build(
        self,
        text: &TextDoc,
    ) -> Result<Box<dyn LibrarySource>, UiDocError> {
        (self.build)(text)
    }
}

/// One branch of the library tree and the page its nodes show.
pub(in crate::gui) trait LibrarySource {
    /// The branch this source brings.
    fn branch(&self) -> &BranchNode;

    /// The branch node keyed `node` was expanded; fills unknown children.
    fn expand(&mut self, node: &str);

    fn id(&self) -> &str;

    /// The rows its page lists for the node selected last, the one keyed `selected` marked.
    fn rows(&self, selected: Option<&str>) -> Vec<TableRow<'_>>;

    /// The key of the track its page lists at `row`.
    fn row_key(&self, row: usize) -> Option<&str>;

    /// The node of its branch keyed `node` became the selected one.
    fn select(&mut self, node: &str);

    /// Where its page stands.
    fn status(&self) -> PageStatus;

    /// Takes in what its background work finished since the last tick.
    fn tick(&mut self);
}

/// Where a source's page stands; the shell words it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gui) enum PageStatus {
    /// It lists at least one row.
    Ready,
    Loading,
    /// It has no row to list.
    Empty,
    Unreadable,
}

/// One node of a source's branch.
pub(in crate::gui) struct BranchNode {
    /// Names the node to its source; unique within the branch.
    pub(in crate::gui) key: String,
    pub(in crate::gui) label: String,
    pub(in crate::gui) icon: IconName,
    pub(in crate::gui) count: Option<u32>,
    pub(in crate::gui) children: Vec<Self>,
    /// Its children are not known yet; it opens all the same.
    pub(in crate::gui) unlisted: bool,
}

impl BranchNode {
    /// A node with no count and no children.
    pub(in crate::gui) fn new(key: &str, label: String, icon: IconName) -> Self {
        Self {
            label,
            icon,
            key: key.to_owned(),
            count: None,
            children: Vec::new(),
            unlisted: false,
        }
    }
}

/// The words `text` has for `key`, which `path` names on the library.
///
/// # Errors
/// Returns [`UiDocError::UnknownTextKey`] when `text` has none.
pub(in crate::gui) fn worded(text: &TextDoc, key: &str, path: &str) -> Result<String, UiDocError> {
    text.get(key)
        .map(str::to_owned)
        .ok_or_else(|| UiDocError::UnknownTextKey {
            origin: SourceUri(PagesModule::PATH.to_owned()),
            key: key.to_owned(),
            path: path.to_owned(),
        })
}
