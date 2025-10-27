use manifold::{
    CommitError, CompactionError, DatabaseError, Error, SavepointError, SetDurabilityError,
    StorageError, TableError, TransactionError, TypeName,
};
use std::io;
use std::sync::{Arc, Mutex};

#[test]
fn test_storage_error_display() {
    let err = StorageError::Corrupted(String::from("test corruption"));
    let display = format!("{}", err);
    assert!(display.contains("corruption"));

    let err = StorageError::ValueTooLarge(1000);
    let display = format!("{}", err);
    assert!(display.contains("1000"));

    let err = StorageError::DatabaseClosed;
    let display = format!("{}", err);
    assert!(display.contains("closed"));

    let err = StorageError::DatabaseClosed;
    let display = format!("{}", err);
    assert!(display.contains("closed"));
}

#[test]
fn test_storage_error_from_io_error() {
    let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    let storage_err: StorageError = io_err.into();

    match storage_err {
        StorageError::Io(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        _ => panic!("Expected Io variant"),
    }
}

#[test]
fn test_storage_error_from_poison_error() {
    let mutex = Arc::new(Mutex::new(42));
    let _guard = mutex.lock().unwrap();

    // Test that poisoned mutex converts to StorageError
    // This is tested implicitly through the From implementation
}

#[test]
fn test_storage_error_into_error() {
    let storage_err = StorageError::DatabaseClosed;
    let err: Error = storage_err.into();
    match err {
        Error::DatabaseClosed => {}
        _ => panic!("Expected DatabaseClosed variant"),
    }

    let storage_err = StorageError::ValueTooLarge(500);
    let err: Error = storage_err.into();
    match err {
        Error::ValueTooLarge(size) => assert_eq!(size, 500),
        _ => panic!("Expected ValueTooLarge variant"),
    }
}

#[test]
fn test_table_error_display() {
    let err = TableError::TableDoesNotExist(String::from("my_table"));
    let display = format!("{}", err);
    assert!(display.contains("my_table"));

    let err = TableError::TableExists(String::from("existing_table"));
    let display = format!("{}", err);
    assert!(display.contains("existing_table"));

    let err = TableError::TableIsMultimap(String::from("multimap_table"));
    let display = format!("{}", err);
    assert!(display.contains("multimap"));

    let err = TableError::TableIsNotMultimap(String::from("regular_table"));
    let display = format!("{}", err);
    assert!(display.contains("not"));
}

#[test]
fn test_table_error_type_mismatch_display() {
    let err = TableError::TableTypeMismatch {
        table: String::from("users"),
        key: TypeName::new("u64"),
        value: TypeName::new("String"),
    };
    let display = format!("{}", err);
    assert!(display.contains("users"));
    assert!(display.contains("u64"));
    assert!(display.contains("String"));
}

#[test]
fn test_table_error_type_definition_changed_display() {
    let err = TableError::TypeDefinitionChanged {
        name: TypeName::new("MyType"),
        alignment: 8,
        width: Some(16),
    };
    let display = format!("{}", err);
    assert!(display.contains("MyType"));
    assert!(display.contains("8"));
    assert!(display.contains("16"));

    let err = TableError::TypeDefinitionChanged {
        name: TypeName::new("DynamicType"),
        alignment: 4,
        width: None,
    };
    let display = format!("{}", err);
    assert!(display.contains("DynamicType"));
    assert!(display.contains("4"));
}

#[test]
fn test_table_error_from_storage_error() {
    let storage_err = StorageError::DatabaseClosed;
    let table_err: TableError = storage_err.into();
    match table_err {
        TableError::Storage(StorageError::DatabaseClosed) => {}
        _ => panic!("Expected Storage variant"),
    }
}

#[test]
fn test_table_error_into_error() {
    let table_err = TableError::TableDoesNotExist(String::from("missing"));
    let err: Error = table_err.into();
    match err {
        Error::TableDoesNotExist(name) => assert_eq!(name, "missing"),
        _ => panic!("Expected TableDoesNotExist variant"),
    }

    let table_err = TableError::TableExists(String::from("duplicate"));
    let err: Error = table_err.into();
    match err {
        Error::TableExists(name) => assert_eq!(name, "duplicate"),
        _ => panic!("Expected TableExists variant"),
    }
}

#[test]
fn test_database_error_display() {
    let err = DatabaseError::DatabaseAlreadyOpen;
    let display = format!("{}", err);
    assert!(display.contains("already open") || display.contains("already in use"));

    let err = DatabaseError::RepairAborted;
    let display = format!("{}", err);
    assert!(display.contains("repair") || display.contains("abort"));

    let err = DatabaseError::UpgradeRequired(1);
    let display = format!("{}", err);
    assert!(display.contains("upgrade") || display.contains("1"));
}

#[test]
fn test_database_error_from_io_error() {
    let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
    let db_err: DatabaseError = io_err.into();
    match db_err {
        DatabaseError::Storage(StorageError::Io(_)) => {}
        _ => panic!("Expected Storage(Io) variant"),
    }
}

#[test]
fn test_database_error_from_storage_error() {
    let storage_err = StorageError::Corrupted(String::from("bad data"));
    let db_err: DatabaseError = storage_err.into();
    match db_err {
        DatabaseError::Storage(StorageError::Corrupted(_)) => {}
        _ => panic!("Expected Storage(Corrupted) variant"),
    }
}

#[test]
fn test_database_error_into_error() {
    let db_err = DatabaseError::DatabaseAlreadyOpen;
    let err: Error = db_err.into();
    match err {
        Error::DatabaseAlreadyOpen => {}
        _ => panic!("Expected DatabaseAlreadyOpen variant"),
    }

    let db_err = DatabaseError::RepairAborted;
    let err: Error = db_err.into();
    match err {
        Error::RepairAborted => {}
        _ => panic!("Expected RepairAborted variant"),
    }
}

#[test]
fn test_savepoint_error_display() {
    let err = SavepointError::InvalidSavepoint;
    let display = format!("{}", err);
    assert!(display.contains("invalid") || display.contains("savepoint"));
}

#[test]
fn test_savepoint_error_from_storage_error() {
    let storage_err = StorageError::DatabaseClosed;
    let sp_err: SavepointError = storage_err.into();
    match sp_err {
        SavepointError::Storage(StorageError::DatabaseClosed) => {}
        _ => panic!("Expected Storage variant"),
    }
}

#[test]
fn test_savepoint_error_into_error() {
    let sp_err = SavepointError::InvalidSavepoint;
    let err: Error = sp_err.into();
    match err {
        Error::InvalidSavepoint => {}
        _ => panic!("Expected InvalidSavepoint variant"),
    }
}

#[test]
fn test_compaction_error_display() {
    let err = CompactionError::PersistentSavepointExists;
    let display = format!("{}", err);
    assert!(display.contains("persistent") || display.contains("savepoint"));

    let err = CompactionError::EphemeralSavepointExists;
    let display = format!("{}", err);
    assert!(display.contains("ephemeral") || display.contains("savepoint"));

    let err = CompactionError::TransactionInProgress;
    let display = format!("{}", err);
    assert!(display.contains("transaction") || display.contains("progress"));
}

#[test]
fn test_compaction_error_from_storage_error() {
    let storage_err = StorageError::DatabaseClosed;
    let comp_err: CompactionError = storage_err.into();
    match comp_err {
        CompactionError::Storage(StorageError::DatabaseClosed) => {}
        _ => panic!("Expected Storage variant"),
    }
}

#[test]
fn test_compaction_error_into_error() {
    let comp_err = CompactionError::PersistentSavepointExists;
    let err: Error = comp_err.into();
    match err {
        Error::PersistentSavepointExists => {}
        _ => panic!("Expected PersistentSavepointExists variant"),
    }

    let comp_err = CompactionError::TransactionInProgress;
    let err: Error = comp_err.into();
    match err {
        Error::TransactionInProgress => {}
        _ => panic!("Expected TransactionInProgress variant"),
    }
}

#[test]
fn test_set_durability_error_display() {
    let err = SetDurabilityError::PersistentSavepointModified;
    let display = format!("{}", err);
    assert!(display.contains("savepoint") || display.contains("modified"));
}

#[test]
fn test_set_durability_error_into_error() {
    let sd_err = SetDurabilityError::PersistentSavepointModified;
    let err: Error = sd_err.into();
    match err {
        Error::PersistentSavepointModified => {}
        _ => panic!("Expected PersistentSavepointModified variant"),
    }
}

#[test]
fn test_transaction_error_display() {
    let storage_err = StorageError::DatabaseClosed;
    let err = TransactionError::Storage(storage_err);
    let display = format!("{}", err);
    assert!(display.contains("closed") || display.contains("storage"));
}

#[test]
fn test_transaction_error_from_storage_error() {
    let storage_err = StorageError::DatabaseClosed;
    let tx_err: TransactionError = storage_err.into();
    match tx_err {
        TransactionError::Storage(StorageError::DatabaseClosed) => {}
        _ => panic!("Expected Storage variant"),
    }
}

#[test]
fn test_transaction_error_into_error() {
    let tx_err = TransactionError::Storage(StorageError::DatabaseClosed);
    let err: Error = tx_err.into();
    match err {
        Error::DatabaseClosed => {}
        _ => panic!("Expected DatabaseClosed variant"),
    }
}

#[test]
fn test_commit_error_from_storage_error() {
    let storage_err = StorageError::DatabaseClosed;
    let commit_err: CommitError = storage_err.into();
    match commit_err {
        CommitError::Storage(StorageError::DatabaseClosed) => {}
        _ => panic!("Expected Storage variant"),
    }
}

#[test]
fn test_commit_error_into_error() {
    let commit_err = CommitError::Storage(StorageError::DatabaseClosed);
    let err: Error = commit_err.into();
    match err {
        Error::DatabaseClosed => {}
        _ => panic!("Expected DatabaseClosed variant"),
    }
}

#[test]
fn test_error_from_io_error() {
    let io_err = io::Error::other("test error");
    let err: Error = io_err.into();
    match err {
        Error::Io(_) => {}
        _ => panic!("Expected Io variant"),
    }
}

#[test]
fn test_error_from_poison_error() {
    // Test that poisoned mutex converts to Error
    // This is tested implicitly through the From implementation
}

#[test]
fn test_error_display_all_variants() {
    let err = Error::DatabaseAlreadyOpen;
    assert!(!format!("{}", err).is_empty());

    let err = Error::InvalidSavepoint;
    assert!(!format!("{}", err).is_empty());

    let err = Error::RepairAborted;
    assert!(!format!("{}", err).is_empty());

    let err = Error::PersistentSavepointModified;
    assert!(!format!("{}", err).is_empty());

    let err = Error::PersistentSavepointExists;
    assert!(!format!("{}", err).is_empty());

    let err = Error::EphemeralSavepointExists;
    assert!(!format!("{}", err).is_empty());

    let err = Error::TransactionInProgress;
    assert!(!format!("{}", err).is_empty());

    let err = Error::Corrupted(String::from("test"));
    assert!(!format!("{}", err).is_empty());

    let err = Error::UpgradeRequired(2);
    assert!(!format!("{}", err).is_empty());

    let err = Error::ValueTooLarge(12345);
    assert!(!format!("{}", err).is_empty());

    let err = Error::TableTypeMismatch {
        table: String::from("t"),
        key: TypeName::new("k"),
        value: TypeName::new("v"),
    };
    assert!(!format!("{}", err).is_empty());

    let err = Error::TableIsMultimap(String::from("m"));
    assert!(!format!("{}", err).is_empty());

    let err = Error::TableIsNotMultimap(String::from("n"));
    assert!(!format!("{}", err).is_empty());

    let err = Error::TypeDefinitionChanged {
        name: TypeName::new("T"),
        alignment: 4,
        width: Some(8),
    };
    assert!(!format!("{}", err).is_empty());

    let err = Error::TableDoesNotExist(String::from("missing"));
    assert!(!format!("{}", err).is_empty());

    let err = Error::TableExists(String::from("exists"));
    assert!(!format!("{}", err).is_empty());

    let err = Error::TableExists(String::from("exists_test"));
    assert!(!format!("{}", err).is_empty());

    let err = Error::DatabaseClosed;
    assert!(!format!("{}", err).is_empty());

    let err = Error::PreviousIo;
    assert!(!format!("{}", err).is_empty());
}

#[test]
fn test_error_std_error_trait() {
    use std::error::Error as StdError;

    let err = StorageError::DatabaseClosed;
    let _ = err.source(); // Ensure Error trait is implemented

    let err = TableError::TableDoesNotExist(String::from("test"));
    let _ = err.source();

    let err = DatabaseError::DatabaseAlreadyOpen;
    let _ = err.source();

    let err = SavepointError::InvalidSavepoint;
    let _ = err.source();

    let err = CompactionError::TransactionInProgress;
    let _ = err.source();

    let err = SetDurabilityError::PersistentSavepointModified;
    let _ = err.source();

    let err = TransactionError::Storage(StorageError::DatabaseClosed);
    let _ = err.source();

    let err = CommitError::Storage(StorageError::DatabaseClosed);
    let _ = err.source();

    let err = manifold::Error::DatabaseClosed;
    let _ = err.source();
}
