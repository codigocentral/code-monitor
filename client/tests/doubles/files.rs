//! Fault-injectable writer providers, restricted to owned test paths.
use std::{
    cell::Cell,
    io::{self, Write},
    path::Path,
};
#[derive(Clone, Copy, PartialEq)]
pub enum Fault {
    None,
    CsvHeader,
    CsvRow,
    CsvFlush,
    JsonWrite,
}
thread_local! {static FAULT:Cell<Fault>=const {Cell::new(Fault::None)};}
pub struct Guard;
impl Guard {
    pub fn arm(value: Fault) -> Self {
        FAULT.with(|f| f.set(value));
        Self
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        FAULT.with(|f| f.set(Fault::None));
    }
}
fn fails(value: Fault) -> bool {
    FAULT.with(|f| f.get() == value)
}
fn error() -> io::Error {
    io::Error::other("unit output provider failed")
}
fn owned(path: &Path) -> io::Result<()> {
    let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("owned temporary root"));
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unit writer requires an owned temporary path",
        ))
    }
}
pub struct File(std::fs::File);
impl File {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        owned(path.as_ref())?;
        std::fs::File::create(path).map(Self)
    }
}
impl Write for File {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if fails(Fault::JsonWrite) {
            Err(error())
        } else {
            self.0.write(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
pub mod csv {
    use super::*;
    pub struct Writer {
        inner: ::csv::Writer<std::fs::File>,
        records: usize,
    }
    impl Writer {
        pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ::csv::Error> {
            owned(path.as_ref())?;
            ::csv::Writer::from_path(path).map(|inner| Self { inner, records: 0 })
        }
        pub fn write_record<I, T>(&mut self, record: I) -> Result<(), ::csv::Error>
        where
            I: IntoIterator<Item = T>,
            T: AsRef<[u8]>,
        {
            self.records += 1;
            if (self.records == 1 && fails(Fault::CsvHeader))
                || (self.records > 1 && fails(Fault::CsvRow))
            {
                return Err(error().into());
            }
            self.inner.write_record(record)
        }
        pub fn flush(&mut self) -> io::Result<()> {
            if fails(Fault::CsvFlush) {
                Err(error())
            } else {
                self.inner.flush()
            }
        }
    }
}
