use std::collections::BTreeMap;

use kithara::ui::{error::UiDocError, render::TreeRow, text::TextDoc};

use super::{BranchNode, LibrarySource, PageStatus, Registration, worded};

/// The library shell: sources, selection, expanded nodes and page states.
#[derive(fieldwork::Fieldwork)]
#[fieldwork(opt_in)]
pub(in crate::gui) struct Library {
    sources: Vec<Box<dyn LibrarySource>>,
    statuses: StatusWords,
    expanded: Vec<NodeAt>,
    selected: Option<NodeAt>,
    /// The key of the track each source's page has selected, by source.
    rows: Vec<Option<String>>,
    widths: Vec<BTreeMap<String, f64>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NodeAt {
    source: usize,
    key: String,
}

impl NodeAt {
    fn is(&self, source: usize, key: &str) -> bool {
        self.source == source && self.key == key
    }
}

/// The catalog's words for every page state, taken when the shell mounts.
struct StatusWords {
    empty: String,
    loading: String,
    unreadable: String,
}

impl StatusWords {
    fn new(text: &TextDoc) -> Result<Self, UiDocError> {
        let word = |status: PageStatus| {
            let Some(key) = Self::key(status) else {
                return Ok(String::new());
            };
            worded(text, key, "source.status")
        };
        Ok(Self {
            empty: word(PageStatus::Empty)?,
            loading: word(PageStatus::Loading)?,
            unreadable: word(PageStatus::Unreadable)?,
        })
    }

    /// The text-catalog key that words `status`; a ready page has no words.
    const fn key(status: PageStatus) -> Option<&'static str> {
        match status {
            PageStatus::Ready => None,
            PageStatus::Loading => Some("library.status.loading"),
            PageStatus::Empty => Some("library.status.empty"),
            PageStatus::Unreadable => Some("library.status.error"),
        }
    }

    fn of(&self, status: PageStatus) -> &str {
        match status {
            PageStatus::Ready => "",
            PageStatus::Loading => &self.loading,
            PageStatus::Empty => &self.empty,
            PageStatus::Unreadable => &self.unreadable,
        }
    }
}

struct Shown<'a> {
    source: usize,
    node: &'a BranchNode,
    row: TreeRow<'a>,
}

