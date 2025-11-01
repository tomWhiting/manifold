//! PropertyValue enum with native type variants.
//!
//! This module defines the core PropertyValue type that replaces string-based
//! property storage with native typed variants for efficient storage and querying.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A property value with native type variants and temporal tracking.
///
/// Each variant stores the actual value along with temporal metadata for
/// version tracking and point-in-time queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    /// 64-bit signed integer value
    Integer {
        value: i64,
        updated_at: u64,
        valid_from: u64,
    },
    /// 64-bit floating point value
    Float {
        value: f64,
        updated_at: u64,
        valid_from: u64,
    },
    /// Boolean value
    Boolean {
        value: bool,
        updated_at: u64,
        valid_from: u64,
    },
    /// String value (variable length)
    String {
        value: String,
        updated_at: u64,
        valid_from: u64,
    },
    /// Null value (property exists but has no value)
    Null { updated_at: u64, valid_from: u64 },
}

impl PropertyValue {
    /// Creates a new Integer property with current timestamp.
    pub fn new_integer(value: i64) -> Self {
        let now = current_timestamp_nanos();
        Self::Integer {
            value,
            updated_at: now,
            valid_from: now,
        }
    }

    /// Creates a new Integer property with explicit timestamps.
    pub fn new_integer_with_timestamps(value: i64, updated_at: u64, valid_from: u64) -> Self {
        Self::Integer {
            value,
            updated_at,
            valid_from,
        }
    }

    /// Creates a new Float property with current timestamp.
    pub fn new_float(value: f64) -> Self {
        let now = current_timestamp_nanos();
        Self::Float {
            value,
            updated_at: now,
            valid_from: now,
        }
    }

    /// Creates a new Float property with explicit timestamps.
    pub fn new_float_with_timestamps(value: f64, updated_at: u64, valid_from: u64) -> Self {
        Self::Float {
            value,
            updated_at,
            valid_from,
        }
    }

    /// Creates a new Boolean property with current timestamp.
    pub fn new_boolean(value: bool) -> Self {
        let now = current_timestamp_nanos();
        Self::Boolean {
            value,
            updated_at: now,
            valid_from: now,
        }
    }

    /// Creates a new Boolean property with explicit timestamps.
    pub fn new_boolean_with_timestamps(value: bool, updated_at: u64, valid_from: u64) -> Self {
        Self::Boolean {
            value,
            updated_at,
            valid_from,
        }
    }

    /// Creates a new String property with current timestamp.
    pub fn new_string(value: impl Into<String>) -> Self {
        let now = current_timestamp_nanos();
        Self::String {
            value: value.into(),
            updated_at: now,
            valid_from: now,
        }
    }

    /// Creates a new String property with explicit timestamps.
    pub fn new_string_with_timestamps(
        value: impl Into<String>,
        updated_at: u64,
        valid_from: u64,
    ) -> Self {
        Self::String {
            value: value.into(),
            updated_at,
            valid_from,
        }
    }

    /// Creates a new Null property with current timestamp.
    pub fn new_null() -> Self {
        let now = current_timestamp_nanos();
        Self::Null {
            updated_at: now,
            valid_from: now,
        }
    }

    /// Creates a new Null property with explicit timestamps.
    pub fn new_null_with_timestamps(updated_at: u64, valid_from: u64) -> Self {
        Self::Null {
            updated_at,
            valid_from,
        }
    }

    /// Returns the type name of this property value.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Integer { .. } => "Integer",
            Self::Float { .. } => "Float",
            Self::Boolean { .. } => "Boolean",
            Self::String { .. } => "String",
            Self::Null { .. } => "Null",
        }
    }

    /// Returns the value as an i64 if this is an Integer variant.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as an f64 if this is a Float variant.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as a bool if this is a Boolean variant.
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as a string slice if this is a String variant.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String { value, .. } => Some(value.as_str()),
            _ => None,
        }
    }

    /// Returns true if this is a Null variant.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null { .. })
    }

    /// Returns the updated_at timestamp for this property.
    pub fn updated_at(&self) -> u64 {
        match self {
            Self::Integer { updated_at, .. } => *updated_at,
            Self::Float { updated_at, .. } => *updated_at,
            Self::Boolean { updated_at, .. } => *updated_at,
            Self::String { updated_at, .. } => *updated_at,
            Self::Null { updated_at, .. } => *updated_at,
        }
    }

    /// Returns the valid_from timestamp for this property.
    pub fn valid_from(&self) -> u64 {
        match self {
            Self::Integer { valid_from, .. } => *valid_from,
            Self::Float { valid_from, .. } => *valid_from,
            Self::Boolean { valid_from, .. } => *valid_from,
            Self::String { valid_from, .. } => *valid_from,
            Self::Null { valid_from, .. } => *valid_from,
        }
    }

    /// Updates the temporal metadata while preserving the value.
    pub fn with_timestamps(mut self, updated_at: u64, valid_from: u64) -> Self {
        match &mut self {
            Self::Integer {
                updated_at: u,
                valid_from: v,
                ..
            } => {
                *u = updated_at;
                *v = valid_from;
            }
            Self::Float {
                updated_at: u,
                valid_from: v,
                ..
            } => {
                *u = updated_at;
                *v = valid_from;
            }
            Self::Boolean {
                updated_at: u,
                valid_from: v,
                ..
            } => {
                *u = updated_at;
                *v = valid_from;
            }
            Self::String {
                updated_at: u,
                valid_from: v,
                ..
            } => {
                *u = updated_at;
                *v = valid_from;
            }
            Self::Null {
                updated_at: u,
                valid_from: v,
            } => {
                *u = updated_at;
                *v = valid_from;
            }
        }
        self
    }
}

