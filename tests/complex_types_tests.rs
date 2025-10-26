use manifold::Value;

#[test]
fn test_vec_encode_varint_len_small() {
    // Test encoding with len < 254
    let vec: Vec<u8> = vec![1, 2, 3];
    let encoded = <Vec<u8>>::as_bytes(&vec);

    // First byte should be the length (3)
    assert_eq!(encoded[0], 3);
    // Following bytes should be the elements
    assert_eq!(&encoded[1..], &[1, 2, 3]);
}

#[test]
fn test_vec_encode_varint_len_medium() {
    // Test encoding with 254 <= len <= u16::MAX
    let vec: Vec<u8> = vec![0; 300];
    let encoded = <Vec<u8>>::as_bytes(&vec);

    // First byte should be 254 (marker for u16 length)
    assert_eq!(encoded[0], 254);
    // Next 2 bytes should be the length as u16
    let len = u16::from_le_bytes([encoded[1], encoded[2]]);
    assert_eq!(len, 300);
}

#[test]
fn test_vec_encode_varint_len_large() {
    // Test encoding with len > u16::MAX
    let vec: Vec<u8> = vec![0; 70000];
    let encoded = <Vec<u8>>::as_bytes(&vec);

    // First byte should be 255 (marker for u32 length)
    assert_eq!(encoded[0], 255);
    // Next 4 bytes should be the length as u32
    let len = u32::from_le_bytes([encoded[1], encoded[2], encoded[3], encoded[4]]);
    assert_eq!(len, 70000);
}

#[test]
fn test_vec_decode_varint_len_small() {
    // Test decoding with len < 254
    let vec: Vec<u8> = vec![42; 100];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_decode_varint_len_medium() {
    // Test decoding with 254 <= len <= u16::MAX
    let vec: Vec<u8> = vec![7; 500];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_decode_varint_len_large() {
    // Test decoding with len > u16::MAX
    let vec: Vec<u8> = vec![99; 80000];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_of_strings() {
    // Test Vec<String> which has variable-width elements
    let vec: Vec<&str> = vec!["hello", "world", "foo", "bar"];
    let encoded = <Vec<&str>>::as_bytes(&vec);
    let decoded = <Vec<&str>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_of_fixed_width() {
    // Test Vec<u64> which has fixed-width elements
    let vec: Vec<u64> = vec![1, 2, 3, 4, 5, 100, 200, 300];
    let encoded = <Vec<u64>>::as_bytes(&vec);
    let decoded = <Vec<u64>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_empty() {
    // Test empty vector
    let vec: Vec<u8> = vec![];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_type_name() {
    let type_name = <Vec<u8>>::type_name();
    assert!(type_name.name().contains("Vec"));
    assert!(type_name.name().contains("u8"));
}

#[test]
fn test_vec_nested() {
    // Test Vec<Vec<u8>>
    let vec: Vec<Vec<u8>> = vec![vec![1, 2, 3], vec![4, 5], vec![6, 7, 8, 9]];
    let encoded = <Vec<Vec<u8>>>::as_bytes(&vec);
    let decoded = <Vec<Vec<u8>>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_fixed_width_is_none() {
    assert_eq!(<Vec<u8>>::fixed_width(), None);
    assert_eq!(<Vec<u64>>::fixed_width(), None);
    assert_eq!(<Vec<&str>>::fixed_width(), None);
}

#[test]
fn test_vec_roundtrip_boundary_254() {
    // Test the boundary at 254 elements
    let vec: Vec<u8> = vec![42; 254];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_roundtrip_boundary_255() {
    // Test just over the boundary at 255 elements
    let vec: Vec<u8> = vec![42; 255];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded, vec);
}

#[test]
fn test_vec_roundtrip_u16_max() {
    // Test at u16::MAX boundary
    let vec: Vec<u8> = vec![1; u16::MAX as usize];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded.len(), vec.len());
}

#[test]
fn test_vec_roundtrip_u16_max_plus_one() {
    // Test just over u16::MAX boundary
    let vec: Vec<u8> = vec![1; (u16::MAX as usize) + 1];
    let encoded = <Vec<u8>>::as_bytes(&vec);
    let decoded = <Vec<u8>>::from_bytes(&encoded);
    assert_eq!(decoded.len(), vec.len());
}
