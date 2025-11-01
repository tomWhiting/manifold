//! Property table implementation with typed storage and efficient access.

use crate::property_value::PropertyValue;
use manifold::{
    AccessGuard, ReadOnlyTable, ReadTransaction, ReadableTable, ReadableTableMetadata,
    StorageError, Table, TableDefinition, TableError, WriteTransaction,
};
use std::ops::Deref;
use uuid::Uuid;

/// A table storing properties with composite keys (entity_id, property_name).
///
/// Properties are stored with native types (Integer, Float, Boolean, String, Null)
/// instead of string-based serialization, providing efficient storage and direct
/// deserialization without parsing overhead.
pub struct PropertyTable<'txn> {
    table: Table<'txn, (Uuid, &'static str), PropertyValue>,
}

impl<'txn> PropertyTable<'txn> {
    /// Opens a property table for writing.
    pub fn open(txn: &'txn WriteTransaction, name: &str) -> Result<Self, TableError> {
        let def: TableDefinition<(Uuid, &str), PropertyValue> = TableDefinition::new(name);
        let table = txn.open_table(def)?;
        Ok(Self { table })
    }

    /// Sets a property value for an entity.
    ///
    /// # Arguments
    ///
    /// * `entity_id` - The UUID of the entity
    /// * `property_key` - The property name
    /// * `value` - The property value
    pub fn set(
        &mut self,
        entity_id: &Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), TableError> {
        let value_ref = value.as_ref();
        self.table.insert(&(*entity_id, property_key), &value_ref)?;
        Ok(())
    }

    /// Bulk insert multiple properties using Manifold's optimized bulk API.
    ///
    /// This is significantly more efficient than calling `set()` multiple times.
    ///
    /// # Arguments
    ///
    /// * `items` - Iterator of ((entity_id, property_key), value_ref) tuples
    /// * `sorted` - Whether the items are pre-sorted by key
    ///
    /// # Returns
    ///
    /// The number of properties inserted.
    pub fn insert_bulk<'a>(
        &mut self,
        items: &[((Uuid, &'a str), crate::encoding::PropertyValueRef<'a>)],
        sorted: bool,
    ) -> Result<usize, TableError> {
        Ok(self.table.insert_bulk(items.iter().cloned(), sorted)?)
    }

    /// Bulk remove multiple properties using Manifold's optimized bulk API.
    ///
    /// This is significantly more efficient than calling `delete()` multiple times.
    ///
    /// # Arguments
    ///
    /// * `keys` - Slice of (entity_id, property_key) tuples to delete
    ///
    /// # Returns
    ///
    /// The number of properties actually deleted.
    pub fn remove_bulk(&mut self, keys: &[(Uuid, &str)]) -> Result<usize, StorageError> {
        self.table.remove_bulk(keys.iter().cloned())
    }

    /// Gets a property value for an entity.
    ///
    /// Returns a guard providing efficient access to the property value.
    pub fn get(
        &self,
        entity_id: &Uuid,
        property_key: &str,
    ) -> Result<Option<PropertyGuard<'_>>, StorageError> {
        Ok(self
            .table
            .get(&(*entity_id, property_key))?
            .map(PropertyGuard::new))
    }

    /// Deletes a property for an entity.
    ///
    /// Returns true if the property existed and was deleted, false otherwise.
    pub fn delete(&mut self, entity_id: &Uuid, property_key: &str) -> Result<bool, StorageError> {
        Ok(self.table.remove(&(*entity_id, property_key))?.is_some())
    }

    /// Returns the total number of properties in the table.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns true if the table contains no properties.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }

    /// Gets all properties for a specific entity.
    ///
    /// Returns a vector of (property_name, PropertyGuard) tuples.
    /// This uses a range query over the composite key for efficiency.
    pub fn get_all(
        &self,
        entity_id: &Uuid,
    ) -> Result<Vec<(String, PropertyGuard<'_>)>, StorageError> {
        let mut results = Vec::new();

        // Range query: all keys starting with (entity_id, *)
        // We scan from (entity_id, "") to the next UUID
        let start_key = (*entity_id, "");

        // Get iterator starting from our entity_id
        let iter = self.table.range(start_key..)?;

        for result in iter {
            let (key_guard, value_guard) = result?;
            let (id, prop_key) = key_guard.value();

            // Stop when we've moved past this entity_id
            if id != *entity_id {
                break;
            }

            results.push((prop_key.to_string(), PropertyGuard::new(value_guard)));
        }

        Ok(results)
    }
}