impl fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer { value, .. } => write!(f, "{}", value),
            Self::Float { value, .. } => write!(f, "{}", value),
            Self::Boolean { value, .. } => write!(f, "{}", value),
            Self::String { value, .. } => write!(f, "{}", value),
            Self::Null { .. } => write!(f, "null"),
        }
    }
}

/// Returns the current timestamp in nanoseconds since Unix epoch.
fn current_timestamp_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System time before Unix epoch")
        .as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_creation() {
        let prop = PropertyValue::new_integer(42);
        assert_eq!(prop.as_integer(), Some(42));
        assert_eq!(prop.type_name(), "Integer");
        assert!(prop.updated_at() > 0);
        assert!(prop.valid_from() > 0);
    }

    #[test]
    fn test_integer_with_timestamps() {
        let prop = PropertyValue::new_integer_with_timestamps(42, 1000, 2000);
        assert_eq!(prop.as_integer(), Some(42));
        assert_eq!(prop.updated_at(), 1000);
        assert_eq!(prop.valid_from(), 2000);
    }

    #[test]
    fn test_float_creation() {
        let prop = PropertyValue::new_float(3.14);
        assert_eq!(prop.as_float(), Some(3.14));
        assert_eq!(prop.type_name(), "Float");
    }

    #[test]
    fn test_boolean_creation() {
        let prop = PropertyValue::new_boolean(true);
        assert_eq!(prop.as_boolean(), Some(true));
        assert_eq!(prop.type_name(), "Boolean");
    }

    #[test]
    fn test_string_creation() {
        let prop = PropertyValue::new_string("hello");
        assert_eq!(prop.as_string(), Some("hello"));
        assert_eq!(prop.type_name(), "String");
    }

    #[test]
    fn test_null_creation() {
        let prop = PropertyValue::new_null();
        assert!(prop.is_null());
        assert_eq!(prop.type_name(), "Null");
    }

    #[test]
    fn test_type_safety() {
        let int_prop = PropertyValue::new_integer(42);
        assert_eq!(int_prop.as_float(), None);
        assert_eq!(int_prop.as_boolean(), None);
        assert_eq!(int_prop.as_string(), None);
        assert!(!int_prop.is_null());
    }

    #[test]
    fn test_with_timestamps() {
        let prop = PropertyValue::new_integer(42).with_timestamps(5000, 6000);
        assert_eq!(prop.as_integer(), Some(42));
        assert_eq!(prop.updated_at(), 5000);
        assert_eq!(prop.valid_from(), 6000);
    }

    #[test]
    fn test_display() {
        assert_eq!(PropertyValue::new_integer(42).to_string(), "42");
        assert_eq!(PropertyValue::new_float(3.14).to_string(), "3.14");
        assert_eq!(PropertyValue::new_boolean(true).to_string(), "true");
        assert_eq!(PropertyValue::new_string("test").to_string(), "test");
        assert_eq!(PropertyValue::new_null().to_string(), "null");
    }

    #[test]
    fn test_clone() {
        let prop1 = PropertyValue::new_string("hello");
        let prop2 = prop1.clone();
        assert_eq!(prop1, prop2);
    }

    #[test]
    fn test_partial_eq() {
        let prop1 = PropertyValue::new_integer_with_timestamps(42, 1000, 1000);
        let prop2 = PropertyValue::new_integer_with_timestamps(42, 1000, 1000);
        let prop3 = PropertyValue::new_integer_with_timestamps(43, 1000, 1000);

        assert_eq!(prop1, prop2);
        assert_ne!(prop1, prop3);
    }
}
