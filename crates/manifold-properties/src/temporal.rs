//! Temporal query support for point-in-time queries and version history.
//!
//! This module provides functions for querying property values at specific points
//! in time and retrieving version history using the `valid_from` timestamp.

use crate::table::{PropertyGuard, PropertyTableRead};
use manifold::StorageError;
use uuid::Uuid;

/// Gets a property value as it existed at a specific timestamp.
///
/// This uses the `valid_from` field to determine which version of the property
/// was valid at the given timestamp. Properties are stored with their temporal
/// metadata, allowing reconstruction of historical state.
///
/// # Arguments
///
/// * `table` - The property table to query
/// * `entity_id` - The UUID of the entity
/// * `property_key` - The property name
/// * `timestamp` - The timestamp to query (nanoseconds since Unix epoch)
///
/// # Returns
///
/// The property value that was valid at the given timestamp, or None if:
/// - The property didn't exist at that time
/// - The property's valid_from is after the requested timestamp
///
/// # Example
///
/// ```no_run
/// use manifold_properties::{PropertyTableRead, temporal::get_property_at};
/// use uuid::Uuid;
/// # use manifold::Database;
/// # let db = Database::builder().create_temp().unwrap();
/// # let txn = db.begin_read().unwrap();
/// # let table = PropertyTableRead::open(&txn, "props").unwrap();
///
/// let entity = Uuid::new_v4();
/// let timestamp = 1000000000; // Some point in time
///
/// if let Some(guard) = get_property_at(&table, &entity, "age", timestamp).unwrap() {
///     println!("Age at timestamp {}: {:?}", timestamp, guard.as_i64());
/// }
/// ```
pub fn get_property_at<'a>(
    table: &'a PropertyTableRead,
    entity_id: &Uuid,
    property_key: &str,
    timestamp: u64,
) -> Result<Option<PropertyGuard<'a>>, StorageError> {
    // Get the current property
    if let Some(guard) = table.get(entity_id, property_key)? {
        // Check if this version was valid at the requested timestamp
        if guard.valid_from() <= timestamp {
            return Ok(Some(guard));
        }
    }

    // Property either doesn't exist or wasn't valid yet at the timestamp
    Ok(None)
}

/// Gets the version history of a property.
///
/// Returns all versions of a property ordered by their `valid_from` timestamp.
/// This is useful for auditing changes and tracking property evolution over time.
///
/// **Note:** Current implementation returns only the current version. Full version
/// history requires a separate versioning table (planned for future implementation).
///
/// # Arguments
///
/// * `table` - The property table to query
/// * `entity_id` - The UUID of the entity
/// * `property_key` - The property name
///
/// # Returns
///
/// A vector of (timestamp, PropertyGuard) tuples ordered by timestamp.
/// In the current implementation, this will contain at most one entry (the current version).
///
/// # Example
///
/// ```no_run
/// use manifold_properties::{PropertyTableRead, temporal::get_property_history};
/// use uuid::Uuid;
/// # use manifold::Database;
/// # let db = Database::builder().create_temp().unwrap();
/// # let txn = db.begin_read().unwrap();
/// # let table = PropertyTableRead::open(&txn, "props").unwrap();
///
/// let entity = Uuid::new_v4();
/// let history = get_property_history(&table, &entity, "age").unwrap();
///
/// for (timestamp, guard) in history {
///     println!("At {}: {:?}", timestamp, guard.value());
/// }
/// ```
pub fn get_property_history<'a>(
    table: &'a PropertyTableRead,
    entity_id: &Uuid,
    property_key: &str,
) -> Result<Vec<(u64, PropertyGuard<'a>)>, StorageError> {
    let mut history = Vec::new();

    // Get current property value
    if let Some(guard) = table.get(entity_id, property_key)? {
        let timestamp = guard.valid_from();
        history.push((timestamp, guard));
    }

    // Note: This implementation returns only the current version of the property.
    // Full version history tracking requires a separate versioning strategy:
    //
    // Option 1: Separate version history table
    //   - Store (entity_id, property_key, version_timestamp) -> PropertyValue
    //   - Append-only writes on property updates
    //   - Query all versions with range scan
    //
    // Option 2: Soft delete with valid_from/valid_to ranges
    //   - Never delete old property values, just mark them superseded
    //   - Add valid_to timestamp to PropertyValue
    //   - Query with timestamp range predicates
    //
    // Option 3: Integrate with Hyperspatial's temporal snapshot system
    //   - Leverage existing entity-level snapshots
    //   - Property history derived from entity snapshots
    //
    // The current design supports temporal queries at a point in time (get_property_at)
    // but does not persist historical versions. Applications requiring full audit trails
    // should implement one of the above strategies at the Hyperspatial level.

    Ok(history)
}

