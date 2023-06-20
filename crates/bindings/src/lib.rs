//! Provides safe abstractions around `bindings-sys`
//! and re-exports `#[spacetimedb]` and `#[duration]`.

#[macro_use]
mod io;
mod impls;
mod logger;
#[doc(hidden)]
pub mod rt;
mod types;

use spacetimedb_lib::buffer::{BufReader, BufWriter, Cursor, DecodeError};
pub use spacetimedb_lib::de::{Deserialize, DeserializeOwned};
pub use spacetimedb_lib::ser::Serialize;
use spacetimedb_lib::{bsatn, ColumnIndexAttribute, IndexType, PrimaryKey, ProductType, ProductValue};
use std::cell::RefCell;
use std::marker::PhantomData;
use std::{fmt, panic};

pub use spacetimedb_bindings_macro::{duration, query, spacetimedb, TableType};

pub use sats::SpacetimeType;
pub use spacetimedb_lib;
pub use spacetimedb_lib::sats;
pub use spacetimedb_lib::AlgebraicValue;
pub use spacetimedb_lib::Identity;
pub use types::Timestamp;

pub use spacetimedb_bindings_sys as sys;
pub use sys::Errno;
use sys::{Buffer, BufferIter};

pub use log;

pub type Result<T = (), E = Errno> = core::result::Result<T, E>;

#[no_mangle]
static SPACETIME_ABI_VERSION: u32 = {
    assert!(spacetimedb_lib::MODULE_ABI_VERSION.to_u32() == sys::ABI_VERSION);
    sys::ABI_VERSION
};
#[no_mangle]
static SPACETIME_ABI_VERSION_IS_ADDR: () = ();

#[non_exhaustive]
#[derive(Copy, Clone)]
pub struct ReducerContext {
    pub sender: Identity,
    pub timestamp: Timestamp,
}

impl ReducerContext {
    #[doc(hidden)]
    pub fn __dummy() -> Self {
        Self {
            sender: Identity { data: [0; 32] },
            timestamp: Timestamp::UNIX_EPOCH,
        }
    }
}

// #[cfg(target_arch = "wasm32")]
// #[global_allocator]
// static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// this gets optimized away to a normal global since wasm32 doesn't have threads by default
thread_local! {
    static ROW_BUF: RefCell<Vec<u8>> = RefCell::new(Vec::with_capacity(8 * 1024));
}

fn with_row_buf<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    ROW_BUF.with(|r| {
        let mut buf = r.borrow_mut();
        buf.clear();
        f(&mut buf)
    })
}

pub fn encode_row(row: ProductValue, bytes: &mut impl BufWriter) {
    row.encode(bytes);
}

pub fn decode_row<'a>(schema: &ProductType, bytes: &mut impl BufReader<'a>) -> Result<ProductValue, DecodeError> {
    ProductValue::decode(schema, bytes)
}

pub fn encode_schema(schema: ProductType, bytes: &mut impl BufWriter) {
    schema.encode(bytes);
}

pub fn decode_schema<'a>(bytes: &mut impl BufReader<'a>) -> Result<ProductType, DecodeError> {
    ProductType::decode(bytes)
}

/*
pub fn create_table(table_name: &str, schema: ProductType) -> Result<u32> {
    with_row_buf(|bytes| {
        schema.encode(bytes);
        sys::create_table(table_name, bytes)
    })
}
*/

pub fn get_table_id(table_name: &str) -> u32 {
    sys::get_table_id(table_name).unwrap_or_else(|_| {
        panic!("Failed to get table with name: {}", table_name);
    })
}

pub fn insert<T: TableType>(table_id: u32, row: T) -> T::InsertResult {
    trait HasAutoinc: TableType {
        const HAS_AUTOINC: bool;
    }
    impl<T: TableType> HasAutoinc for T {
        const HAS_AUTOINC: bool = {
            let mut i = 0;
            let mut x = false;
            while i < T::COLUMN_ATTRS.len() {
                if T::COLUMN_ATTRS[i].is_autoinc() {
                    x = true;
                    break;
                }
                i += 1;
            }
            x
        };
    }
    with_row_buf(|bytes| {
        bsatn::to_writer(bytes, &row).unwrap();
        let res = sys::insert(table_id, bytes).map(|()| {
            if <T as HasAutoinc>::HAS_AUTOINC {
                bsatn::from_slice(bytes).expect("decode error")
            } else {
                row
            }
        });
        sealed::InsertResult::from_res(res)
    })
}

