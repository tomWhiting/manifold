//! Manifold Value trait implementations for PropertyValue serialization.
//!
//! This module implements Manifold's native Value trait for PropertyValue,
//! enabling zero-copy deserialization and efficient storage.
//!
//! IMPLEMENTATION REQUIRED:
//! - Implement Value trait for PropertyValue enum
//! - Handle variant discriminant encoding
//! - Zero-copy SelfType<'a> for fixed-width variants
//! - String handling for variable-width variant
//! - Temporal metadata encoding

// Implementation goes here
