use crate::db::{messages::transaction::Transaction, relational_db::ST_TABLES_ID};
use core::fmt;
use spacetimedb_lib::DataKey;
use spacetimedb_sats::{
    relation::{DbTable, FieldName, FieldOnly, Header, TableField},
    AlgebraicType, AlgebraicValue, ProductType, ProductTypeElement, ProductValue,
};
use spacetimedb_vm::expr::SourceExpr;
use std::{ops::RangeBounds, sync::Arc};

use super::{system_tables::StTableRow, Result};

/// The `id` for [Sequence]
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct TableId(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ColId(pub(crate) u32);
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct IndexId(pub(crate) u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SequenceId(pub(crate) u32);

impl fmt::Display for SequenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceSchema {
    pub(crate) sequence_id: u32,
    pub(crate) sequence_name: String,
    pub(crate) table_id: u32,
    pub(crate) col_id: u32,
    pub(crate) increment: i128,
    pub(crate) start: i128,
    pub(crate) min_value: i128,
    pub(crate) max_value: i128,
    pub(crate) allocated: i128,
}

/// This type is just the [SequenceSchema] without the autoinc fields
/// It's also adjusted to be convenient for specifying a new sequence
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceDef {
    pub(crate) sequence_name: String,
    pub(crate) table_id: u32,
    pub(crate) col_id: u32,
    pub(crate) increment: i128,
    pub(crate) start: Option<i128>,
    pub(crate) min_value: Option<i128>,
    pub(crate) max_value: Option<i128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSchema {
    pub(crate) index_id: u32,
    pub(crate) table_id: u32,
    pub(crate) col_id: u32,
    pub(crate) index_name: String,
    pub(crate) is_unique: bool,
}

/// This type is just the [IndexSchema] without the autoinc fields
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDef {
    pub(crate) table_id: u32,
    pub(crate) col_id: u32,
    pub(crate) name: String,
    pub(crate) is_unique: bool,
}

impl IndexDef {
    pub fn new(name: String, table_id: u32, col_id: u32, is_unique: bool) -> Self {
        Self {
            col_id,
            name,
            is_unique,
            table_id,
        }
    }
}

impl From<IndexSchema> for IndexDef {
    fn from(value: IndexSchema) -> Self {
        Self {
            table_id: value.table_id,
            col_id: value.col_id,
            name: value.index_name,
            is_unique: value.is_unique,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    pub(crate) table_id: u32,
    pub(crate) col_id: u32,
    pub(crate) col_name: String,
    pub(crate) col_type: AlgebraicType,
    pub(crate) is_autoinc: bool,
    // TODO(cloutiertyler): Unique constraints do not belong here.
    // pub(crate) is_unique: bool,
}

impl From<&ColumnSchema> for spacetimedb_lib::table::ColumnDef {
    fn from(value: &ColumnSchema) -> Self {
        Self {
            column: ProductTypeElement::from(value),
            // TODO(cloutiertyler): !!! This is not correct !!! We do not have the information regarding constraints here.
            // We should remove this field from the ColumnDef struct.
            attr: if value.is_autoinc {
                spacetimedb_lib::ColumnIndexAttribute::AutoInc
            } else {
                spacetimedb_lib::ColumnIndexAttribute::UnSet
            },
            // if value.is_autoinc && value.is_unique {
            //     spacetimedb_lib::ColumnIndexAttribute::Identity
            // } else if value.is_autoinc {
            //     spacetimedb_lib::ColumnIndexAttribute::AutoInc
            // } else if value.is_unique {
            //     spacetimedb_lib::ColumnIndexAttribute::Unique
            // } else {
            //     spacetimedb_lib::ColumnIndexAttribute::UnSet
            // },
            pos: value.col_id as usize,
        }
    }
}

impl From<&ColumnSchema> for ProductTypeElement {
    fn from(value: &ColumnSchema) -> Self {
        Self {
            name: Some(value.col_name.clone()),
            algebraic_type: value.col_type.clone(),
        }
    }
}

/// This type is just the [ColumnSchema] without the autoinc fields
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDef {
    pub(crate) col_name: String,
    pub(crate) col_type: AlgebraicType,
    pub(crate) is_autoinc: bool,
}

impl From<ColumnSchema> for ColumnDef {
    fn from(value: ColumnSchema) -> Self {
        Self {
            col_name: value.col_name,
            col_type: value.col_type,
            is_autoinc: value.is_autoinc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub(crate) table_id: u32,
    pub(crate) table_name: String,
    pub(crate) columns: Vec<ColumnSchema>,
    pub(crate) indexes: Vec<IndexSchema>,
}

impl TableSchema {
    pub fn get_column_by_field(&self, field: &FieldName) -> Option<&ColumnSchema> {
        match field.field() {
            FieldOnly::Name(x) => self.get_column_by_name(x),
            FieldOnly::Pos(x) => self.get_column(x),
        }
    }

    pub fn get_column(&self, pos: usize) -> Option<&ColumnSchema> {
        self.columns.get(pos)
    }

    pub fn get_column_by_name(&self, name: &str) -> Option<&ColumnSchema> {
        self.columns.iter().find(|x| x.col_name == name)
    }

    /// Turn a [TableField] that could be an unqualified field `id` into `table.id`
    pub fn normalize_field(&self, name: &TableField) -> FieldName {
        if let Some(t) = name.table {
            FieldName::named(t, name.field)
        } else {
            FieldName::named(&self.table_name, name.field)
        }
    }
}

impl From<&TableSchema> for ProductType {
    fn from(value: &TableSchema) -> Self {
        ProductType::new(
            value
                .columns
                .iter()
                .map(|c| ProductTypeElement {
                    name: Some(c.col_name.clone()),
                    algebraic_type: c.col_type.clone(),
                })
                .collect(),
        )
    }
}

impl From<&TableSchema> for SourceExpr {
    fn from(value: &TableSchema) -> Self {
        SourceExpr::DbTable(DbTable::new(
            &Header::from_product_type(&value.table_name, value.into()),
            value.table_id,
        ))
    }
}

impl From<&TableSchema> for DbTable {
    fn from(value: &TableSchema) -> Self {
        DbTable::new(&value.into(), value.table_id)
    }
}

impl From<&TableSchema> for Header {
    fn from(value: &TableSchema) -> Self {
        Header::from_product_type(&value.table_name, value.into())
    }
}

impl TableDef {
    pub fn get_row_type(&self) -> ProductType {
        ProductType::new(
            self.columns
                .iter()
                .map(|c| ProductTypeElement {
                    name: None,
                    algebraic_type: c.col_type.clone(),
                })
                .collect(),
        )
    }
}

/// This type is just the [TableSchema] without the autoinc fields
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDef {
    pub(crate) table_name: String,
    pub(crate) columns: Vec<ColumnDef>,
    pub(crate) indexes: Vec<IndexDef>,
}

impl From<ProductType> for TableDef {
    fn from(value: ProductType) -> Self {
        Self {
            table_name: "".to_string(),
            columns: value
                .elements
                .iter()
                .enumerate()
                .map(|(i, e)| ColumnDef {
                    col_name: e.name.to_owned().unwrap_or(i.to_string()),
                    col_type: e.algebraic_type.clone(),
                    is_autoinc: false,
                })
                .collect(),
            indexes: vec![],
        }
    }
}

impl From<TableSchema> for TableDef {
    fn from(value: TableSchema) -> Self {
        Self {
            table_name: value.table_name,
            columns: value.columns.into_iter().map(Into::into).collect(),
            indexes: value.indexes.into_iter().map(Into::into).collect(),
        }
    }
}

pub trait Blob {
    fn view(&self) -> &[u8];
}

pub trait Data: Into<ProductValue> {
    fn view(&self) -> &ProductValue;
}

pub trait BlobRow: Send + Sync {
    type TableId: Copy;
    type RowId: Copy;

    type Blob: Blob;
    type BlobRef: Clone;

    fn blob_to_owned(&self, blob_ref: Self::BlobRef) -> Self::Blob;
}

pub trait DataRow: Send + Sync {
    type RowId: Copy;

    type Data: Data;
    type DataRef: Clone;

    fn data_to_owned(&self, data_ref: Self::DataRef) -> Self::Data;
}

pub trait Tx {
    type TxId;

    fn begin_tx(&self) -> Self::TxId;
    fn release_tx(&self, tx: Self::TxId);
}

pub trait MutTx {
    type MutTxId;

    fn begin_mut_tx(&self) -> Self::MutTxId;
    fn rollback_mut_tx(&self, tx: Self::MutTxId);
    fn commit_mut_tx(&self, tx: Self::MutTxId) -> Result<Option<Arc<Transaction>>>;
}

pub trait Blobstore: BlobRow {
    type ScanIterator<'a>: Iterator<Item = Self::BlobRef>
    where
        Self: 'a;

    fn scan_blobs(&self, table_id: TableId) -> Result<Self::ScanIterator<'_>>;

    fn get_row_blob(&self, table_id: TableId, row_id: Self::RowId) -> Result<Option<Self::BlobRef>>;
}

pub trait MutBlobstore: Blobstore {
    fn delete_row_blob(&self, table_id: TableId, row_id: Self::RowId) -> Result<()>;

    fn insert_row_blob(&self, table_id: TableId, row: &[u8]) -> Result<Self::RowId>;
}

pub trait Datastore: DataRow {
    type ScanIterator<'a>: Iterator<Item = Self::DataRef>
    where
        Self: 'a;

    type RangeIterator<'a, R: RangeBounds<AlgebraicValue>>: Iterator<Item = Self::DataRef>
    where
        Self: 'a;

    type SeekIterator<'a>: Iterator<Item = Self::DataRef>
    where
        Self: 'a;

    fn scan(&self, table_id: TableId) -> Result<Self::ScanIterator<'_>>;

    fn range_scan<R: RangeBounds<AlgebraicValue>>(
        &self,
        table_id: TableId,
        col_id: ColId,
        range: R,
    ) -> Result<Self::RangeIterator<'_, R>>;

    fn seek<'a>(
        &'a self,
        table_id: TableId,
        col_id: ColId,
        value: &'a AlgebraicValue,
    ) -> Result<Self::SeekIterator<'a>>;

    fn get_row(&self, table_id: TableId, row_id: Self::RowId) -> Result<Option<Self::DataRef>>;
}

pub trait MutDatastore: Datastore {
    fn delete_row(&self, table_id: TableId, row_id: Self::RowId) -> Result<()>;

    fn insert_row(&self, table_id: TableId, row: ProductValue) -> Result<Self::RowId>;
}

pub trait TxBlobstore: BlobRow + Tx {
    type ScanIterator<'a>: Iterator<Item = Self::BlobRef>
    where
        Self: 'a;

    fn scan_blobs_tx<'a>(&'a self, tx: &'a Self::TxId, table_id: TableId) -> Result<Self::ScanIterator<'a>>;

    fn get_row_blob_tx<'a>(
        &'a self,
        tx: &'a Self::TxId,
        table_id: TableId,
        row_id: Self::RowId,
    ) -> Result<Option<Self::BlobRef>>;
}

pub trait MutTxBlobstore: TxBlobstore + MutTx {
    fn scan_blobs_mut_tx<'a>(&'a self, tx: &'a Self::MutTxId, table_id: TableId) -> Result<Self::ScanIterator<'a>>;

    fn get_row_blob_mut_tx<'a>(
        &'a self,
        tx: &'a Self::MutTxId,
        table_id: TableId,
        row_id: Self::RowId,
    ) -> Result<Option<Self::BlobRef>>;

    fn delete_row_blob_mut_tx<'a>(
        &'a self,
        tx: &'a mut Self::MutTxId,
        table_id: TableId,
        row_id: Self::RowId,
    ) -> Result<()>;

    fn insert_row_blob_mut_tx<'a>(
        &'a self,
        tx: &'a mut Self::MutTxId,
        table_id: TableId,
        row: &[u8],
    ) -> Result<Self::RowId>;
}

pub trait TxDatastore: DataRow + Tx {
    type ScanIterator<'a>: Iterator<Item = Self::DataRef>
    where
        Self: 'a;

    type RangeIterator<'a, R: RangeBounds<AlgebraicValue>>: Iterator<Item = Self::DataRef>
    where
        Self: 'a;

    type SeekIterator<'a>: Iterator<Item = Self::DataRef>
    where
        Self: 'a;

    fn scan_tx<'a>(&'a self, tx: &'a Self::TxId, table_id: TableId) -> Result<Self::ScanIterator<'a>>;

    fn range_scan_tx<'a, R: RangeBounds<AlgebraicValue>>(
        &'a self,
        tx: &'a Self::TxId,
        table_id: TableId,
        col_id: ColId,
        range: R,
    ) -> Result<Self::RangeIterator<'a, R>>;

    fn seek_tx<'a>(
        &'a self,
        tx: &'a Self::TxId,
        table_id: TableId,
        col_id: ColId,
        value: &'a AlgebraicValue,
    ) -> Result<Self::SeekIterator<'a>>;

    fn get_row_tx<'a>(
        &'a self,
        tx: &'a Self::TxId,
        table_id: TableId,
        row_id: Self::RowId,
    ) -> Result<Option<Self::DataRef>>;
}