/// Gets all properties for an entity as they existed at a specific timestamp.
///
/// This reconstructs the complete property state of an entity at a point in time.
///
/// # Arguments
///
/// * `table` - The property table to query
/// * `entity_id` - The UUID of the entity
/// * `timestamp` - The timestamp to query (nanoseconds since Unix epoch)
///
/// # Returns
///
/// A vector of (property_key, PropertyGuard) tuples for all properties that were
/// valid at the given timestamp.
///
/// # Example
///
/// ```no_run
/// use manifold_properties::{PropertyTableRead, temporal::get_all_properties_at};
/// use uuid::Uuid;
/// # use manifold::Database;
/// # let db = Database::builder().create_temp().unwrap();
/// # let txn = db.begin_read().unwrap();
/// # let table = PropertyTableRead::open(&txn, "props").unwrap();
///
/// let entity = Uuid::new_v4();
/// let timestamp = 1000000000;
///
/// let properties = get_all_properties_at(&table, &entity, timestamp).unwrap();
/// for (key, guard) in properties {
///     println!("{}: {:?}", key, guard.value());
/// }
/// ```
pub fn get_all_properties_at<'a>(
    table: &'a PropertyTableRead,
    entity_id: &Uuid,
    timestamp: u64,
) -> Result<Vec<(String, PropertyGuard<'a>)>, StorageError> {
    let mut results = Vec::new();

    // Get all current properties for the entity
    let all_properties = table.get_all(entity_id)?;

    // Filter to only those valid at the requested timestamp
    for (key, guard) in all_properties {
        if guard.valid_from() <= timestamp {
            results.push((key, guard));
        }
    }

    Ok(results)
}