pub fn seek_eq(table_id: u32, col_id: u8, val: &impl Serialize) -> Result<Buffer> {
    with_row_buf(|bytes| {
        bsatn::to_writer(bytes, val).unwrap();
        sys::seek_eq(table_id, col_id as u32, bytes)
    })
}

pub fn delete_eq(table_id: u32, col_id: u8, eq_value: &impl Serialize) -> Result<u32> {
    with_row_buf(|bytes| {
        bsatn::to_writer(bytes, eq_value).unwrap();
        sys::delete_eq(table_id, col_id.into(), bytes)
    })
}

/*
pub fn delete_pk(table_id: u32, primary_key: &PrimaryKey) -> Result<()> {
    with_row_buf(|bytes| {
        primary_key.encode(bytes);
        sys::delete_pk(table_id, bytes)
    })
}

pub fn delete_filter<F: Fn(&ProductValue) -> bool>(table_id: u32, f: F) -> Result<usize> {
    with_row_buf(|bytes| {
        let mut count = 0;
        for tuple_value in pv_table_iter(table_id, None)? {
            if f(&tuple_value) {
                count += 1;
                bytes.clear();
                tuple_value.encode(bytes);
                sys::delete_value(table_id, bytes)?;
            }
        }
        Ok(count)
    })
}

pub fn delete_range(table_id: u32, col_id: u8, range: Range<AlgebraicValue>) -> Result<u32> {
    with_row_buf(|bytes| {
        range.start.encode(bytes);
        let mid = bytes.len();
        range.end.encode(bytes);
        let (range_start, range_end) = bytes.split_at(mid);
        sys::delete_range(table_id, col_id.into(), range_start, range_end)
    })
}
*/

// TODO: going to have to somehow ensure AlgebraicValue is equatable
// pub fn filter_eq(_table_id: u32, _col_id: u8, _eq_value: AlgebraicValue) -> Option<ProductValue> {
//     return None;
// }

//
// fn page_table(table_id : u32, pager_token : u32, read_entries : u32) {
//
// }

// Get the buffer iterator for this table, and return it and its decoded `ProductType` schema.
fn buffer_table_iter(
    table_id: u32,
    filter: Option<spacetimedb_lib::filter::Expr>,
) -> Result<(BufferIter, ProductType)> {
    let filter = filter
        .as_ref()
        .map(bsatn::to_vec)
        .transpose()
        .expect("Couldn't decode the filter query");
    let mut iter = sys::iter(table_id, filter.as_deref())?;

    // First item is an encoded schema.
    let schema_raw = iter.next().expect("Missing schema").expect("Failed to get schema");
    let schema = decode_schema(&mut &schema_raw[..]).expect("Could not decode schema");

    Ok((iter, schema))
}

/// A table iterator which yields `ProductValue`s.
// type ProductValueTableIter = RawTableIter<ProductValue, ProductValueBufferDeserialize>;

// fn pv_table_iter(table_id: u32, filter: Option<spacetimedb_lib::filter::Expr>) -> Result<ProductValueTableIter> {
//     let (iter, schema) = buffer_table_iter(table_id, filter)?;
//     let deserializer = ProductValueBufferDeserialize::new(schema);
//     Ok(RawTableIter::new(iter, deserializer))
// }

/// A table iterator which yields values of the `TableType` corresponding to the table.
type TableTypeTableIter<T> = RawTableIter<T, TableTypeBufferDeserialize<T>>;

