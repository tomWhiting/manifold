use manifold::{TypeName, Value};

#[test]
fn test_type_name_new() {
    let name = TypeName::new("my_custom_type");
    assert_eq!(name.name(), "my_custom_type");
}

#[test]
fn test_type_name_equality() {
    let name1 = TypeName::new("same_name");
    let name2 = TypeName::new("same_name");
    assert_eq!(name1, name2);

    let name3 = TypeName::new("different_name");
    assert_ne!(name1, name3);
}

#[test]
fn test_type_name_clone() {
    let name1 = TypeName::new("cloneable");
    let name2 = name1.clone();
    assert_eq!(name1, name2);
    assert_eq!(name1.name(), name2.name());
}

#[test]
fn test_type_name_debug() {
    let name = TypeName::new("debug_test");
    let debug_str = format!("{:?}", name);
    assert!(debug_str.contains("debug_test"));
}

#[test]
fn test_primitive_type_names() {
    assert_eq!(<u8>::type_name().name(), "u8");
    assert_eq!(<u16>::type_name().name(), "u16");
    assert_eq!(<u32>::type_name().name(), "u32");
    assert_eq!(<u64>::type_name().name(), "u64");
    assert_eq!(<u128>::type_name().name(), "u128");
    assert_eq!(<i8>::type_name().name(), "i8");
    assert_eq!(<i16>::type_name().name(), "i16");
    assert_eq!(<i32>::type_name().name(), "i32");
    assert_eq!(<i64>::type_name().name(), "i64");
    assert_eq!(<i128>::type_name().name(), "i128");
}

#[test]
fn test_bool_type_name() {
    assert_eq!(<bool>::type_name().name(), "bool");
}

#[test]
fn test_str_type_name() {
    let name = <&str>::type_name();
    assert_eq!(name.name(), "&str");
}

#[test]
fn test_string_type_name() {
    let name = <String>::type_name();
    assert_eq!(name.name(), "String");
}

#[test]
fn test_bytes_type_name() {
    let name = <&[u8]>::type_name();
    assert_eq!(name.name(), "&[u8]");
}

#[test]
fn test_vec_type_name() {
    let name = <Vec<u8>>::type_name();
    assert!(name.name().contains("Vec"));
    assert!(name.name().contains("u8"));
}

#[test]
fn test_option_type_name() {
    let name = <Option<u64>>::type_name();
    assert!(name.name().contains("Option"));
    assert!(name.name().contains("u64"));
}

#[test]
fn test_fixed_width_primitives() {
    assert_eq!(<u8>::fixed_width(), Some(1));
    assert_eq!(<u16>::fixed_width(), Some(2));
    assert_eq!(<u32>::fixed_width(), Some(4));
    assert_eq!(<u64>::fixed_width(), Some(8));
    assert_eq!(<u128>::fixed_width(), Some(16));
    assert_eq!(<i8>::fixed_width(), Some(1));
    assert_eq!(<i16>::fixed_width(), Some(2));
    assert_eq!(<i32>::fixed_width(), Some(4));
    assert_eq!(<i64>::fixed_width(), Some(8));
    assert_eq!(<i128>::fixed_width(), Some(16));
}

#[test]
fn test_bool_fixed_width() {
    assert_eq!(<bool>::fixed_width(), Some(1));
}

#[test]
fn test_variable_width_types() {
    assert_eq!(<&str>::fixed_width(), None);
    assert_eq!(<String>::fixed_width(), None);
    assert_eq!(<&[u8]>::fixed_width(), None);
    assert_eq!(<Vec<u8>>::fixed_width(), None);
}

