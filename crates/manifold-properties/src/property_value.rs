//! PropertyValue enum with native type variants.
//!
//! This module defines the core PropertyValue type that replaces string-based
//! property storage with native typed variants.
//!
//! IMPLEMENTATION REQUIRED:
//! - PropertyValue enum with variants: Integer(i64), Float(f64), Boolean(bool), String(String), Null
//! - Each variant includes: value, updated_at: u64, valid_from: u64
//! - Conversion methods: from_string(), to_string(), type_name()
//! - Comparison methods for filtering
//! - Default implementations

// Implementation goes here
