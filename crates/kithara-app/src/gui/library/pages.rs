use super::{Registration, SourcePage};

/// Mounts each source's page in one box, shown while it owns the selection.
#[derive(fieldwork::Fieldwork)]
#[fieldwork(opt_in, get)]
pub(in crate::gui) struct PagesModule {
    #[field(get, vis = "pub(in crate::gui)")]
    text: String,
}

impl PagesModule {
    /// Where the library layout includes it from, relative to the package root.
    pub(in crate::gui) const PATH: &str = "library-pages.kmodule.ron";

    pub(in crate::gui) fn new(sources: &[Registration]) -> Self {
        let entries: String = sources
            .iter()
            .map(|source| {
                let &SourcePage { id, page } = source.page();
                format!(
                    "            Optional(id: {id:?}, hidden: Model(id: \"library.page.hidden\", \
                     with: {{ \"source\": {id:?} }}), child: Include(id: \"{id}-page\", \
                     source: {page:?}, with: {{ \"source\": {id:?} }})),\n",
                )
            })
            .collect();
        Self {
            text: format!(
                "(\n    schema: \"kithara.module\",\n    version: 1,\n    id: \"library-pages\",\n    \
                 chrome: Plain,\n    root: Stage(\n        id: \"pages\",\n        children: [\n{entries}        ],\n    ),\n)\n"
            ),
        }
    }
}