/// Read-only property table providing efficient access without write capabilities.
pub struct PropertyTableRead {
    table: ReadOnlyTable<(Uuid, &'static str), PropertyValue>,
}

impl PropertyTableRead {
    /// Opens a property table for reading.
    pub fn open(txn: &ReadTransaction, name: &str) -> Result<Self, StorageError> {
        let def: TableDefinition<(Uuid, &str), PropertyValue> = TableDefinition::new(name);
        let table = txn.open_table(def).map_err(|e| match e {
            TableError::Storage(s) => s,
            _ => StorageError::Io(std::io::Error::other(e)),
        })?;
        Ok(Self { table })
    }

    /// Gets a property value for an entity.
    ///
    /// Returns a guard providing efficient access to the property value.
    pub fn get(
        &self,
        entity_id: &Uuid,
        property_key: &str,
    ) -> Result<Option<PropertyGuard<'_>>, StorageError> {
        Ok(self
            .table
            .get(&(*entity_id, property_key))?
            .map(PropertyGuard::new))
    }

    /// Returns the total number of properties in the table.
    pub fn len(&self) -> Result<u64, StorageError> {
        self.table.len()
    }

    /// Returns true if the table contains no properties.
    pub fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }

    /// Gets all properties for a specific entity.
    ///
    /// Returns a vector of (property_name, PropertyGuard) tuples.
    pub fn get_all(
        &self,
        entity_id: &Uuid,
    ) -> Result<Vec<(String, PropertyGuard<'_>)>, StorageError> {
        let mut results = Vec::new();

        let start_key = (*entity_id, "");
        let iter = self.table.range(start_key..)?;

        for result in iter {
            let (key_guard, value_guard) = result?;
            let (id, prop_key) = key_guard.value();

            if id != *entity_id {
                break;
            }

            results.push((prop_key.to_string(), PropertyGuard::new(value_guard)));
        }

        Ok(results)
    }

    /// Iterates over all properties in the table.
    pub fn iter(&self) -> Result<PropertyIter<'_>, StorageError> {
        Ok(PropertyIter {
            inner: self.table.iter()?,
        })
    }

    /// Gets multiple properties in a single bulk operation.
    ///
    /// This uses Manifold's bulk get API for better performance than individual gets.
    pub fn get_bulk(
        &self,
        keys: &[(Uuid, &str)],
    ) -> Result<Vec<Option<PropertyGuard<'_>>>, StorageError> {
        use manifold::ReadableTable;

        let composite_keys: Vec<(Uuid, &str)> = keys
            .iter()
            .map(|(entity_id, property_key)| (*entity_id, *property_key))
            .collect();

        let guards = self.table.get_bulk(composite_keys.into_iter())?;

        Ok(guards
            .into_iter()
            .map(|opt_guard| opt_guard.map(PropertyGuard::new))
            .collect())
    }
}

/// A guard providing access to a stored property value.
///
/// This guard provides both generic and type-specific accessors for the property value.
/// The value is deserialized once when the guard is created and cached for subsequent access.
pub struct PropertyGuard<'a> {
    value: PropertyValue,
    _guard: AccessGuard<'a, PropertyValue>,
}

impl<'a> PropertyGuard<'a> {
    pub(crate) fn new(guard: AccessGuard<'a, PropertyValue>) -> Self {
        let value = guard.value().to_owned();
        Self {
            value,
            _guard: guard,
        }
    }

    /// Returns a reference to the property value.
    pub fn value(&self) -> &PropertyValue {
        &self.value
    }

    /// Returns the type name of this property value.
    pub fn type_name(&self) -> &'static str {
        self.value.type_name()
    }

    /// Returns the value as an i64 if this is an Integer variant.
    pub fn as_i64(&self) -> Option<i64> {
        self.value.as_integer()
    }

    /// Returns the value as an f64 if this is a Float variant.
    pub fn as_f64(&self) -> Option<f64> {
        self.value.as_float()
    }

    /// Returns the value as a bool if this is a Boolean variant.
    pub fn as_bool(&self) -> Option<bool> {
        self.value.as_boolean()
    }

    /// Returns the value as a string slice if this is a String variant.
    pub fn as_str(&self) -> Option<&str> {
        self.value.as_string()
    }

    /// Returns true if this is a Null variant.
    pub fn is_null(&self) -> bool {
        self.value.is_null()
    }

    /// Returns the updated_at timestamp for this property.
    pub fn updated_at(&self) -> u64 {
        self.value.updated_at()
    }

    /// Returns the valid_from timestamp for this property.
    pub fn valid_from(&self) -> u64 {
        self.value.valid_from()
    }

    /// Converts this guard to an owned PropertyValue (clones the internal value).
    pub fn to_owned(&self) -> PropertyValue {
        self.value.clone()
    }
}