fn table_iter<T: TableType>(table_id: u32, filter: Option<spacetimedb_lib::filter::Expr>) -> Result<TableIter<T>> {
    // The TableType deserializer doesn't need the schema, as we have type-directed
    // dispatch to deserialize any given `TableType`.
    let (iter, _schema) = buffer_table_iter(table_id, filter)?;
    let deserializer = TableTypeBufferDeserialize::new();
    let iter = RawTableIter::new(iter, deserializer);
    Ok(TableIter::new(iter))
}

/// A trait for deserializing mulitple items out of a single `BufReader`.
///
/// Each `BufReader` holds a number of concatenated serialized objects.
trait BufferDeserialize {
    /// The type of the items being deserialized.
    type Item;

    /// Deserialize one entry from the `reader`, which must not be empty.
    fn deserialize<'de>(&mut self, reader: impl BufReader<'de>) -> Self::Item;
}

/// Deserialize `ProductValue`s from `Buffer`s.
// struct ProductValueBufferDeserialize {
//     /// The schema to deserialize with.
//     schema: ProductType,
// }

// impl ProductValueBufferDeserialize {
//     fn new(schema: ProductType) -> Self {
//         Self { schema }
//     }
// }

// impl BufferDeserialize for ProductValueBufferDeserialize {
//     type Item = ProductValue;

//     fn deserialize<'de>(&mut self, mut reader: impl BufReader<'de>) -> Self::Item {
//         decode_row(&self.schema, &mut reader).expect("Failed to decode row!")
//     }
// }

/// Deserialize bsatn values to a particular `TableType`.
struct TableTypeBufferDeserialize<T> {
    _marker: PhantomData<T>,
}

impl<T> TableTypeBufferDeserialize<T> {
    fn new() -> Self {
        Self { _marker: PhantomData }
    }
}

impl<T: TableType> BufferDeserialize for TableTypeBufferDeserialize<T> {
    type Item = T;

    fn deserialize<'de>(&mut self, mut reader: impl BufReader<'de>) -> Self::Item {
        bsatn::from_reader(&mut reader).expect("Failed to decode row!")
    }
}

/// Iterate over a sequence of `Buffer`s and deserialize a number of `T`s
/// out of each.
struct RawTableIter<T, De: BufferDeserialize<Item = T>> {
    /// The underlying source of our `Buffer`s.
    inner: BufferIter,

    /// The current position in the current buffer, from which
    /// `deserializer` can read. A value of `None` indicates that
    /// we need to pull another `Buffer` from `inner`.
    reader: Option<Cursor<Box<[u8]>>>,

    deserializer: De,
}

impl<T, De: BufferDeserialize<Item = T>> RawTableIter<T, De> {
    fn new(iter: BufferIter, deserializer: De) -> Self {
        RawTableIter {
            inner: iter,
            reader: None,
            deserializer,
        }
    }
}

impl<T, De: BufferDeserialize<Item = T>> Iterator for RawTableIter<T, De> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // If we currently have some bytes in the buffer to still decode,
            // do that. Otherwise, try to fetch the next buffer first.

            match &self.reader {
                Some(reader) => {
                    if reader.remaining() == 0 {
                        self.reader = None;
                        continue;
                    }
                    break;
                }
                None => {
                    // If we receive None here, iteration is complete.
                    let buffer = self.inner.next()?;
                    let buffer = buffer.expect("RawTableIter::next: Failed to get buffer!");
                    self.reader = Some(Cursor::new(buffer));
                    break;
                }
            }
        }

        let reader = self.reader.as_ref().unwrap();
        let row = self.deserializer.deserialize(reader);
        Some(row)
    }
}

#[derive(Clone, Copy)]
pub struct IndexDef<'a> {
    pub name: &'a str,
    pub ty: IndexType,
    pub col_ids: &'a [u8],
}

pub struct TableIter<T: TableType> {
    iter: TableTypeTableIter<T>,
}

impl<T: TableType> TableIter<T> {
    fn new(iter: TableTypeTableIter<T>) -> Self {
        Self { iter }
    }
}

impl<T: TableType> Iterator for TableIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

