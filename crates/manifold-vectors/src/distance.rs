//! Common distance and similarity functions for vectors.
//!
//! All functions work directly with slices, making them compatible with
//! zero-copy VectorGuard types through deref coercion.

/// Computes the cosine similarity between two vectors
///
/// Works with any slice type, including zero-copy VectorGuard.
#[inline]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let mag = (norm_a * norm_b).sqrt();
    if mag == 0.0 { 0.0 } else { dot / mag }
}

/// Computes the Euclidean (L2) distance between two vectors
///
/// Works with any slice type, including zero-copy VectorGuard.
#[inline]
pub fn euclidean(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let mut sum = 0.0;
    for i in 0..a.len() {
        let diff = a[i] - b[i];
        sum += diff * diff;
    }
    sum.sqrt()
}

/// Computes the squared Euclidean distance between two vectors
///
/// Faster than euclidean() as it avoids the sqrt operation.
/// Works with any slice type, including zero-copy VectorGuard.
#[inline]
pub fn euclidean_squared(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let mut sum = 0.0;
    for i in 0..a.len() {
        let diff = a[i] - b[i];
        sum += diff * diff;
    }
    sum
}

/// Computes the dot product of two vectors
///
/// Works with any slice type, including zero-copy VectorGuard.
#[inline]
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let mut sum = 0.0;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

/// Computes the Manhattan (L1) distance between two vectors
///
/// Works with any slice type, including zero-copy VectorGuard.
#[inline]
pub fn manhattan(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let mut sum = 0.0;
    for i in 0..a.len() {
        sum += (a[i] - b[i]).abs();
    }
    sum
}
