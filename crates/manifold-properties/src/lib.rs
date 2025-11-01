//! # manifold-properties
//!
//! Type-safe property storage optimizations for the Manifold embedded database.
//!
//! This crate provides efficient property storage using native Manifold types instead of
//! string-based serialization. Properties are stored with their actual types (integers,
//! floats, booleans) for zero-copy access and optimal storage efficiency.
//!
//! ## Performance Characteristics
//!
//! **Storage Reduction:**
//! - Numeric properties: 50-60% smaller than bincode
//! - Overall (50% numeric): 25-30% reduction
//! - Eliminates data_type string storage (6-15 bytes saved per property)
//!
//! **Query Performance:**
//! - Numeric comparisons: 3-8x faster (no string parsing)
//! - Zero-copy reads for Integer, Float, Boolean
//! - Critical for WHERE clauses, filters, cascade aggregations
//!
//! ## Architecture
//!
//! PropertyValue is an enum with native type variants:
//! - Integer(i64): 8 bytes, zero-copy
//! - Float(f64): 8 bytes, zero-copy
//! - Boolean(bool): 1 byte, zero-copy
//! - String(String): variable, no parsing overhead
//! - Null: 0 bytes
//!
//! Each variant includes temporal metadata (updated_at, valid_from) for versioning.
//!
//! ## Module Organization (Documentation Only)
//!
//! This file contains NO implementation code - only module organization and documentation.
//! All implementation is in the individual module files.

pub mod property_value;
pub mod table;
pub mod encoding;
pub mod operations;
pub mod temporal;

pub use property_value::PropertyValue;
pub use table::{PropertyTable, PropertyTableRead, PropertyGuard};