pub trait TableType: SpacetimeType + DeserializeOwned + Serialize {
    const TABLE_NAME: &'static str;
    const COLUMN_ATTRS: &'static [ColumnIndexAttribute];
    const INDEXES: &'static [IndexDef<'static>];
    type InsertResult: sealed::InsertResult<T = Self>;

    fn table_id() -> u32;

    fn insert(ins: Self) -> Self::InsertResult {
        insert(Self::table_id(), ins)
    }

    fn iter() -> TableIter<Self> {
        table_iter(Self::table_id(), None).unwrap()
    }

    #[doc(hidden)]
    fn iter_filtered(filter: spacetimedb_lib::filter::Expr) -> TableIter<Self> {
        table_iter(Self::table_id(), Some(filter)).unwrap()
    }
}

mod sealed {
    use super::*;
    pub trait InsertResult {
        type T: TableType;
        fn from_res(res: Result<Self::T>) -> Self;
    }
}

pub struct UniqueConstraintViolation<T: TableType>(PhantomData<T>);
impl<T: TableType> fmt::Debug for UniqueConstraintViolation<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UniqueConstraintViolation({})", T::TABLE_NAME)
    }
}
impl<T: TableType> fmt::Display for UniqueConstraintViolation<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not able to insert into table {}; duplicate unique column",
            T::TABLE_NAME
        )
    }
}
impl<T: TableType> std::error::Error for UniqueConstraintViolation<T> {}

impl<T: TableType> sealed::InsertResult for Result<T, UniqueConstraintViolation<T>> {
    type T = T;
    fn from_res(res: Result<Self::T>) -> Self {
        res.map_err(|e| match e {
            Errno::EXISTS => UniqueConstraintViolation(PhantomData),
            _ => panic!("unexpected error from insert(): {e}"),
        })
    }
}

impl<T: TableType> sealed::InsertResult for T {
    type T = T;
    fn from_res(res: Result<Self::T>) -> Self {
        res.unwrap_or_else(|e| panic!("unexpected error from insert(): {e}"))
    }
}

// Decode exactly 0 or 1 |T|, leaving the buffer exactly empty.
fn bsatn_from_reader<'de, R: BufReader<'de>, T: spacetimedb_lib::de::Deserialize<'de>>(
    r: &mut R,
) -> Result<Option<T>, DecodeError> {
    Ok(match r.remaining() {
        0 => None,
        _ => {
            let t = bsatn::from_reader(r)?;
            assert_eq!(r.remaining(), 0);
            Some(t)
        }
    })
}

pub trait FilterableValue: Serialize + Eq {}

pub trait UniqueValue: FilterableValue {
    fn into_primarykey(self) -> PrimaryKey;
}

#[doc(hidden)]
pub mod query {
    use super::*;

    pub trait FieldAccess<const N: u8> {
        type Field;
        fn get_field(&self) -> &Self::Field;
    }

    #[doc(hidden)]
    pub fn filter_by_unique_field<Table: TableType, T: UniqueValue, const COL_IDX: u8>(val: &T) -> Option<Table>
    where
        Table: FieldAccess<COL_IDX, Field = T>,
    {
        let buffer = seek_eq(Table::table_id(), COL_IDX, val).unwrap();
        let bytes = buffer.read();
        let mut slice: &[u8] = &bytes;
        // We will always find either 0 or 1 rows here.
        bsatn_from_reader(&mut slice).unwrap()
    }

