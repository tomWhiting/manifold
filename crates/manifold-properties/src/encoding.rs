//! Manifold Value trait implementation for PropertyValue.
//!
//! This module implements the serialization and deserialization logic for PropertyValue,
//! enabling efficient storage in Manifold tables with zero-copy reads for fixed-width types.

use crate::property_value::PropertyValue;
use manifold::{TypeName, Value};

/// Zero-copy reference version of PropertyValue for efficient reads.
///
/// This enum mirrors PropertyValue but uses borrowed strings for the String variant,
/// enabling zero-copy deserialization from memory-mapped pages.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValueRef<'a> {
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
    /// String value (borrowed from underlying storage)
    String {
        value: &'a str,
        updated_at: u64,
        valid_from: u64,
    },
    /// Null value
    Null { updated_at: u64, valid_from: u64 },
}

impl<'a> PropertyValueRef<'a> {
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
    pub fn as_string(&self) -> Option<&'a str> {
        match self {
            Self::String { value, .. } => Some(value),
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

    /// Converts this reference to an owned PropertyValue.
    pub fn to_owned(&self) -> PropertyValue {
        match self {
            Self::Integer {
                value,
                updated_at,
                valid_from,
            } => PropertyValue::Integer {
                value: *value,
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::Float {
                value,
                updated_at,
                valid_from,
            } => PropertyValue::Float {
                value: *value,
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::Boolean {
                value,
                updated_at,
                valid_from,
            } => PropertyValue::Boolean {
                value: *value,
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::String {
                value,
                updated_at,
                valid_from,
            } => PropertyValue::String {
                value: value.to_string(),
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::Null {
                updated_at,
                valid_from,
            } => PropertyValue::Null {
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
        }
    }
}

/// Discriminant values for PropertyValue variants.
const DISCRIMINANT_INTEGER: u8 = 0;
const DISCRIMINANT_FLOAT: u8 = 1;
const DISCRIMINANT_BOOLEAN: u8 = 2;
const DISCRIMINANT_STRING: u8 = 3;
const DISCRIMINANT_NULL: u8 = 4;

impl Value for PropertyValue {
    type SelfType<'a> = PropertyValueRef<'a>;
    type AsBytes<'a> = Vec<u8>;

    fn fixed_width() -> Option<usize> {
        // Variable width due to String variant
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        if data.is_empty() {
            panic!("Cannot deserialize PropertyValue from empty data");
        }

        let discriminant = data[0];
        let payload = &data[1..];

        match discriminant {
            DISCRIMINANT_INTEGER => {
                if payload.len() < 24 {
                    panic!("Invalid Integer payload length");
                }
                let value = i64::from_le_bytes(payload[0..8].try_into().unwrap());
                let updated_at = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                let valid_from = u64::from_le_bytes(payload[16..24].try_into().unwrap());
                PropertyValueRef::Integer {
                    value,
                    updated_at,
                    valid_from,
                }
            }
            DISCRIMINANT_FLOAT => {
                if payload.len() < 24 {
                    panic!("Invalid Float payload length");
                }
                let value = f64::from_le_bytes(payload[0..8].try_into().unwrap());
                let updated_at = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                let valid_from = u64::from_le_bytes(payload[16..24].try_into().unwrap());
                PropertyValueRef::Float {
                    value,
                    updated_at,
                    valid_from,
                }
            }
            DISCRIMINANT_BOOLEAN => {
                if payload.len() < 17 {
                    panic!("Invalid Boolean payload length");
                }
                let value = payload[0] != 0;
                let updated_at = u64::from_le_bytes(payload[1..9].try_into().unwrap());
                let valid_from = u64::from_le_bytes(payload[9..17].try_into().unwrap());
                PropertyValueRef::Boolean {
                    value,
                    updated_at,
                    valid_from,
                }
            }
            DISCRIMINANT_STRING => {
                if payload.len() < 16 {
                    panic!("Invalid String payload length");
                }
                // String format: updated_at (8) + valid_from (8) + utf8 bytes
                let updated_at = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                let valid_from = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                let value =
                    std::str::from_utf8(&payload[16..]).expect("Invalid UTF-8 in String property");
                PropertyValueRef::String {
                    value,
                    updated_at,
                    valid_from,
                }
            }
            DISCRIMINANT_NULL => {
                if payload.len() < 16 {
                    panic!("Invalid Null payload length");
                }
                let updated_at = u64::from_le_bytes(payload[0..8].try_into().unwrap());
                let valid_from = u64::from_le_bytes(payload[8..16].try_into().unwrap());
                PropertyValueRef::Null {
                    updated_at,
                    valid_from,
                }
            }
            _ => panic!("Invalid PropertyValue discriminant: {}", discriminant),
        }
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        match value {
            PropertyValueRef::Integer {
                value: v,
                updated_at,
                valid_from,
            } => {
                let mut bytes = Vec::with_capacity(25);
                bytes.push(DISCRIMINANT_INTEGER);
                bytes.extend_from_slice(&v.to_le_bytes());
                bytes.extend_from_slice(&updated_at.to_le_bytes());
                bytes.extend_from_slice(&valid_from.to_le_bytes());
                bytes
            }
            PropertyValueRef::Float {
                value: v,
                updated_at,
                valid_from,
            } => {
                let mut bytes = Vec::with_capacity(25);
                bytes.push(DISCRIMINANT_FLOAT);
                bytes.extend_from_slice(&v.to_le_bytes());
                bytes.extend_from_slice(&updated_at.to_le_bytes());
                bytes.extend_from_slice(&valid_from.to_le_bytes());
                bytes
            }
            PropertyValueRef::Boolean {
                value: v,
                updated_at,
                valid_from,
            } => {
                let mut bytes = Vec::with_capacity(18);
                bytes.push(DISCRIMINANT_BOOLEAN);
                bytes.push(if *v { 1 } else { 0 });
                bytes.extend_from_slice(&updated_at.to_le_bytes());
                bytes.extend_from_slice(&valid_from.to_le_bytes());
                bytes
            }
            PropertyValueRef::String {
                value: v,
                updated_at,
                valid_from,
            } => {
                let str_bytes = v.as_bytes();
                let mut bytes = Vec::with_capacity(1 + 16 + str_bytes.len());
                bytes.push(DISCRIMINANT_STRING);
                bytes.extend_from_slice(&updated_at.to_le_bytes());
                bytes.extend_from_slice(&valid_from.to_le_bytes());
                bytes.extend_from_slice(str_bytes);
                bytes
            }
            PropertyValueRef::Null {
                updated_at,
                valid_from,
            } => {
                let mut bytes = Vec::with_capacity(17);
                bytes.push(DISCRIMINANT_NULL);
                bytes.extend_from_slice(&updated_at.to_le_bytes());
                bytes.extend_from_slice(&valid_from.to_le_bytes());
                bytes
            }
        }
    }

    fn type_name() -> TypeName {
        TypeName::new("manifold_properties::PropertyValue")
    }
}

// Helper to convert owned PropertyValue to PropertyValueRef for serialization
impl PropertyValue {
    /// Converts this owned value to a reference for serialization.
    pub(crate) fn as_ref(&self) -> PropertyValueRef<'_> {
        match self {
            Self::Integer {
                value,
                updated_at,
                valid_from,
            } => PropertyValueRef::Integer {
                value: *value,
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::Float {
                value,
                updated_at,
                valid_from,
            } => PropertyValueRef::Float {
                value: *value,
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::Boolean {
                value,
                updated_at,
                valid_from,
            } => PropertyValueRef::Boolean {
                value: *value,
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::String {
                value,
                updated_at,
                valid_from,
            } => PropertyValueRef::String {
                value: value.as_str(),
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
            Self::Null {
                updated_at,
                valid_from,
            } => PropertyValueRef::Null {
                updated_at: *updated_at,
                valid_from: *valid_from,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_roundtrip() {
        let original = PropertyValue::new_integer_with_timestamps(42, 1000, 2000);
        let ref_val = original.as_ref();
        let bytes = PropertyValue::as_bytes(&ref_val);
        let decoded = PropertyValue::from_bytes(&bytes);

        assert_eq!(decoded.as_integer(), Some(42));
        assert_eq!(decoded.updated_at(), 1000);
        assert_eq!(decoded.valid_from(), 2000);
    }

    #[test]
    fn test_float_roundtrip() {
        let original = PropertyValue::new_float_with_timestamps(3.14, 1000, 2000);
        let ref_val = original.as_ref();
        let bytes = PropertyValue::as_bytes(&ref_val);
        let decoded = PropertyValue::from_bytes(&bytes);

        assert_eq!(decoded.as_float(), Some(3.14));
        assert_eq!(decoded.updated_at(), 1000);
        assert_eq!(decoded.valid_from(), 2000);
    }

    #[test]
    fn test_boolean_roundtrip() {
        let original = PropertyValue::new_boolean_with_timestamps(true, 1000, 2000);
        let ref_val = original.as_ref();
        let bytes = PropertyValue::as_bytes(&ref_val);
        let decoded = PropertyValue::from_bytes(&bytes);

        assert_eq!(decoded.as_boolean(), Some(true));
        assert_eq!(decoded.updated_at(), 1000);
        assert_eq!(decoded.valid_from(), 2000);
    }

    #[test]
    fn test_string_roundtrip() {
        let original = PropertyValue::new_string_with_timestamps("hello world", 1000, 2000);
        let ref_val = original.as_ref();
        let bytes = PropertyValue::as_bytes(&ref_val);
        let decoded = PropertyValue::from_bytes(&bytes);

        assert_eq!(decoded.as_string(), Some("hello world"));
        assert_eq!(decoded.updated_at(), 1000);
        assert_eq!(decoded.valid_from(), 2000);
    }

    #[test]
    fn test_null_roundtrip() {
        let original = PropertyValue::new_null_with_timestamps(1000, 2000);
        let ref_val = original.as_ref();
        let bytes = PropertyValue::as_bytes(&ref_val);
        let decoded = PropertyValue::from_bytes(&bytes);

        assert!(decoded.is_null());
        assert_eq!(decoded.updated_at(), 1000);
        assert_eq!(decoded.valid_from(), 2000);
    }

    #[test]
    fn test_to_owned() {
        let bytes = PropertyValue::as_bytes(&PropertyValueRef::Integer {
            value: 42,
            updated_at: 1000,
            valid_from: 2000,
        });
        let ref_val = PropertyValue::from_bytes(&bytes);
        let owned = ref_val.to_owned();

        assert_eq!(owned.as_integer(), Some(42));
        assert_eq!(owned.updated_at(), 1000);
        assert_eq!(owned.valid_from(), 2000);
    }

    #[test]
    fn test_encoding_size_integer() {
        let val = PropertyValue::new_integer_with_timestamps(42, 1000, 2000);
        let bytes = PropertyValue::as_bytes(&val.as_ref());
        // 1 (discriminant) + 8 (i64) + 8 (updated_at) + 8 (valid_from) = 25 bytes
        assert_eq!(bytes.len(), 25);
    }

    #[test]
    fn test_encoding_size_boolean() {
        let val = PropertyValue::new_boolean_with_timestamps(true, 1000, 2000);
        let bytes = PropertyValue::as_bytes(&val.as_ref());
        // 1 (discriminant) + 1 (bool) + 8 (updated_at) + 8 (valid_from) = 18 bytes
        assert_eq!(bytes.len(), 18);
    }

    #[test]
    fn test_encoding_size_null() {
        let val = PropertyValue::new_null_with_timestamps(1000, 2000);
        let bytes = PropertyValue::as_bytes(&val.as_ref());
        // 1 (discriminant) + 8 (updated_at) + 8 (valid_from) = 17 bytes
        assert_eq!(bytes.len(), 17);
    }
}
