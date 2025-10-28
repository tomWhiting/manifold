//! Downsampling and retention lifecycle example.
//!
//! This example demonstrates the full lifecycle of time series data:
//! - Writing raw high-resolution data
//! - Downsampling to minute/hour/day aggregates
//! - Applying retention policies to clean up old data
//! - Querying aggregates at different granularities
//!
//! Run with: cargo run --example downsampling_lifecycle

use manifold::column_family::ColumnFamilyDatabase;
use manifold_timeseries::{
    AbsoluteEncoding, Granularity, TimeSeriesTable, TimeSeriesTableRead,
};
use tempfile::tempdir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Downsampling & Retention Lifecycle Example ===\n");

    let dir = tempdir()?;
    let db = ColumnFamilyDatabase::open(dir.path().join("lifecycle.db"))?;
    let cf = db.column_family_or_create("metrics")?;

    // Use a fixed base time for predictable results
    let base_time = 1609459200000u64; // 2021-01-01 00:00:00 UTC

    println!("Step 1: Writing high-resolution raw data\n");
    
    // Write 2 hours of data at 10-second intervals (720 data points)
    {
        let write_txn = cf.begin_write()?;
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;

        for i in 0..720 {
            let timestamp = base_time + (i * 10_000); // 10 second intervals
            
            // Simulate CPU usage with daily pattern
            let hour_of_day = (i * 10) as f32 / 3600.0;
            let base_load = 30.0 + (hour_of_day * 0.5).sin() * 15.0;
            let noise = (i as f32 * 0.3).sin() * 5.0;
            let cpu_usage = base_load + noise;

            ts.write("server1", timestamp, cpu_usage)?;
        }

        println!("Wrote 720 raw data points (2 hours at 10s intervals)\n");
        
        drop(ts);
        write_txn.commit()?;
    }

    println!("Step 2: Downsampling to minute aggregates\n");
    
    // Downsample to 1-minute aggregates
    {
        let write_txn = cf.begin_write()?;
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;

        let start_time = base_time;
        let end_time = base_time + (2 * 60 * 60 * 1000); // 2 hours

        let downsampled = ts.downsample_to_minute(
            "server1",
            start_time,
            end_time,
        )?;

        println!("Created {} minute aggregates\n", downsampled);
        
        drop(ts);
        write_txn.commit()?;
    }

    println!("Step 3: Downsampling to hour aggregates\n");
    
    // Downsample from minutes to hours
    {
        let write_txn = cf.begin_write()?;
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;

        let start_time = base_time;
        let end_time = base_time + (2 * 60 * 60 * 1000);

        let downsampled = ts.downsample_minute_to_hour(
            "server1",
            start_time,
            end_time,
        )?;

        println!("Created {} hour aggregates\n", downsampled);
        
        drop(ts);
        write_txn.commit()?;
    }

    println!("Step 4: Querying aggregates at different granularities\n");
    
    {
        let read_txn = cf.begin_read()?;
        let ts_read = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")?;

        // Show sample raw data
        println!("Raw data (first 5 samples):");
        let raw_start = base_time;
        let raw_end = base_time + 60_000; // First minute
        
        let mut count = 0;
        for result in ts_read.range("server1", raw_start, raw_end)? {
            let (timestamp, value) = result?;
            if count < 5 {
                let offset_sec = (timestamp - base_time) / 1000;
                println!("  t+{}s: {:.2}%", offset_sec, value);
            }
            count += 1;
        }
        println!("  Total raw points in first minute: {}\n", count);

        // Show minute aggregates
        println!("Minute aggregates (first 5 minutes):");
        let minute_start = base_time;
        let minute_end = base_time + (5 * 60 * 1000); // 5 minutes
        
        for result in ts_read.range_aggregates(
            Granularity::Minute,
            "server1",
            minute_start,
            minute_end,
        )? {
            let (timestamp, agg) = result?;
            let offset_min = (timestamp - base_time) / 60_000;
            println!(
                "  Minute {}: min={:.1}%, max={:.1}%, avg={:.1}%, count={}",
                offset_min,
                agg.min,
                agg.max,
                agg.average(),
                agg.count
            );
        }
        println!();

        // Show hour aggregates
        println!("Hour aggregates:");
        let hour_start = base_time;
        let hour_end = base_time + (2 * 60 * 60 * 1000); // 2 hours
        
        for result in ts_read.range_aggregates(
            Granularity::Hour,
            "server1",
            hour_start,
            hour_end,
        )? {
            let (timestamp, agg) = result?;
            let offset_hour = (timestamp - base_time) / 3_600_000;
            println!(
                "  Hour {}: min={:.1}%, max={:.1}%, avg={:.1}%, count={}",
                offset_hour,
                agg.min,
                agg.max,
                agg.average(),
                agg.count
            );
        }
    }

    println!("\nStep 5: Applying retention policies\n");
    
    // Apply retention: keep only last hour of raw data
    {
        let write_txn = cf.begin_write()?;
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "cpu")?;

        // Delete raw data older than 1 hour from end
        let cutoff = base_time + (1 * 60 * 60 * 1000);
        let deleted = ts.delete_before(Granularity::Raw, cutoff)?;

        println!("Deleted {} old raw data points", deleted);
        println!("   (kept only last hour of raw data)\n");
        
        drop(ts);
        write_txn.commit()?;
    }

    // Verify retention
    {
        let read_txn = cf.begin_read()?;
        let ts_read = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "cpu")?;

        let remaining = ts_read.len()?;
        println!("Remaining raw data points: {}", remaining);
        println!("   (should be ~360 points for 1 hour at 10s intervals)\n");
    }

    println!("Lifecycle example complete!\n");
    println!("Summary:");
    println!("  - Raw data collected at high resolution");
    println!("  - Aggregated to minute and hour granularities");
    println!("  - Old raw data cleaned up via retention policy");
    println!("  - Aggregates preserved for long-term analysis\n");

    Ok(())
}