impl<'a> Deref for PropertyGuard<'a> {
    type Target = PropertyValue;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

/// Iterator over properties in a PropertyTableRead.
pub struct PropertyIter<'a> {
    inner: manifold::Range<'a, (Uuid, &'static str), PropertyValue>,
}

impl<'a> Iterator for PropertyIter<'a> {
    type Item = Result<((Uuid, String), PropertyGuard<'a>), StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|result| {
            result.map(|(key_guard, value_guard)| {
                let (entity_id, prop_key) = key_guard.value();
                (
                    (entity_id, prop_key.to_string()),
                    PropertyGuard::new(value_guard),
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifold::{Database, ReadableDatabase};
    use tempfile::TempDir;

    fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::builder().create(&db_path).unwrap();
        (temp_dir, db)
    }

    #[test]
    fn test_set_and_get_integer() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Write
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "age", PropertyValue::new_integer(42))
                .unwrap();
        }
        write_txn.commit().unwrap();

        // Read
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let guard = table.get(&entity_id, "age").unwrap().unwrap();
        assert_eq!(guard.as_i64(), Some(42));
        assert_eq!(guard.type_name(), "Integer");
    }

    #[test]
    fn test_set_and_get_float() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "score", PropertyValue::new_float(98.6))
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let guard = table.get(&entity_id, "score").unwrap().unwrap();
        assert_eq!(guard.as_f64(), Some(98.6));
    }

    #[test]
    fn test_set_and_get_boolean() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "active", PropertyValue::new_boolean(true))
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let guard = table.get(&entity_id, "active").unwrap().unwrap();
        assert_eq!(guard.as_bool(), Some(true));
    }

    #[test]
    fn test_set_and_get_string() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "name", PropertyValue::new_string("Alice"))
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let guard = table.get(&entity_id, "name").unwrap().unwrap();
        assert_eq!(guard.as_str(), Some("Alice"));
    }

    #[test]
    fn test_set_and_get_null() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "optional", PropertyValue::new_null())
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let guard = table.get(&entity_id, "optional").unwrap().unwrap();
        assert!(guard.is_null());
    }

    #[test]
    fn test_delete_property() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Write
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "temp", PropertyValue::new_integer(100))
                .unwrap();
        }
        write_txn.commit().unwrap();

        // Delete
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            let deleted = table.delete(&entity_id, "temp").unwrap();
            assert!(deleted);
        }
        write_txn.commit().unwrap();

        // Verify deleted
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        assert!(table.get(&entity_id, "temp").unwrap().is_none());
    }

    #[test]
    fn test_get_all_properties() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Write multiple properties
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "age", PropertyValue::new_integer(42))
                .unwrap();
            table
                .set(&entity_id, "name", PropertyValue::new_string("Bob"))
                .unwrap();
            table
                .set(&entity_id, "active", PropertyValue::new_boolean(true))
                .unwrap();
        }
        write_txn.commit().unwrap();

        // Read all
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let properties = table.get_all(&entity_id).unwrap();

        assert_eq!(properties.len(), 3);

        // Properties should be sorted by key name
        let prop_names: Vec<_> = properties.iter().map(|(k, _)| k.as_str()).collect();
        assert!(prop_names.contains(&"age"));
        assert!(prop_names.contains(&"name"));
        assert!(prop_names.contains(&"active"));
    }

    #[test]
    fn test_get_all_empty() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Create the table first with a write transaction
        let write_txn = db.begin_write().unwrap();
        {
            let _table = PropertyTable::open(&write_txn, "properties").unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let properties = table.get_all(&entity_id).unwrap();

        assert_eq!(properties.len(), 0);
    }

    #[test]
    fn test_multiple_entities() {
        let (_temp, db) = setup_test_db();
        let entity1 = Uuid::new_v4();
        let entity2 = Uuid::new_v4();

        // Write properties for two entities
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity1, "age", PropertyValue::new_integer(30))
                .unwrap();
            table
                .set(&entity2, "age", PropertyValue::new_integer(40))
                .unwrap();
        }
        write_txn.commit().unwrap();

        // Read both
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();

        let guard1 = table.get(&entity1, "age").unwrap().unwrap();
        let guard2 = table.get(&entity2, "age").unwrap().unwrap();

        assert_eq!(guard1.as_i64(), Some(30));
        assert_eq!(guard2.as_i64(), Some(40));
    }

    #[test]
    fn test_type_safety() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(&entity_id, "age", PropertyValue::new_integer(42))
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let guard = table.get(&entity_id, "age").unwrap().unwrap();

        // Integer property should not be accessible as other types
        assert_eq!(guard.as_i64(), Some(42));
        assert_eq!(guard.as_f64(), None);
        assert_eq!(guard.as_bool(), None);
        assert_eq!(guard.as_str(), None);
        assert!(!guard.is_null());
    }
}
