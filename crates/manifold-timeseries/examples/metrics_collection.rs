//! Real system metrics collection example using sysinfo.
//!
//! This example shows how to:
//! - Collect actual CPU and memory metrics from the running system
//! - Store metrics using AbsoluteEncoding (default)
//! - Query metrics by time range
//! - Compute statistics from stored data
//!
//! Run with: cargo run --example metrics_collection

use manifold::column_family::ColumnFamilyDatabase;
use manifold_timeseries::{AbsoluteEncoding, TimeSeriesTable, TimeSeriesTableRead};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::System;
use tempfile::tempdir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Real System Metrics Collection Example ===\n");

    // Create temporary database
    let dir = tempdir()?;
    let db = ColumnFamilyDatabase::open(dir.path().join("metrics.db"))?;
    let cf = db.column_family_or_create("system_metrics")?;

    println!("Collecting live system metrics for 60 seconds (1 sample/sec)...\n");

    let base_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;

    // Initialize system monitor
    let mut sys = System::new_all();

    // Collect real metrics over 60 seconds
    {
        let write_txn = cf.begin_write()?;
        let mut ts = TimeSeriesTable::<AbsoluteEncoding>::open(&write_txn, "metrics")?;

        for i in 0..60 {
            let timestamp = base_time + (i * 1000);

            // Refresh system information
            sys.refresh_cpu_all();
            sys.refresh_memory();

            // Sleep a bit to get accurate CPU measurements
            thread::sleep(Duration::from_millis(200));

            // Get global CPU usage (average across all cores)
            let cpu_usage = sys.global_cpu_usage();

            // Get memory usage percentage
            let mem_total = sys.total_memory() as f32;
            let mem_used = sys.used_memory() as f32;
            let memory_usage = (mem_used / mem_total) * 100.0;

            // Get available memory
            let mem_available = sys.available_memory() as f32;
            let mem_available_pct = (mem_available / mem_total) * 100.0;

            ts.write("system.cpu.usage", timestamp, cpu_usage)?;
            ts.write("system.memory.usage_pct", timestamp, memory_usage)?;
            ts.write("system.memory.available_pct", timestamp, mem_available_pct)?;

            // Print progress every 10 seconds
            if (i + 1) % 10 == 0 {
                println!("  [{}/60] CPU: {:.1}%, Memory: {:.1}% used, {:.1}% available",
                    i + 1, cpu_usage, memory_usage, mem_available_pct);
            }

            // Sleep for remaining time to hit 1 second interval
            thread::sleep(Duration::from_millis(800));
        }

        println!("\nWrote 180 metric data points (60 samples x 3 metrics)");

        drop(ts);
        write_txn.commit()?;
    }

    println!("\n=== Analyzing Collected Metrics ===\n");

    // Read and analyze metrics
    {
        let read_txn = cf.begin_read()?;
        let ts_read = TimeSeriesTableRead::<AbsoluteEncoding>::open(&read_txn, "metrics")?;

        // Analyze CPU usage over full period
        println!("CPU Usage Analysis:");
        let cpu_start = base_time;
        let cpu_end = base_time + 60_000;

        let mut cpu_values = Vec::new();
        for result in ts_read.range("system.cpu.usage", cpu_start, cpu_end)? {
            let (_timestamp, value) = result?;
            cpu_values.push(value);
        }

        if !cpu_values.is_empty() {
            let cpu_min = cpu_values.iter().cloned().fold(f32::INFINITY, f32::min);
            let cpu_max = cpu_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let cpu_avg = cpu_values.iter().sum::<f32>() / cpu_values.len() as f32;

            println!("  Samples: {}", cpu_values.len());
            println!("  Min: {:.2}%", cpu_min);
            println!("  Max: {:.2}%", cpu_max);
            println!("  Avg: {:.2}%", cpu_avg);
        }

        // Analyze memory usage
        println!("\nMemory Usage Analysis:");
        let mut mem_values = Vec::new();
        for result in ts_read.range("system.memory.usage_pct", cpu_start, cpu_end)? {
            let (_timestamp, value) = result?;
            mem_values.push(value);
        }

        if !mem_values.is_empty() {
            let mem_min = mem_values.iter().cloned().fold(f32::INFINITY, f32::min);
            let mem_max = mem_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let mem_avg = mem_values.iter().sum::<f32>() / mem_values.len() as f32;

            println!("  Samples: {}", mem_values.len());
            println!("  Min: {:.2}%", mem_min);
            println!("  Max: {:.2}%", mem_max);
            println!("  Avg: {:.2}%", mem_avg);
        }

        // Show last 10 seconds of data
        println!("\nLast 10 Seconds Detail:");
        let detail_start = base_time + 50_000;
        let detail_end = base_time + 60_000;

        let mut detail_points = Vec::new();
        for result in ts_read.range("system.cpu.usage", detail_start, detail_end)? {
            let (timestamp, cpu) = result?;
            let mem = ts_read.get("system.memory.usage_pct", timestamp)?.unwrap_or(0.0);
            detail_points.push((timestamp, cpu, mem));
        }

        for (timestamp, cpu, mem) in detail_points.iter().take(10) {
            let offset = (timestamp - base_time) / 1000;
            println!("  t+{}s: CPU {:.1}%, Memory {:.1}%", offset, cpu, mem);
        }

        println!("\nTotal data points stored: {}", ts_read.len()?);
    }

    println!("\nExample complete!\n");

    Ok(())
}
