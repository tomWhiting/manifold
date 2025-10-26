use manifold::{Key, Value};
use std::cmp::Ordering;

#[test]
fn test_tuple_type_name() {
    // Test type names for various tuple types
    let name = <(u8, u16)>::type_name();
    assert!(name.name().contains("u8"));
    assert!(name.name().contains("u16"));

    let name = <(&str, u64)>::type_name();
    assert!(name.name().contains("str"));
    assert!(name.name().contains("u64"));
}

#[test]
fn test_tuple_fixed_width_all_fixed() {
    // All fixed-width types should result in fixed width
    assert!(<(u8, u16)>::fixed_width().is_some());
    assert_eq!(<(u8, u16)>::fixed_width().unwrap(), 1 + 2);

    assert!(<(u32, u64)>::fixed_width().is_some());
    assert_eq!(<(u32, u64)>::fixed_width().unwrap(), 4 + 8);

    assert!(<(u8, u16, u32)>::fixed_width().is_some());
    assert_eq!(<(u8, u16, u32)>::fixed_width().unwrap(), 1 + 2 + 4);
}

#[test]
fn test_tuple_variable_width_mixed() {
    // Mixed fixed and variable width should be variable
    assert!(<(&str, u8)>::fixed_width().is_none());
    assert!(<(u16, &str)>::fixed_width().is_none());
    assert!(<(u8, &str, u64)>::fixed_width().is_none());
}

