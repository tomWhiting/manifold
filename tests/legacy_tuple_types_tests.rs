use manifold::{Key, Legacy, Value};
use std::cmp::Ordering;

#[test]
fn test_legacy_tuple_type_name() {
    // Test type names for legacy tuple types
    let name = <Legacy<(u8, u16)>>::type_name();
    assert!(name.name().contains("u8"));
    assert!(name.name().contains("u16"));

    let name = <Legacy<(&str, u64)>>::type_name();
    assert!(name.name().contains("str"));
    assert!(name.name().contains("u64"));
}

#[test]
fn test_legacy_tuple_fixed_width_all_fixed() {
    // All fixed-width types should result in fixed width
    assert!(<Legacy<(u8, u16)>>::fixed_width().is_some());
    assert_eq!(<Legacy<(u8, u16)>>::fixed_width().unwrap(), 1 + 2);

    assert!(<Legacy<(u32, u64)>>::fixed_width().is_some());
    assert_eq!(<Legacy<(u32, u64)>>::fixed_width().unwrap(), 4 + 8);

    assert!(<Legacy<(u8, u16, u32)>>::fixed_width().is_some());
    assert_eq!(<Legacy<(u8, u16, u32)>>::fixed_width().unwrap(), 1 + 2 + 4);
}

#[test]
fn test_legacy_tuple_variable_width_mixed() {
    // Mixed fixed and variable width should be variable
    assert!(<Legacy<(&str, u8)>>::fixed_width().is_none());
    assert!(<Legacy<(u16, &str)>>::fixed_width().is_none());
    assert!(<Legacy<(u8, &str, u64)>>::fixed_width().is_none());
}

