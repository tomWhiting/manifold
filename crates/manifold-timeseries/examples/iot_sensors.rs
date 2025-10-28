//! IoT sensor data collection example.
//!
//! This example demonstrates:
//! - Collecting data from multiple IoT sensors
//! - Using batch write operations for high throughput
//! - Querying data from specific sensors
//! - Handling different sensor types (temperature, humidity, pressure)
//!
//! Run with: cargo run --example iot_sensors

use manifold::column_family::ColumnFamilyDatabase;
use manifold_timeseries::{AbsoluteEncoding, TimeSeriesTable, TimeSeriesTableRead};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== IoT Sensor Data Collection Example ===\n");

    let dir = tempdir()?;
    let db = ColumnFamilyDatabase::open(dir.path().join("iot.db"))?;
    let cf = db.column_family_or_create("sensor_data")?;

    let base_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis() as u64;

    println!("Simulating IoT sensor network...\n");
    println!("Sensors:");
    println!("  - sensor_01: Temperature (living room)");
    println!("  - sensor_02: Temperature (bedroom)");
    println!("  - sensor_03: Humidity (living room)");
    println!("  - sensor_04: Pressure (outdoor)");
    println!();

    // Collect sensor data using batch writes
    {
        let write_txn = cf.begin_write()?;
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "sensors")?;

        // Simulate 100 readings from 4 sensors (400 total data points)
        let mut batch = Vec::new();
        
        for i in 0..100 {
            let timestamp = base_time + (i * 5000); // 5 second intervals
            
            // Temperature sensors (°C)
            let temp_living = 22.0 + (i as f32 * 0.1).sin() * 2.0;
            let temp_bedroom = 20.0 + (i as f32 * 0.15).cos() * 1.5;
            
            // Humidity sensor (%)
            let humidity = 45.0 + (i as f32 * 0.08).sin() * 10.0;
            
            // Pressure sensor (hPa)
            let pressure = 1013.0 + (i as f32 * 0.05).cos() * 5.0;

            batch.push(("sensor_01.temperature", timestamp, temp_living));
            batch.push(("sensor_02.temperature", timestamp, temp_bedroom));
            batch.push(("sensor_03.humidity", timestamp, humidity));
            batch.push(("sensor_04.pressure", timestamp, pressure));
        }

        println!("Writing {} sensor readings in batch...", batch.len());
        
        // Batch write is more efficient than individual writes
        ts.write_batch(batch, false)?; // false = data not pre-sorted
        
        println!("Batch write complete!\n");
        
        drop(ts);
        write_txn.commit()?;
    }

    // Query and analyze sensor data
    {
        let read_txn = cf.begin_read()?;
        let ts_read = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "sensors")?;

        // Query living room temperature for last 30 seconds
        println!("Living room temperature (last 30 seconds):");
        let query_start = base_time + 470_000; // Last 6 readings
        let query_end = base_time + 500_000;

        let mut temps = Vec::new();
        for result in ts_read.range("sensor_01.temperature", query_start, query_end)? {
            let (timestamp, value) = result?;
            temps.push(value);
            let offset = (timestamp - base_time) / 1000;
            println!("  t+{}s: {:.1}°C", offset, value);
        }

        if !temps.is_empty() {
            let avg = temps.iter().sum::<f32>() / temps.len() as f32;
            println!("  Average: {:.1}°C\n", avg);
        }

        // Query humidity levels
        println!("Humidity levels (last 40 seconds):");
        let humidity_start = base_time + 460_000;
        let humidity_end = base_time + 500_000;

        let mut humidities = Vec::new();
        for result in ts_read.range("sensor_03.humidity", humidity_start, humidity_end)? {
            let (timestamp, value) = result?;
            humidities.push(value);
            let offset = (timestamp - base_time) / 1000;
            println!("  t+{}s: {:.1}%", offset, value);
        }

        if !humidities.is_empty() {
            let avg = humidities.iter().sum::<f32>() / humidities.len() as f32;
            let min = humidities.iter().cloned().fold(f32::INFINITY, f32::min);
            let max = humidities.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            println!("  Min: {:.1}%, Max: {:.1}%, Avg: {:.1}%\n", min, max, avg);
        }

        // Compare both temperature sensors
        println!("Temperature comparison (sample at t+100s):");
        let sample_time = base_time + 100_000;
        
        if let Some(living) = ts_read.get("sensor_01.temperature", sample_time)? {
            println!("  Living room: {:.1}°C", living);
        }
        
        if let Some(bedroom) = ts_read.get("sensor_02.temperature", sample_time)? {
            println!("  Bedroom: {:.1}°C", bedroom);
        }

        println!("\nTotal sensor readings: {}", ts_read.len()?);
    }

    println!("\nExample complete!\n");

    Ok(())
}
