use nix::{sys::stat::Mode, unistd};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use tokio::{fs, io};

/// Create a new Unix named pipe on filesystem
fn create_pipe<P: ?Sized + nix::NixPath>(path: &P, mode: Option<Mode>) -> nix::Result<()> {
    unistd::mkfifo(
        path,
        mode.unwrap_or_else(|| Mode::from_bits_truncate(0o660)),
    )
}

/// Delete a Unix named pipe from filesystem
async fn remove_pipe<P: AsRef<Path>>(path: P) -> io::Result<()> {
    fs::remove_file(&path).await
}

/// This object represents a path to a Unix named pipe
#[derive(Clone)]
pub struct Pipe {
    inner: PathBuf,
}

impl Pipe {
    pub fn new<T: Into<PathBuf>>(path: T) -> Self {
        Self { inner: path.into() }
    }
    /// Check if the path exists
    pub fn exists(&self) -> bool {
        self.inner.exists()
    }
    /// Make sure the path exists, otherwise create a named pipe in its place
    pub fn ensure_exists(&self) -> nix::Result<()> {
        if !self.exists() {
            create_pipe(&self.inner, None)
        } else {
            Ok(())
        }
    }
    /// Try to delete the pipe from filesystem and consume the `NamedPipe`
    pub async fn delete(self) -> io::Result<()> {
        if self.inner.exists() {
            remove_pipe(&self.inner).await
        } else {
            Ok(())
        }
    }

    /// Create a reader for this named pipe
    pub fn reader(&self) -> Reader {
        Reader::from_path(self)
    }
    /// Create a writer for this named pipe
    pub fn writer(&self) -> Writer {
        Writer::from_path(self)
    }
}

/// An util wrapper for reading from Unix named pipes
pub struct Reader {
    path: Pipe,
}

impl Reader {
    /// Create a new reader, clone the specific Pipe
    pub fn from_path(source: &Pipe) -> Self {
        Self {
            path: source.clone(),
        }
    }
    /// Check if the named pipe actually exists, otherwise try to create it
    pub fn pipe_exists(&self) -> nix::Result<&Self> {
        self.path.ensure_exists()?;
        Ok(self)
    }
    /// Read all bytes from the pipe no async
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        std::fs::read(&self.path.inner)
    }
    /// Read all bytes from the pipe
    /// The returned Future will resolve when something is written to the pipe
    pub async fn async_read(&self) -> io::Result<Vec<u8>> {
        fs::read(&self.path.inner).await
    }
    /// Read a String from the pipe no async
    /// The returned Future will resolve when something is written to the pipe
    pub fn string(&self) -> std::io::Result<String> {
        std::fs::read_to_string(&self.path.inner)
    }
    /// Reads a String from the pipe.
    /// The returned Future will resolve when something is written to the pipe
    pub async fn async_read_str(&self) -> io::Result<String> {
        fs::read_to_string(&self.path.inner).await
    }
}

/// An util wrapper for writing to Unix named pipes
pub struct Writer {
    path: Pipe,
}

impl Writer {
    async fn _write(&self, data: &[u8]) -> io::Result<()> {
        use io::AsyncWriteExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(false)
            .open(&self.path.inner)
            .await?;
        file.write_all(data).await
    }
    pub fn from_path(source: &Pipe) -> Self {
        Self {
            path: source.clone(),
        }
    }
    /// Check if the named pipe actually exists, otherwise try to create it
    pub fn pipe_exists(&self) -> nix::Result<&Self> {
        self.path.ensure_exists()?;
        Ok(self)
    }
    /// Write byte data to the pipe
    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        let mut buffer = std::fs::File::create(&self.path.inner.to_str().unwrap())?;
        buffer.write_all(data)?;
        Ok(())
    }
    /// Write byte data to the pipe
    pub async fn async_write(&self, data: &[u8]) -> io::Result<()> {
        self._write(data).await
    }
    /// Write &str data to the pipe
    pub fn write_str(&self, data: String) -> std::io::Result<()> {
        let mut buffer = std::fs::File::create(&self.path.inner.to_str().unwrap())?;
        buffer.write_all(data.as_bytes())?;
        Ok(())
    }
    /// Write &str data to the pipe
    pub async fn async_write_str(&self, data: &str) -> io::Result<()> {
        self._write(data.as_bytes()).await
    }
}

#[cfg(test)]
mod tests {
    use tokio::runtime::Handle;
    use tokio::{io, task};

    #[ignore]
    #[tokio::test]
    async fn write_and_read_threaded() -> io::Result<()> {
        use std::thread;
        let pipe = super::Pipe::new("/tmp/test_pipe_0");
        pipe.ensure_exists().unwrap();
        let writer = pipe.writer();
        let reader = pipe.reader();
        let data_to_send = "Hello pipe";
        let handle = Handle::current();
        let handle_erad = handle.clone();
        let handle_del = handle.clone();
        let t_write = thread::spawn(move || handle.block_on(writer.async_write_str(data_to_send)));
        let t_read = thread::spawn(move || handle_erad.block_on(reader.async_read_str()));
        let _a = t_write.join().unwrap();
        let read_result = t_read.join().unwrap()?;
        assert_eq!(read_result, data_to_send);
        handle_del.block_on(pipe.delete())
        // _a
    }

    #[tokio::test]
    async fn ensure_on_write() -> io::Result<()> {
        task::spawn(async move {
            let pipe = super::Pipe::new("/tmp/test_pipe_1");
            pipe.ensure_exists().unwrap();
            let writer = pipe.writer();
            let reader = pipe.reader();
            let data_to_send = "Hello pipe";
            let t1 = task::spawn(async move {
                writer
                    .pipe_exists()
                    .unwrap()
                    .async_write(data_to_send.as_bytes())
                    .await
            });
            let t2 = task::spawn(async move { reader.async_read().await });
            let _a = t1.await?;
            let read_result = t2.await?;
            assert_eq!(read_result.unwrap(), data_to_send.as_bytes());
            pipe.delete().await
        })
        .await?
    }

    #[tokio::test]
    async fn ensure_on_read() -> io::Result<()> {
        task::spawn(async move {
            let pipe = super::Pipe::new("/tmp/test_pipe_2");
            pipe.ensure_exists().unwrap();
            let writer = pipe.writer();
            let reader = pipe.reader();
            let data_to_send = "Hello pipe";
            let t1 = task::spawn(async move { writer.async_write(data_to_send.as_bytes()).await });
            let t2 = task::spawn(async move { reader.pipe_exists().unwrap().async_read().await });
            let _a = t1.await?;
            let read_result = t2.await?;
            assert_eq!(read_result.unwrap(), data_to_send.as_bytes());
            pipe.delete().await
        })
        .await?
    }

    #[tokio::test]
    async fn write_and_read_async() -> io::Result<()> {
        task::spawn(async move {
            let pipe = super::Pipe::new("/tmp/test_pipe_3");
            pipe.ensure_exists().unwrap();
            let writer = pipe.writer();
            let reader = pipe.reader();
            let data_to_send = "Hello pipe";
            let t1 = task::spawn(async move { writer.async_write(data_to_send.as_bytes()).await });
            let t2 = task::spawn(async move { reader.async_read_str().await });
            let _a = t1.await?;
            let read_result = t2.await?;
            assert_eq!(read_result.unwrap(), data_to_send);
            pipe.delete().await
        })
        .await?
    }
}
