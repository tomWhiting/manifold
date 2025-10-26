use manifold::column_family::ColumnFamilyDatabase;
use manifold::{ReadableTableMetadata, TableDefinition};
use std::error::Error;
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use tempfile::NamedTempFile;

// Example: E-commerce application using column families for different domains
//
// This example demonstrates:
// 1. Creating column families for different business domains
// 2. Using multiple tables within each column family
// 3. Concurrent writes to different column families
// 4. Reading data across column families

// Define tables for the "users" column family
const USERS_TABLE: TableDefinition<u64, &str> = TableDefinition::new("users");
const USER_EMAILS: TableDefinition<u64, &str> = TableDefinition::new("user_emails");

// Define tables for the "products" column family
const PRODUCTS_TABLE: TableDefinition<u64, &str> = TableDefinition::new("products");
const PRODUCT_PRICES: TableDefinition<u64, u64> = TableDefinition::new("product_prices");

// Define tables for the "orders" column family
const ORDERS_TABLE: TableDefinition<u64, &str> = TableDefinition::new("orders");

fn main() -> Result<(), Box<dyn Error>> {
    println!("Column Family Example: E-commerce Application");
    println!("==============================================\n");

    // Use a temporary file for this example to ensure clean state
    let tmpfile = NamedTempFile::new()?;
    let db_path = tmpfile.path();

    // Create or open a column family database
    let db = ColumnFamilyDatabase::open(db_path)?;

    // Create column families for different domains
    println!("Creating column families...");
    db.create_column_family("users", Some(512 * 1024 * 1024))?; // 512MB for users
    db.create_column_family("products", Some(512 * 1024 * 1024))?; // 512MB for products
    db.create_column_family("orders", Some(1024 * 1024 * 1024))?; // 1GB for orders

    println!("Column families created: {:?}\n", db.list_column_families());

    // Demonstrate concurrent writes to different column families
    println!("Demonstrating concurrent writes...");
    demonstrate_concurrent_writes(&db)?;

    // Demonstrate multiple tables within a column family
    println!("\nDemonstrating multiple tables within column families...");
    demonstrate_multiple_tables(&db)?;

    // Demonstrate reading data
    println!("\nReading data from column families...");
    demonstrate_reads(&db)?;

    println!("\nExample completed successfully!");
    Ok(())
}

fn demonstrate_concurrent_writes(db: &ColumnFamilyDatabase) -> Result<(), Box<dyn Error>> {
    let db = Arc::new(db);
    let start = Instant::now();

    // Spawn threads to write to different column families concurrently
    let mut handles = vec![];

    // Thread 1: Write users
    {
        let users_cf = db.column_family("users")?;
        let handle = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
            let write_txn = users_cf.begin_write()?;
            {
                let mut table = write_txn.open_table(USERS_TABLE)?;
                for i in 1..=1000 {
                    table.insert(&i, &format!("user_{i}").as_str())?;
                }
            }
            write_txn.commit()?;
            println!("  [Users] Inserted 1000 users");
            Ok(())
        });
        handles.push(handle);
    }

    // Thread 2: Write products
    {
        let products_cf = db.column_family("products")?;
        let handle = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
            let write_txn = products_cf.begin_write()?;
            {
                let mut table = write_txn.open_table(PRODUCTS_TABLE)?;
                for i in 1..=500 {
                    table.insert(&i, &format!("product_{i}").as_str())?;
                }
            }
            write_txn.commit()?;
            println!("  [Products] Inserted 500 products");
            Ok(())
        });
        handles.push(handle);
    }

    // Thread 3: Write orders
    {
        let orders_cf = db.column_family("orders")?;
        let handle = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
            let write_txn = orders_cf.begin_write()?;
            {
                let mut table = write_txn.open_table(ORDERS_TABLE)?;
                for i in 1..=2000 {
                    table.insert(&i, &format!("order_{i}").as_str())?;
                }
            }
            write_txn.commit()?;
            println!("  [Orders] Inserted 2000 orders");
            Ok(())
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    #[allow(clippy::question_mark)]
    for handle in handles {
        if let Err(e) = handle.join().expect("Thread panicked") {
            return Err(e);
        }
    }

    let elapsed = start.elapsed();
    println!(
        "  Concurrent writes completed in {:.2}ms",
        elapsed.as_secs_f64() * 1000.0
    );

    Ok(())
}

fn demonstrate_multiple_tables(db: &ColumnFamilyDatabase) -> Result<(), Box<dyn Error>> {
    // Write to multiple tables within the users column family
    let users_cf = db.column_family("users")?;
    let write_txn = users_cf.begin_write()?;
    {
        // Write to users table
        let mut users_table = write_txn.open_table(USERS_TABLE)?;
        users_table.insert(&9001, "alice")?;
        users_table.insert(&9002, "bob")?;

        // Write to user_emails table (different table, same column family)
        let mut emails_table = write_txn.open_table(USER_EMAILS)?;
        emails_table.insert(&9001, "alice@example.com")?;
        emails_table.insert(&9002, "bob@example.com")?;

        println!("  [Users CF] Written to 2 tables atomically");
    }
    write_txn.commit()?;

    // Similarly for products
    let products_cf = db.column_family("products")?;
    let write_txn = products_cf.begin_write()?;
    {
        let mut products_table = write_txn.open_table(PRODUCTS_TABLE)?;
        products_table.insert(&5001, "laptop")?;
        products_table.insert(&5002, "keyboard")?;

        let mut prices_table = write_txn.open_table(PRODUCT_PRICES)?;
        prices_table.insert(&5001, 1200_u64)?; // $1200
        prices_table.insert(&5002, 80_u64)?; // $80

        println!("  [Products CF] Written to 2 tables atomically");
    }
    write_txn.commit()?;

    Ok(())
}

fn demonstrate_reads(db: &ColumnFamilyDatabase) -> Result<(), Box<dyn Error>> {
    // Read from users column family
    let users_cf = db.column_family("users")?;
    let read_txn = users_cf.begin_read()?;
    let users_table = read_txn.open_table(USERS_TABLE)?;
    let emails_table = read_txn.open_table(USER_EMAILS)?;

    if let Some(user) = users_table.get(&9001)? {
        let email = emails_table.get(&9001)?;
        let email_str = email.as_ref().map(|e| e.value()).unwrap_or("no email");
        println!("  User 9001: {} ({})", user.value(), email_str);
    }

    // Count total users
    let user_count = users_table.len()?;
    println!("  Total users: {user_count}");

    // Read from products column family
    let products_cf = db.column_family("products")?;
    let read_txn = products_cf.begin_read()?;
    let products_table = read_txn.open_table(PRODUCTS_TABLE)?;
    let prices_table = read_txn.open_table(PRODUCT_PRICES)?;

    if let Some(product) = products_table.get(&5001)? {
        let price = prices_table.get(&5001)?;
        println!(
            "  Product 5001: {} (${:?})",
            product.value(),
            price.map(|p| p.value())
        );
    }

    let product_count = products_table.len()?;
    println!("  Total products: {product_count}");

    // Read from orders column family
    let orders_cf = db.column_family("orders")?;
    let read_txn = orders_cf.begin_read()?;
    let orders_table = read_txn.open_table(ORDERS_TABLE)?;
    let order_count = orders_table.len()?;
    println!("  Total orders: {order_count}");

    Ok(())
}
