use std::{cell::OnceCell, collections::BTreeMap};

use kithara::ui::render::{Node, ReadValue, Scope, TableCell, TableRow, TreeRow};

use super::value::{Value, impl_child_node};
use crate::gui::library::Library;

/// Answers the tree, the hidden pages and whether Add folder is hidden.
pub(super) struct LibraryNode<'a> {
    library: &'a Library,
    tree: Vec<TreeRow<'a>>,
    /// No folder picker answers Add folder.
    add_folder_hidden: bool,
}

impl<'a> LibraryNode<'a> {
    pub(super) fn new(library: &'a Library, add_folder_hidden: bool) -> Self {
        Self {
            library,
            add_folder_hidden,
            tree: library.tree(),
        }
    }
}

impl<'a, 'b: 'a> Node<'a> for &'a LibraryNode<'b> {
    fn child(&self, segment: &str, _scope: Scope<'_>) -> Option<Box<dyn Node<'a> + 'a>> {
        let tree: &'a [TreeRow<'a>] = &self.tree;
        let node: Box<dyn Node<'a> + 'a> = match segment {
            "tree" => Box::new(Value(ReadValue::Tree(tree))),
            "page" => Box::new(PageNode(self.library)),
            "add_folder" => Box::new(AddFolderNode(self.add_folder_hidden)),
            _ => return None,
        };
        Some(node)
    }
}

#[derive(Clone, Copy)]
struct PageNode<'a>(&'a Library);

impl_child_node!(PageNode<'a>, |this, segment, scope| {
    match segment {
        "hidden" => Some(Box::new(Value(ReadValue::Bool(
            this.0.page_hidden(scope.get("source")?)?,
        )))),
        _ => None,
    }
});

#[derive(Clone, Copy)]
struct AddFolderNode(bool);

impl_child_node!(AddFolderNode, |this, segment, _scope| {
    match segment {
        "hidden" => Some(Box::new(Value(ReadValue::Bool(this.0)))),
        _ => None,
    }
});

/// Answers each source under its own key, building its rows on first read.
pub(super) struct SourcesNode<'a> {
    library: &'a Library,
    rows: Vec<OnceCell<Vec<TableRow<'a>>>>,
    bpms: &'a BTreeMap<String, String>,
}

impl<'a> SourcesNode<'a> {
    pub(super) fn new(library: &'a Library, bpms: &'a BTreeMap<String, String>) -> Self {
        Self {
            library,
            bpms,
            rows: library.sources().map(|_| OnceCell::new()).collect(),
        }
    }
}

impl<'a, 'b: 'a> Node<'a> for &'a SourcesNode<'b> {
    fn child(&self, segment: &str, scope: Scope<'_>) -> Option<Box<dyn Node<'a> + 'a>> {
        let id = scope.get("source")?;
        let (at, source) = self
            .library
            .sources()
            .enumerate()
            .find(|(_, source)| source.id() == id)?;
        if matches!(segment, "columns" | "column") {
            return Some(Box::new(ColumnsNode {
                library: self.library,
                at,
            }));
        }
        let value = match segment {
            "rows" => {
                let rows: &'a [TableRow<'b>] = self.rows.get(at)?.get_or_init(|| {
                    source
                        .rows(self.library.selected_row(at))
                        .into_iter()
                        .map(|row| {
                            let bpm = row.drag().and_then(|url| {
                                self.bpms.get(&crate::catalog::canonical_source(url))
                            });
                            match bpm {
                                Some(bpm) if !row.cells().iter().any(|cell| cell.id() == "bpm") => {
                                    row.with_cell(TableCell::text("bpm", bpm))
                                }
                                _ => row,
                            }
                        })
                        .collect()
                });
                ReadValue::Table(rows)
            }
            "status" => ReadValue::Text(self.library.status_words(source.status())),
            _ => return None,
        };
        Some(Box::new(Value(value)))
    }
}

#[derive(Clone, Copy)]
struct ColumnsNode<'a> {
    library: &'a Library,
    at: usize,
}

impl_child_node!(ColumnsNode<'a>, |this, segment, scope| {
    if segment != "width" {
        return None;
    }
    if let Some(column) = scope.get("column") {
        Some(Box::new(Value(ReadValue::Scalar(
            this.library.column_width(this.at, column)?,
        ))))
    } else {
        Some(Box::new(WidthsNode(*this)))
    }
});

#[derive(Clone, Copy)]
struct WidthsNode<'a>(ColumnsNode<'a>);

impl_child_node!(WidthsNode<'a>, |this, segment, _scope| {
    Some(Box::new(Value(ReadValue::Scalar(
        this.0.library.column_width(this.0.at, segment)?,
    ))))
});