impl Library {
    /// Mounts the `registered` sources, selecting the first branch's first leaf.
    ///
    /// # Errors
    /// Returns [`UiDocError::UnknownTextKey`] when `text` lacks a label.
    pub(in crate::gui) fn new(
        registered: Vec<Registration>,
        text: &TextDoc,
    ) -> Result<Self, UiDocError> {
        let sources = registered
            .into_iter()
            .map(|source| source.build(text))
            .collect::<Result<Vec<_>, _>>()?;
        let mut library = Self {
            rows: vec![None; sources.len()],
            widths: vec![BTreeMap::new(); sources.len()],
            sources,
            statuses: StatusWords::new(text)?,
            expanded: Vec::new(),
            selected: None,
        };
        let mut path: Vec<String> = Vec::new();
        let mut node = library.sources.first().map(|source| source.branch());
        while let Some(branch) = node {
            path.push(branch.key.clone());
            node = branch.children.first();
        }
        let roots: Vec<NodeAt> = library
            .sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.branch().unlisted || !source.branch().children.is_empty())
            .map(|(source, mounted)| NodeAt {
                source,
                key: mounted.branch().key.clone(),
            })
            .collect();
        for root in roots {
            library.open(root);
        }
        if let Some(leaf) = path.pop() {
            for key in path.into_iter().skip(1) {
                library.open(NodeAt { source: 0, key });
            }
            library.select_at(NodeAt {
                source: 0,
                key: leaf,
            });
        }
        Ok(library)
    }

    /// Whether the page of source `id` stays hidden; `None` for an unknown id.
    pub(in crate::gui) fn page_hidden(&self, id: &str) -> Option<bool> {
        let source = self.sources.iter().position(|source| source.id() == id)?;
        Some(
            self.selected
                .as_ref()
                .is_none_or(|selected| selected.source != source),
        )
    }

    pub(in crate::gui) fn column_width(&self, source: usize, column: &str) -> Option<f64> {
        self.widths.get(source)?.get(column).copied()
    }

    pub(in crate::gui) fn set_column_width(&mut self, source: &str, column: &str, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        if let Some(at) = self
            .sources
            .iter()
            .position(|mounted| mounted.id() == source)
        {
            self.widths[at].insert(column.to_owned(), width);
        }
    }

    /// Selects the track the page of source `id` lists at `row`.
    pub(in crate::gui) fn select_row(&mut self, id: &str, row: usize) {
        let Some(at) = self.sources.iter().position(|source| source.id() == id) else {
            return;
        };
        let key = self.sources[at].row_key(row).map(str::to_owned);
        if let Some(selected) = self.rows.get_mut(at) {
            *selected = key;
        }
    }

    /// The key of the track the page of source `at` has selected.
    pub(in crate::gui) fn selected_row(&self, at: usize) -> Option<&str> {
        self.rows.get(at).and_then(Option::as_deref)
    }

    /// Selects the node drawn at `row` of the tree.
    pub(in crate::gui) fn select(&mut self, row: usize) {
        if let Some(at) = self.at(row) {
            self.select_at(at);
        }
    }

    pub(in crate::gui) fn sources(&self) -> impl Iterator<Item = &dyn LibrarySource> {
        self.sources.iter().map(AsRef::as_ref)
    }

    /// Lets every source take in what its background work finished.
    pub(in crate::gui) fn tick(&mut self) {
        for source in &mut self.sources {
            source.tick();
        }
    }

    /// Expands or collapses the node drawn at `row` of the tree.
    pub(in crate::gui) fn toggle(&mut self, row: usize) {
        let Some(at) = self.at(row) else {
            return;
        };
        if let Some(index) = self.expanded.iter().position(|open| *open == at) {
            self.expanded.swap_remove(index);
            return;
        }
        self.open(at);
    }

    /// The tree as drawn: every source's branch, open where it is expanded.
    pub(in crate::gui) fn tree(&self) -> Vec<TreeRow<'_>> {
        self.shown().into_iter().map(|shown| shown.row).collect()
    }

    /// The catalog's words for `status`.
    pub(in crate::gui) fn status_words(&self, status: PageStatus) -> &str {
        self.statuses.of(status)
    }

    /// The node drawn at `row` of the tree.
    fn at(&self, row: usize) -> Option<NodeAt> {
        self.shown().into_iter().nth(row).map(|shown| NodeAt {
            source: shown.source,
            key: shown.node.key.clone(),
        })
    }

    fn push<'a>(
        &'a self,
        out: &mut Vec<Shown<'a>>,
        source: usize,
        node: &'a BranchNode,
        depth: u8,
    ) {
        let open = self.expanded.iter().any(|at| at.is(source, &node.key));
        out.push(Shown {
            source,
            node,
            row: TreeRow {
                label: &node.label,
                depth,
                icon: node.icon,
                count: node.count,
                expanded: (node.unlisted || !node.children.is_empty()).then_some(open),
                muted: false,
                selected: self
                    .selected
                    .as_ref()
                    .is_some_and(|at| at.is(source, &node.key)),
            },
        });
        if open {
            for child in &node.children {
                self.push(out, source, child, depth.saturating_add(1));
            }
        }
    }

    fn open(&mut self, at: NodeAt) {
        if let Some(source) = self.sources.get_mut(at.source) {
            source.expand(&at.key);
        }
        self.expanded.push(at);
    }

    fn select_at(&mut self, at: NodeAt) {
        if let Some(source) = self.sources.get_mut(at.source) {
            source.select(&at.key);
        }
        if self.selected.as_ref() != Some(&at)
            && let Some(row) = self.rows.get_mut(at.source)
        {
            *row = None;
        }
        self.selected = Some(at);
    }

    fn shown(&self) -> Vec<Shown<'_>> {
        let mut out = Vec::new();
        for (at, source) in self.sources.iter().enumerate() {
            self.push(&mut out, at, source.branch(), 0);
        }
        out
    }
}
