use bon::Builder;

use crate::expand::Binding;

/// The library tree, with a search field when it reads or writes a query.
#[derive(Builder, kithara_derive::Control)]
#[control(size = skin.tree.size)]
pub(crate) struct Tree<'a> {
    pub(crate) query: Option<&'a Binding>,
    pub(crate) search: bool,
    /// Whether a pressed chevron writes apart from its row.
    pub(crate) toggle: bool,
}

impl<'a> Tree<'a> {
    /// The search field it draws, if any.
    pub(crate) fn search_field(&self) -> Option<SearchField<&'a Binding>> {
        self.search.then_some(SearchField { query: self.query })
    }
}

/// A tree's search field and the endpoint its query is read from.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SearchField<Q> {
    pub(crate) query: Option<Q>,
}