pub trait MutTxDatastore: TxDatastore + MutTx {
    // Tables
    fn create_table_mut_tx(&self, tx: &mut Self::MutTxId, schema: TableDef) -> Result<TableId>;
    fn row_type_for_table_mut_tx(&self, tx: &Self::MutTxId, table_id: TableId) -> Result<ProductType>;
    fn schema_for_table_mut_tx(&self, tx: &Self::MutTxId, table_id: TableId) -> Result<TableSchema>;
    fn drop_table_mut_tx(&self, tx: &mut Self::MutTxId, table_id: TableId) -> Result<()>;
    fn rename_table_mut_tx(&self, tx: &mut Self::MutTxId, table_id: TableId, new_name: &str) -> Result<()>;
    fn table_id_exists(&self, tx: &Self::MutTxId, table_id: &TableId) -> bool;
    fn table_id_from_name_mut_tx(&self, tx: &Self::MutTxId, table_name: &str) -> Result<Option<TableId>>;
    fn table_name_from_id_mut_tx(&self, tx: &Self::MutTxId, table_id: TableId) -> Result<Option<String>>;
    fn get_all_tables_mut_tx(&self, tx: &Self::MutTxId) -> super::Result<Vec<TableSchema>> {
        let mut tables = Vec::new();
        let table_rows = self.scan_mut_tx(tx, TableId(ST_TABLES_ID))?.collect::<Vec<_>>();
        for data_ref in table_rows {
            let data = self.data_to_owned(data_ref);
            let row = StTableRow::try_from(data.view())?;
            let table_id = TableId(row.table_id);
            tables.push(self.schema_for_table_mut_tx(tx, table_id)?);
        }
        Ok(tables)
    }

