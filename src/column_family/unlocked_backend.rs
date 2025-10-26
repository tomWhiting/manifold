use crate::{DatabaseError, StorageBackend};
use std::fs::File;
use std::io;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
use std::os::windows::fs::FileExt;

/// A file backend that does not acquire file locks.
///
/// This is used for per-column-family file handles within the file handle pool.
/// Since all handles are in the same process and coordinated by the pool,
/// file locking is unnecessary and would prevent opening multiple handles.
///
/// The main database header backend still uses a locked `FileBackend` to prevent
/// other processes from opening the same database file.
#[derive(Debug)]
pub(crate) struct UnlockedFileBackend {
    file: File,
}

impl UnlockedFileBackend {
    /// Creates a new unlocked file backend.
    ///
    /// Does not acquire any file locks. The caller must ensure proper
    /// coordination between file handles.
    pub fn new(file: File) -> Result<Self, DatabaseError> {
        Ok(Self { file })
    }
}

impl StorageBackend for UnlockedFileBackend {
    fn len(&self) -> Result<u64, io::Error> {
        Ok(self.file.metadata()?.len())
    }

    #[cfg(unix)]
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), io::Error> {
        self.file.read_exact_at(out, offset)?;
        Ok(())
    }

    #[cfg(target_os = "wasi")]
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<(), io::Error> {
        read_exact_at(&self.file, out, offset)?;
        Ok(())
    }

    #[cfg(windows)]
    fn read(&self, mut offset: u64, out: &mut [u8]) -> Result<(), io::Error> {
        let mut data_offset = 0;
        while data_offset < out.len() {
            let read = self.file.seek_read(&mut out[data_offset..], offset)?;
            offset += read as u64;
            data_offset += read;
        }
        Ok(())
    }

    fn set_len(&self, len: u64) -> Result<(), io::Error> {
        self.file.set_len(len)
    }

    fn sync_data(&self) -> Result<(), io::Error> {
        self.file.sync_data()
    }

    #[cfg(unix)]
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), io::Error> {
        self.file.write_all_at(data, offset)
    }

    #[cfg(target_os = "wasi")]
    fn write(&self, offset: u64, data: &[u8]) -> Result<(), io::Error> {
        write_all_at(&self.file, data, offset)
    }

    #[cfg(windows)]
    fn write(&self, mut offset: u64, data: &[u8]) -> Result<(), io::Error> {
        let mut data_offset = 0;
        while data_offset < data.len() {
            let written = self.file.seek_write(&data[data_offset..], offset)?;
            offset += written as u64;
            data_offset += written;
        }
        Ok(())
    }

    fn close(&self) -> Result<(), io::Error> {
        Ok(())
    }
}

#[cfg(target_os = "wasi")]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    while !buf.is_empty() {
        let nbytes = unsafe {
            libc::pread(
                file.as_raw_fd(),
                buf.as_mut_ptr() as _,
                std::cmp::min(buf.len(), libc::ssize_t::MAX as _),
                offset as _,
            )
        };
        match nbytes {
            0 => break,
            -1 => match io::Error::last_os_error() {
                err if err.kind() == io::ErrorKind::Interrupted => {}
                err => return Err(err),
            },
            n => {
                let tmp = buf;
                buf = &mut tmp[n as usize..];
                offset += n as u64;
            }
        }
    }
    if !buf.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "failed to fill whole buffer",
        ))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "wasi")]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    while !buf.is_empty() {
        let nbytes = unsafe {
            libc::pwrite(
                file.as_raw_fd(),
                buf.as_ptr() as _,
                std::cmp::min(buf.len(), libc::ssize_t::MAX as _),
                offset as _,
            )
        };
        match nbytes {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ));
            }
            -1 => match io::Error::last_os_error() {
                err if err.kind() == io::ErrorKind::Interrupted => {}
                err => return Err(err),
            },
            n => {
                buf = &buf[n as usize..];
                offset += n as u64;
            }
        }
    }
    Ok(())
}