#[test]
fn test_tuple_roundtrip_fixed_width() {
    // Test roundtrip for fixed-width tuples
    let tuple = (42u8, 1000u16);
    let encoded = <(u8, u16)>::as_bytes(&tuple);
    let decoded = <(u8, u16)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = (100u32, 999999u64);
    let encoded = <(u32, u64)>::as_bytes(&tuple);
    let decoded = <(u32, u64)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_roundtrip_variable_width() {
    // Test roundtrip for variable-width tuples
    let tuple = ("hello", 42u8);
    let encoded = <(&str, u8)>::as_bytes(&tuple);
    let decoded = <(&str, u8)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = (100u16, "world");
    let encoded = <(u16, &str)>::as_bytes(&tuple);
    let decoded = <(u16, &str)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_roundtrip_mixed_variable() {
    // Test tuples with multiple variable-width fields
    let tuple = ("foo", "bar");
    let encoded = <(&str, &str)>::as_bytes(&tuple);
    let decoded = <(&str, &str)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = ("hello", 42u8, "world");
    let encoded = <(&str, u8, &str)>::as_bytes(&tuple);
    let decoded = <(&str, u8, &str)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_compare_fixed_width() {
    // Test comparison for fixed-width tuples
    let t1 = (1u8, 2u16);
    let t2 = (1u8, 3u16);
    let t3 = (2u8, 1u16);

    let e1 = <(u8, u16)>::as_bytes(&t1);
    let e2 = <(u8, u16)>::as_bytes(&t2);
    let e3 = <(u8, u16)>::as_bytes(&t3);

    assert_eq!(<(u8, u16)>::compare(&e1, &e2), Ordering::Less);
    assert_eq!(<(u8, u16)>::compare(&e2, &e1), Ordering::Greater);
    assert_eq!(<(u8, u16)>::compare(&e1, &e1), Ordering::Equal);
    assert_eq!(<(u8, u16)>::compare(&e1, &e3), Ordering::Less);
}

#[test]
fn test_tuple_compare_variable_width() {
    // Test comparison for variable-width tuples
    let t1 = ("abc", 1u8);
    let t2 = ("abc", 2u8);
    let t3 = ("def", 1u8);

    let e1 = <(&str, u8)>::as_bytes(&t1);
    let e2 = <(&str, u8)>::as_bytes(&t2);
    let e3 = <(&str, u8)>::as_bytes(&t3);

    assert_eq!(<(&str, u8)>::compare(&e1, &e2), Ordering::Less);
    assert_eq!(<(&str, u8)>::compare(&e2, &e1), Ordering::Greater);
    assert_eq!(<(&str, u8)>::compare(&e1, &e1), Ordering::Equal);
    assert_eq!(<(&str, u8)>::compare(&e1, &e3), Ordering::Less);
}

#[test]
fn test_tuple_large_arity() {
    // Test tuples with many elements
    let tuple = (1u8, 2u8, 3u8, 4u8, 5u8);
    let encoded = <(u8, u8, u8, u8, u8)>::as_bytes(&tuple);
    let decoded = <(u8, u8, u8, u8, u8)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = ("a", "b", "c", "d", "e");
    let encoded = <(&str, &str, &str, &str, &str)>::as_bytes(&tuple);
    let decoded = <(&str, &str, &str, &str, &str)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_varint_encoding_small_strings() {
    // Test that small strings use 1-byte varint
    let tuple = ("hi", 42u8);
    let encoded = <(&str, u8)>::as_bytes(&tuple);

    // Should be: 1 byte (varint for "hi" length) + 2 bytes ("hi") + 1 byte (u8)
    assert_eq!(encoded.len(), 1 + 2 + 1);
}

#[test]
fn test_tuple_varint_encoding_medium_strings() {
    // Test that medium strings (254+ bytes) use 3-byte varint
    let long_str = "a".repeat(300);
    let tuple = (long_str.as_str(), 42u8);
    let encoded = <(&str, u8)>::as_bytes(&tuple);

    // Should be: 3 bytes (varint for length) + 300 bytes (string) + 1 byte (u8)
    assert_eq!(encoded.len(), 3 + 300 + 1);
}

#[test]
fn test_tuple_mixed_fixed_variable_compare() {
    // Test comparison with mixed fixed/variable width
    let t1 = (1u8, "aaa");
    let t2 = (1u8, "bbb");
    let t3 = (2u8, "aaa");

    let e1 = <(u8, &str)>::as_bytes(&t1);
    let e2 = <(u8, &str)>::as_bytes(&t2);
    let e3 = <(u8, &str)>::as_bytes(&t3);

    assert_eq!(<(u8, &str)>::compare(&e1, &e2), Ordering::Less);
    assert_eq!(<(u8, &str)>::compare(&e1, &e3), Ordering::Less);
    assert_eq!(<(u8, &str)>::compare(&e3, &e1), Ordering::Greater);
}

#[test]
fn test_tuple_empty_strings() {
    // Test tuples with empty strings
    let tuple = ("", 42u8);
    let encoded = <(&str, u8)>::as_bytes(&tuple);
    let decoded = <(&str, u8)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);

    let tuple = ("", "");
    let encoded = <(&str, &str)>::as_bytes(&tuple);
    let decoded = <(&str, &str)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_single_element() {
    // Test single-element tuples
    let tuple = (42u8,);
    let encoded = <(u8,)>::as_bytes(&tuple);
    let decoded = <(u8,)>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, tuple);

    let tuple = ("hello",);
    let encoded = <(&str,)>::as_bytes(&tuple);
    let decoded = <(&str,)>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_single_element_type_name() {
    let name = <(u8,)>::type_name();
    assert!(name.name().contains("u8"));
    assert!(name.name().contains(","));
}

#[test]
fn test_tuple_multiple_variable_in_middle() {
    // Test with variable-width types not at the end
    let tuple = ("first", 100u32, "second", 200u64);
    let encoded = <(&str, u32, &str, u64)>::as_bytes(&tuple);
    let decoded = <(&str, u32, &str, u64)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
}

#[test]
fn test_tuple_all_same_fixed_type() {
    // Test tuples with all the same fixed-width type
    let tuple = (1u64, 2u64, 3u64);
    let encoded = <(u64, u64, u64)>::as_bytes(&tuple);
    let decoded = <(u64, u64, u64)>::from_bytes(&encoded);
    assert_eq!(decoded, tuple);
    assert_eq!(<(u64, u64, u64)>::fixed_width().unwrap(), 24);
}

#[test]
fn test_tuple_compare_early_difference() {
    // Test comparison when difference is in first element
    let t1 = (1u8, 100u16, 1000u32);
    let t2 = (2u8, 50u16, 500u32);

    let e1 = <(u8, u16, u32)>::as_bytes(&t1);
    let e2 = <(u8, u16, u32)>::as_bytes(&t2);

    assert_eq!(<(u8, u16, u32)>::compare(&e1, &e2), Ordering::Less);
}

#[test]
fn test_tuple_compare_late_difference() {
    // Test comparison when difference is in last element
    let t1 = (1u8, 100u16, 1000u32);
    let t2 = (1u8, 100u16, 2000u32);

    let e1 = <(u8, u16, u32)>::as_bytes(&t1);
    let e2 = <(u8, u16, u32)>::as_bytes(&t2);

    assert_eq!(<(u8, u16, u32)>::compare(&e1, &e2), Ordering::Less);
}

#[test]
fn test_tuple_last_field_length_elided() {
    // Verify that the last field's length is not encoded for variable-width tuples
    let tuple = (42u8, "hello");
    let encoded = <(u8, &str)>::as_bytes(&tuple);

    // Should be: 1 byte (u8) + 5 bytes ("hello")
    // No length prefix for the last field
    assert_eq!(encoded.len(), 1 + 5);
}