    // Indexes
    fn create_index_mut_tx(&self, tx: &mut Self::MutTxId, index: IndexDef) -> Result<IndexId>;
    fn drop_index_mut_tx(&self, tx: &mut Self::MutTxId, index_id: IndexId) -> Result<()>;
    fn index_id_from_name_mut_tx(&self, tx: &Self::MutTxId, index_name: &str) -> super::Result<Option<IndexId>>;

    // TODO: Index data
    // - index_scan_mut_tx
    // - index_range_scan_mut_tx
    // - index_seek_mut_tx

    // Sequences
    fn get_next_sequence_value_mut_tx(&self, tx: &mut Self::MutTxId, seq_id: SequenceId) -> Result<i128>;
    fn create_sequence_mut_tx(&self, tx: &mut Self::MutTxId, seq: SequenceDef) -> Result<SequenceId>;
    fn drop_sequence_mut_tx(&self, tx: &mut Self::MutTxId, seq_id: SequenceId) -> Result<()>;
    fn sequence_id_from_name_mut_tx(
        &self,
        tx: &Self::MutTxId,
        sequence_name: &str,
    ) -> super::Result<Option<SequenceId>>;

    // Data
    fn scan_mut_tx<'a>(&'a self, tx: &'a Self::MutTxId, table_id: TableId) -> Result<Self::ScanIterator<'a>>;
    fn range_scan_mut_tx<'a, R: RangeBounds<AlgebraicValue>>(
        &'a self,
        tx: &'a Self::MutTxId,
        table_id: TableId,
        col_id: ColId,
        range: R,
    ) -> Result<Self::RangeIterator<'a, R>>;
    fn seek_mut_tx<'a>(
        &'a self,
        tx: &'a Self::MutTxId,
        table_id: TableId,
        col_id: ColId,
        value: &'a AlgebraicValue,
    ) -> Result<Self::SeekIterator<'a>>;
    fn get_row_mut_tx<'a>(
        &'a self,
        tx: &'a Self::MutTxId,
        table_id: TableId,
        row_id: Self::RowId,
    ) -> Result<Option<Self::DataRef>>;
    fn delete_row_mut_tx<'a>(
        &'a self,
        tx: &'a mut Self::MutTxId,
        table_id: TableId,
        row_id: Self::RowId,
    ) -> Result<bool>;
    fn delete_rows_in_mut_tx<R: IntoIterator<Item = ProductValue>>(
        &self,
        tx: &mut Self::MutTxId,
        table_id: TableId,
        relation: R,
    ) -> Result<Option<u32>>;
    fn insert_row_mut_tx<'a>(
        &'a self,
        tx: &'a mut Self::MutTxId,
        table_id: TableId,
        row: ProductValue,
    ) -> Result<ProductValue>;

    // TODO(cloutiertyler): This function is needed as a kludge and should be removed.
    fn resolve_data_key_mut_tx(&self, tx: &Self::MutTxId, data_key: &DataKey) -> Result<Option<Arc<Vec<u8>>>>;
}