    #[doc(hidden)]
    pub fn filter_by_field<'a, Table: TableType, T: FilterableValue, const COL_IDX: u8>(
        val: &'a T,
    ) -> FilterByIter<'a, Table, COL_IDX, T>
    where
        'a: 'a,
    {
        // In the future, this should instead call seek_eq.
        FilterByIter {
            inner: Table::iter(),
            val,
        }
    }

    #[doc(hidden)]
    pub fn delete_by_field<Table: TableType, T: UniqueValue, const COL_IDX: u8>(val: &T) -> bool {
        let result = delete_eq(Table::table_id(), COL_IDX, val);
        match result {
            Err(_) => {
                //TODO: Returning here was supposed to signify an error, but it can also return `Err(_)` when there is nothing to delete.
                //spacetimedb::println!("Internal server error on equatable type: {}", #primary_key_tuple_type_str);
                false
            }
            Ok(count) => count > 0,
        }
    }

    #[doc(hidden)]
    pub fn update_by_field<Table: TableType, T: UniqueValue, const COL_IDX: u8>(val: &T, new_value: Table) -> bool {
        delete_by_field::<Table, T, COL_IDX>(val);
        Table::insert(new_value);

        // For now this is always successful
        true
    }

    #[doc(hidden)]
    pub struct FilterByIter<'a, Table: TableType, const COL_IDX: u8, T: FilterableValue> {
        inner: TableIter<Table>,
        val: &'a T,
    }
    impl<'a, Table: TableType, const COL_IDX: u8, T: FilterableValue> Iterator for FilterByIter<'a, Table, COL_IDX, T>
    where
        Table: FieldAccess<COL_IDX, Field = T>,
    {
        type Item = Table;
        fn next(&mut self) -> Option<Self::Item> {
            self.inner.find_map(|row| (row.get_field() == self.val).then_some(row))
        }
    }
}

#[macro_export]
macro_rules! schedule {
    // this errors on literals with time unit suffixes, e.g. 100ms
    // I swear I saw a rustc tracking issue to allow :literal to match even an invalid suffix but I can't seem to find it
    ($dur:literal, $($args:tt)*) => {
        $crate::schedule!($crate::duration!($dur), $($args)*)
    };
    ($dur:expr, $($args:tt)*) => {
        $crate::__schedule_impl!($crate::rt::schedule_in($dur), [] [$($args)*])
    };
}
#[macro_export]
macro_rules! schedule_at {
    ($time:expr, $($args:tt)*) => {
        $crate::__schedule_impl!($time, [] [$($args)*])
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __schedule_impl {
    ($time:expr, [$repeater:path] [($($args:tt)*)]) => {
        $crate::__schedule_impl!(@process_args $time, $repeater, ($($args)*))
    };
    ($time:expr, [$($cur:tt)*] [$next:tt $($rest:tt)*]) => {
        $crate::__schedule_impl!($time, [$($cur)* $next] [$($rest)*])
    };
    (@process_args $time:expr, $repeater:path, (_$(, $args:expr)* $(,)?)) => {
        $crate::__schedule_impl!(@call $time, $repeater, $crate::ReducerContext::__dummy(), ($($args),*))
    };
    (@process_args $time:expr, $repeater:path, ($($args:expr),* $(,)?)) => {
        $crate::__schedule_impl!(@call $time, $repeater, , ($($args),*))
    };
    (@call $time:expr, $repeater:path, $($ctx:expr)?, ($($args:expr),*)) => {
        <$repeater>::schedule($time, $($ctx,)? $($args),*);
    };
}

pub struct ScheduleToken<R = AnyReducer> {
    id: u64,
    _marker: PhantomData<R>,
}

impl<R> Clone for ScheduleToken<R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R> Copy for ScheduleToken<R> {}

impl<R> Serialize for ScheduleToken<R> {
    fn serialize<S: spacetimedb_lib::ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.id.serialize(serializer)
    }
}
impl<'de, R> Deserialize<'de> for ScheduleToken<R> {
    fn deserialize<D: spacetimedb_lib::de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u64::deserialize(deserializer).map(Self::new)
    }
}
impl<R> SpacetimeType for ScheduleToken<R> {
    fn make_type<S: spacetimedb_lib::sats::typespace::TypespaceBuilder>(_ts: &mut S) -> spacetimedb_lib::AlgebraicType {
        spacetimedb_lib::AlgebraicType::U64
    }
}

impl<R> ScheduleToken<R> {
    #[inline]
    fn new(id: u64) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn erase(self) -> ScheduleToken {
        ScheduleToken::new(self.id)
    }

    /// Cancel this scheduled reducer. This method is idempotent.
    #[inline]
    pub fn cancel(self) {
        sys::cancel_reducer(self.id)
    }
}

pub struct AnyReducer {
    _never: std::convert::Infallible,
}