#[test]
fn test_u8_roundtrip() {
    let value: u8 = 42;
    let encoded = <u8>::as_bytes(&value);
    let decoded = <u8>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_u64_roundtrip() {
    let value: u64 = 0x0123456789ABCDEF;
    let encoded = <u64>::as_bytes(&value);
    let decoded = <u64>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_i64_roundtrip() {
    let value: i64 = -12345678901234567;
    let encoded = <i64>::as_bytes(&value);
    let decoded = <i64>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_bool_roundtrip() {
    let value_true = true;
    let encoded_true = <bool>::as_bytes(&value_true);
    let decoded_true = <bool>::from_bytes(encoded_true.as_ref());
    assert_eq!(decoded_true, value_true);

    let value_false = false;
    let encoded_false = <bool>::as_bytes(&value_false);
    let decoded_false = <bool>::from_bytes(encoded_false.as_ref());
    assert_eq!(decoded_false, value_false);
}

#[test]
fn test_str_roundtrip() {
    let value = "hello world";
    let encoded = <&str>::as_bytes(&value);
    let decoded = <&str>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_string_roundtrip() {
    let value = String::from("test string");
    let encoded = <String>::as_bytes(&value);
    let decoded = <String>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_bytes_roundtrip() {
    let value: &[u8] = &[1, 2, 3, 4, 5];
    let encoded = <&[u8]>::as_bytes(&value);
    let decoded = <&[u8]>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_option_some_roundtrip() {
    let value: Option<u64> = Some(12345);
    let encoded = <Option<u64>>::as_bytes(&value);
    let decoded = <Option<u64>>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_option_none_roundtrip() {
    let value: Option<u64> = None;
    let encoded = <Option<u64>>::as_bytes(&value);
    let decoded = <Option<u64>>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_u128_max_value() {
    let value = u128::MAX;
    let encoded = <u128>::as_bytes(&value);
    let decoded = <u128>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_i128_min_value() {
    let value = i128::MIN;
    let encoded = <i128>::as_bytes(&value);
    let decoded = <i128>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_i128_max_value() {
    let value = i128::MAX;
    let encoded = <i128>::as_bytes(&value);
    let decoded = <i128>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_empty_str() {
    let value = "";
    let encoded = <&str>::as_bytes(&value);
    let decoded = <&str>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_empty_bytes() {
    let value: &[u8] = &[];
    let encoded = <&[u8]>::as_bytes(&value);
    let decoded = <&[u8]>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_unicode_str() {
    let value = "Hello 世界 🌍";
    let encoded = <&str>::as_bytes(&value);
    let decoded = <&str>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value);
}

#[test]
fn test_large_bytes() {
    let value: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    let value_slice = value.as_slice();
    let encoded = <&[u8]>::as_bytes(&value_slice);
    let decoded = <&[u8]>::from_bytes(encoded.as_ref());
    assert_eq!(decoded, value_slice);
}

#[test]
fn test_zero_values() {
    assert_eq!(<u8>::from_bytes(&<u8>::as_bytes(&0)), 0);
    assert_eq!(<u16>::from_bytes(&<u16>::as_bytes(&0)), 0);
    assert_eq!(<u32>::from_bytes(&<u32>::as_bytes(&0)), 0);
    assert_eq!(<u64>::from_bytes(&<u64>::as_bytes(&0)), 0);
    assert_eq!(<u128>::from_bytes(&<u128>::as_bytes(&0)), 0);
    assert_eq!(<i8>::from_bytes(&<i8>::as_bytes(&0)), 0);
    assert_eq!(<i16>::from_bytes(&<i16>::as_bytes(&0)), 0);
    assert_eq!(<i32>::from_bytes(&<i32>::as_bytes(&0)), 0);
    assert_eq!(<i64>::from_bytes(&<i64>::as_bytes(&0)), 0);
    assert_eq!(<i128>::from_bytes(&<i128>::as_bytes(&0)), 0);
}

#[test]
fn test_option_fixed_width() {
    // Option<T> where T is fixed width should be fixed width
    assert_eq!(<Option<u64>>::fixed_width(), Some(9)); // 1 byte discriminant + 8 bytes value
    assert_eq!(<Option<u8>>::fixed_width(), Some(2)); // 1 byte discriminant + 1 byte value
}

#[test]
fn test_option_variable_width() {
    // Option<T> where T is variable width should be variable width
    assert_eq!(<Option<&str>>::fixed_width(), None);
    assert_eq!(<Option<String>>::fixed_width(), None);
}