/// Checks if a property existed at a specific timestamp.
///
/// This is a convenience function that returns true if the property had a value
/// (including Null) at the given timestamp.
///
/// # Arguments
///
/// * `table` - The property table to query
/// * `entity_id` - The UUID of the entity
/// * `property_key` - The property name
/// * `timestamp` - The timestamp to check (nanoseconds since Unix epoch)
///
/// # Example
///
/// ```no_run
/// use manifold_properties::{PropertyTableRead, temporal::property_existed_at};
/// use uuid::Uuid;
/// # use manifold::Database;
/// # let db = Database::builder().create_temp().unwrap();
/// # let txn = db.begin_read().unwrap();
/// # let table = PropertyTableRead::open(&txn, "props").unwrap();
///
/// let entity = Uuid::new_v4();
/// let timestamp = 1000000000;
///
/// if property_existed_at(&table, &entity, "age", timestamp).unwrap() {
///     println!("Property 'age' existed at timestamp {}", timestamp);
/// }
/// ```
pub fn property_existed_at(
    table: &PropertyTableRead,
    entity_id: &Uuid,
    property_key: &str,
    timestamp: u64,
) -> Result<bool, StorageError> {
    Ok(get_property_at(table, entity_id, property_key, timestamp)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PropertyValue;
    use crate::table::PropertyTable;
    use manifold::{Database, ReadableDatabase};
    use tempfile::TempDir;

    fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::builder().create(&db_path).unwrap();
        (temp_dir, db)
    }

    #[test]
    fn test_get_property_at_valid() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Create property with specific valid_from timestamp
        let valid_from = 1000;
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            let prop = PropertyValue::new_integer_with_timestamps(42, 1000, valid_from);
            table.set(&entity_id, "age", prop).unwrap();
        }
        write_txn.commit().unwrap();

        // Query at timestamp after valid_from
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let result = get_property_at(&table, &entity_id, "age", 2000).unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().as_i64(), Some(42));
    }

    #[test]
    fn test_get_property_at_before_valid() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Create property with valid_from = 1000
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            let prop = PropertyValue::new_integer_with_timestamps(42, 1000, 1000);
            table.set(&entity_id, "age", prop).unwrap();
        }
        write_txn.commit().unwrap();

        // Query at timestamp before valid_from
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let result = get_property_at(&table, &entity_id, "age", 500).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_get_property_at_exact_timestamp() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let valid_from = 1000;
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            let prop = PropertyValue::new_integer_with_timestamps(42, 1000, valid_from);
            table.set(&entity_id, "age", prop).unwrap();
        }
        write_txn.commit().unwrap();

        // Query at exact valid_from timestamp
        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let result = get_property_at(&table, &entity_id, "age", valid_from).unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().as_i64(), Some(42));
    }

    #[test]
    fn test_get_property_history() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            let prop = PropertyValue::new_integer_with_timestamps(42, 1000, 1000);
            table.set(&entity_id, "age", prop).unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();
        let history = get_property_history(&table, &entity_id, "age").unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, 1000); // valid_from timestamp
        assert_eq!(history[0].1.as_i64(), Some(42));
    }

    #[test]
    fn test_get_property_history_nonexistent() {
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
        let history = get_property_history(&table, &entity_id, "nonexistent").unwrap();

        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_get_all_properties_at() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        // Set up properties with different valid_from timestamps
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(
                    &entity_id,
                    "age",
                    PropertyValue::new_integer_with_timestamps(30, 1000, 1000),
                )
                .unwrap();
            table
                .set(
                    &entity_id,
                    "name",
                    PropertyValue::new_string_with_timestamps("Alice", 2000, 2000),
                )
                .unwrap();
            table
                .set(
                    &entity_id,
                    "score",
                    PropertyValue::new_float_with_timestamps(95.5, 3000, 3000),
                )
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();

        // Query at timestamp 2500 (age and name should exist, but not score)
        let properties = get_all_properties_at(&table, &entity_id, 2500).unwrap();
        assert_eq!(properties.len(), 2);

        let prop_names: Vec<_> = properties.iter().map(|(k, _)| k.as_str()).collect();
        assert!(prop_names.contains(&"age"));
        assert!(prop_names.contains(&"name"));
        assert!(!prop_names.contains(&"score"));
    }

    #[test]
    fn test_property_existed_at() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(
                    &entity_id,
                    "age",
                    PropertyValue::new_integer_with_timestamps(30, 1000, 1000),
                )
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();

        // Should exist after valid_from
        assert!(property_existed_at(&table, &entity_id, "age", 2000).unwrap());

        // Should not exist before valid_from
        assert!(!property_existed_at(&table, &entity_id, "age", 500).unwrap());

        // Should not exist if never created
        assert!(!property_existed_at(&table, &entity_id, "nonexistent", 2000).unwrap());
    }

    #[test]
    fn test_temporal_with_null_property() {
        let (_temp, db) = setup_test_db();
        let entity_id = Uuid::new_v4();

        let write_txn = db.begin_write().unwrap();
        {
            let mut table = PropertyTable::open(&write_txn, "properties").unwrap();
            table
                .set(
                    &entity_id,
                    "optional",
                    PropertyValue::new_null_with_timestamps(1000, 1000),
                )
                .unwrap();
        }
        write_txn.commit().unwrap();

        let read_txn = db.begin_read().unwrap();
        let table = PropertyTableRead::open(&read_txn, "properties").unwrap();

        // Null property should still be queryable
        let result = get_property_at(&table, &entity_id, "optional", 2000).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().is_null());

        // Should exist at timestamp
        assert!(property_existed_at(&table, &entity_id, "optional", 2000).unwrap());
    }
}