#[test]
fn test_legacy_tuple_roundtrip_fixed_width() {
    // Test roundtrip for fixed-width tuples
    let tuple = (42u8, 1000u16);
    let encoded = <Legacy<(u8, u16)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u8, u16)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = (100u32, 999999u64);
    let encoded = <Legacy<(u32, u64)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u32, u64)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_legacy_tuple_roundtrip_variable_width() {
    // Test roundtrip for variable-width tuples using old u32 encoding
    let tuple = ("hello", 42u8);
    let encoded = <Legacy<(&str, u8)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, u8)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = (100u16, "world");
    let encoded = <Legacy<(u16, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u16, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_legacy_tuple_roundtrip_mixed_variable() {
    // Test tuples with multiple variable-width fields
    let tuple = ("foo", "bar");
    let encoded = <Legacy<(&str, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = ("hello", 42u8, "world");
    let encoded = <Legacy<(&str, u8, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, u8, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_legacy_tuple_compare_fixed_width() {
    // Test comparison for fixed-width tuples
    let t1 = (1u8, 2u16);
    let t2 = (1u8, 3u16);
    let t3 = (2u8, 1u16);

    let e1 = <Legacy<(u8, u16)>>::as_bytes(&t1);
    let e2 = <Legacy<(u8, u16)>>::as_bytes(&t2);
    let e3 = <Legacy<(u8, u16)>>::as_bytes(&t3);

    assert_eq!(<Legacy<(u8, u16)>>::compare(&e1, &e2), Ordering::Less);
    assert_eq!(<Legacy<(u8, u16)>>::compare(&e2, &e1), Ordering::Greater);
    assert_eq!(<Legacy<(u8, u16)>>::compare(&e1, &e1), Ordering::Equal);
    assert_eq!(<Legacy<(u8, u16)>>::compare(&e1, &e3), Ordering::Less);
}

#[test]
fn test_legacy_tuple_compare_variable_width() {
    // Test comparison for variable-width tuples
    let t1 = ("abc", 1u8);
    let t2 = ("abc", 2u8);
    let t3 = ("def", 1u8);

    let e1 = <Legacy<(&str, u8)>>::as_bytes(&t1);
    let e2 = <Legacy<(&str, u8)>>::as_bytes(&t2);
    let e3 = <Legacy<(&str, u8)>>::as_bytes(&t3);

    assert_eq!(<Legacy<(&str, u8)>>::compare(&e1, &e2), Ordering::Less);
    assert_eq!(<Legacy<(&str, u8)>>::compare(&e2, &e1), Ordering::Greater);
    assert_eq!(<Legacy<(&str, u8)>>::compare(&e1, &e1), Ordering::Equal);
    assert_eq!(<Legacy<(&str, u8)>>::compare(&e1, &e3), Ordering::Less);
}

#[test]
fn test_legacy_tuple_large_arity() {
    // Test tuples with many elements
    let tuple = (1u8, 2u8, 3u8, 4u8, 5u8);
    let encoded = <Legacy<(u8, u8, u8, u8, u8)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u8, u8, u8, u8, u8)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = ("a", "b", "c", "d", "e");
    let encoded = <Legacy<(&str, &str, &str, &str, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, &str, &str, &str, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_legacy_tuple_u32_length_encoding() {
    // Legacy encoding uses u32 for all variable-length fields (except last)
    // Verify the encoding uses 4 bytes per length
    let tuple = ("hi", 42u8);
    let encoded = <Legacy<(&str, u8)>>::as_bytes(&tuple);

    // Should be: 4 bytes (u32 length) + 2 bytes ("hi") + 1 byte (u8)
    assert_eq!(encoded.len(), 4 + 2 + 1);
}

#[test]
fn test_legacy_tuple_single_element() {
    // Test single-element tuples
    let tuple = (42u8,);
    let encoded = <Legacy<(u8,)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u8,)>>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, tuple);

    let tuple = ("hello",);
    let encoded = <Legacy<(&str,)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str,)>>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, tuple);
}

#[test]
fn test_legacy_tuple_single_element_type_name() {
    let name = <Legacy<(u8,)>>::type_name();
    assert!(name.name().contains("u8"));
    assert!(name.name().contains(","));
}

#[test]
fn test_legacy_tuple_empty_strings() {
    // Test tuples with empty strings
    let tuple = ("", 42u8);
    let encoded = <Legacy<(&str, u8)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, u8)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = ("", "");
    let encoded = <Legacy<(&str, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_legacy_vs_new_tuple_encoding_difference() {
    // Demonstrate that legacy and new encodings differ for variable-width tuples
    let tuple = ("hello", 42u8);

    let legacy_encoded = <Legacy<(&str, u8)>>::as_bytes(&tuple);
    let new_encoded = <(&str, u8)>::as_bytes(&tuple);

    // Legacy uses u32 (4 bytes) for length, new uses varint (1 byte for small strings)
    assert_ne!(legacy_encoded.len(), new_encoded.len());
    assert_eq!(legacy_encoded.len(), 4 + 5 + 1); // u32 + "hello" + u8
    assert_eq!(new_encoded.len(), 1 + 5 + 1); // varint + "hello" + u8
}

#[test]
fn test_legacy_tuple_multiple_variable_fields() {
    // Test with multiple variable-width fields
    let tuple = ("first", "second", "third");
    let encoded = <Legacy<(&str, &str, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(&str, &str, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    // Should have two u32 length prefixes (not for the last field)
    // 4 + 5 ("first") + 4 + 6 ("second") + 5 ("third")
    assert_eq!(encoded.len(), 4 + 5 + 4 + 6 + 5);
}

#[test]
fn test_legacy_tuple_compare_early_difference() {
    // Test comparison when difference is in first element
    let t1 = (1u8, 100u16, 1000u32);
    let t2 = (2u8, 50u16, 500u32);

    let e1 = <Legacy<(u8, u16, u32)>>::as_bytes(&t1);
    let e2 = <Legacy<(u8, u16, u32)>>::as_bytes(&t2);

    assert_eq!(<Legacy<(u8, u16, u32)>>::compare(&e1, &e2), Ordering::Less);
}

#[test]
fn test_legacy_tuple_compare_late_difference() {
    // Test comparison when difference is in last element
    let t1 = (1u8, 100u16, 1000u32);
    let t2 = (1u8, 100u16, 2000u32);

    let e1 = <Legacy<(u8, u16, u32)>>::as_bytes(&t1);
    let e2 = <Legacy<(u8, u16, u32)>>::as_bytes(&t2);

    assert_eq!(<Legacy<(u8, u16, u32)>>::compare(&e1, &e2), Ordering::Less);
}

#[test]
fn test_legacy_tuple_all_same_fixed_type() {
    // Test tuples with all the same fixed-width type
    let tuple = (1u64, 2u64, 3u64);
    let encoded = <Legacy<(u64, u64, u64)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u64, u64, u64)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
    assert_eq!(<Legacy<(u64, u64, u64)>>::fixed_width().unwrap(), 24);
}

#[test]
fn test_legacy_tuple_mixed_fixed_variable() {
    // Test with alternating fixed and variable width types
    let tuple = (1u32, "hello", 2u64, "world");
    let encoded = <Legacy<(u32, &str, u64, &str)>>::as_bytes(&tuple);
    let decoded = <Legacy<(u32, &str, u64, &str)>>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}
