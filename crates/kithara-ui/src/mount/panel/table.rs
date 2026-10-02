use bon::Builder;

use crate::{expand::Binding, module::TableColumn};

/// A table whose columns and row values are supplied by the document and host.
#[derive(Builder, kithara_derive::Control)]
#[control(size = skin.table.size)]
pub(crate) struct Table<'a> {
    pub(crate) columns: &'a [TableColumn],
    pub(crate) columns_state: Option<&'a Binding>,
    pub(crate) resizable: bool,
    pub(crate) footer: bool,
    pub(crate) padding_left: f32,
    pub(crate) padding_right: f32,
    pub(crate) status: Option<&'a Binding>,
}
