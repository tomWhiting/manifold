//! Integration traits for external vector index libraries.

use crate::dense::{VectorGuard, VectorIter};
use manifold::StorageError;

/// Trait for vector sources consumable by index builders
///
/// This trait enables external indexing libraries (HNSW, FAISS, etc.)
/// to efficiently iterate over vectors with zero-copy access.
pub trait VectorSource<const DIM: usize> {
    /// Iterator type over vectors with zero-copy access
    type Iter<'a>: Iterator<Item = Result<(String, VectorGuard<'a, DIM>), StorageError>> where Self: 'a;
    
    /// Returns an iterator over all vectors
    ///
    /// The iterator provides zero-copy access to vector data through guards.
    fn iter(&self) -> Result<Self::Iter<'_>, StorageError>;
    
    /// Returns the number of vectors
    fn len(&self) -> Result<u64, StorageError>;
    
    /// Returns true if empty
    fn is_empty(&self) -> Result<bool, StorageError> {
        Ok(self.len()? == 0)
    }
}

impl<const DIM: usize> VectorSource<DIM> for crate::dense::VectorTableRead<DIM> {
    type Iter<'a> = VectorIter<'a, DIM> where Self: 'a;
    
    fn iter(&self) -> Result<Self::Iter<'_>, StorageError> {
        self.iter()
    }
    
    fn len(&self) -> Result<u64, StorageError> {
        self.len()
    }
}
