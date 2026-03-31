use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};

#[derive(Debug)]
pub struct TcpListening {
    listener: TcpListener,
    max_conn_queued: usize,
    peers: VecDeque<TcpTransfering>,
}

impl TcpListening {
    pub fn bind(port: u16, local_only: bool) -> io::Result<Self> {
        let host = if local_only { "127.0.0.1" } else { "0.0.0.0" };
        let listener = TcpListener::bind((host, port))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            max_conn_queued: 200,
            peers: VecDeque::new(),
        })
    }

    pub fn max_conn_queued_set(&mut self, max_conn_queued: usize) {
        self.max_conn_queued = max_conn_queued.max(1);
    }

    pub fn accept_ready(&mut self) -> io::Result<usize> {
        let mut accepted = 0usize;
        loop {
            if self.peers.len() >= self.max_conn_queued {
                break;
            }

            match self.listener.accept() {
                Ok((stream, _)) => {
                    self.peers.push_back(TcpTransfering::from_stream(stream)?);
                    accepted += 1;
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => return Err(err),
            }
        }
        Ok(accepted)
    }

    pub fn next_peer(&mut self) -> Option<TcpTransfering> {
        self.peers.pop_front()
    }
}

#[derive(Debug)]
pub struct TcpTransfering {
    stream: TcpStream,
    pending_write: VecDeque<u8>,
    read_ready: bool,
    send_ready: bool,
    done: bool,
    local_addr: Option<SocketAddr>,
    remote_addr: Option<SocketAddr>,
    bytes_received: usize,
    bytes_sent: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadStatus {
    Data(usize),
    WouldBlock,
    Closed,
}

impl TcpTransfering {
    pub fn from_stream(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        let local_addr = stream.local_addr().ok();
        let remote_addr = stream.peer_addr().ok();
        Ok(Self {
            stream,
            pending_write: VecDeque::new(),
            read_ready: true,
            send_ready: true,
            done: false,
            local_addr,
            remote_addr,
            bytes_received: 0,
            bytes_sent: 0,
        })
    }

    pub fn connect(host_addr: &str, host_port: u16) -> io::Result<Self> {
        let mut addrs = (host_addr, host_port).to_socket_addrs()?;
        let Some(addr) = addrs.next() else {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "host did not resolve to any socket address",
            ));
        };
        let stream = TcpStream::connect(addr)?;
        Self::from_stream(stream)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<ReadStatus> {
        if !self.read_ready {
            return Ok(ReadStatus::WouldBlock);
        }
        if self.done {
            return Ok(ReadStatus::Closed);
        }

        match self.stream.read(buf) {
            Ok(0) => {
                self.done = true;
                Ok(ReadStatus::Closed)
            }
            Ok(bytes) => {
                self.bytes_received += bytes;
                Ok(ReadStatus::Data(bytes))
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => Ok(ReadStatus::WouldBlock),
            Err(err) => {
                self.done = true;
                Err(err)
            }
        }
    }

    pub fn read_available(&mut self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = [0u8; 512];
        loop {
            match self.read(&mut buf)? {
                ReadStatus::Data(bytes) => out.extend_from_slice(&buf[..bytes]),
                ReadStatus::WouldBlock => break,
                ReadStatus::Closed => break,
            }
        }
        Ok(out)
    }

    pub fn queue_send(&mut self, data: &[u8]) -> io::Result<()> {
        self.pending_write.extend(data.iter().copied());
        let _ = self.flush_pending()?;
        Ok(())
    }

    pub fn flush_pending(&mut self) -> io::Result<bool> {
        if !self.send_ready || self.done {
            return Ok(false);
        }

        while !self.pending_write.is_empty() {
            let contiguous = self.pending_write.make_contiguous();
            match self.stream.write(contiguous) {
                Ok(0) => {
                    self.done = true;
                    return Ok(false);
                }
                Ok(bytes) => {
                    self.bytes_sent += bytes;
                    self.pending_write.drain(..bytes);
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => return Ok(true),
                Err(err) => {
                    self.done = true;
                    return Err(err);
                }
            }
        }

        Ok(true)
    }

    pub fn done_set(&mut self) {
        self.done = true;
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    pub fn has_pending_write(&self) -> bool {
        !self.pending_write.is_empty()
    }

    pub fn addr_remote(&self) -> Option<SocketAddr> {
        self.remote_addr
    }

    pub fn addr_local(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    pub fn is_open(&self) -> bool {
        !self.done
    }

    pub fn bytes_received(&self) -> usize {
        self.bytes_received
    }

    pub fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn loopback_exchange_works() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut conn = TcpTransfering::from_stream(stream).unwrap();
            thread::sleep(Duration::from_millis(20));
            let bytes = conn.read_available().unwrap();
            assert_eq!(String::from_utf8(bytes).unwrap(), "ping");
            conn.queue_send(b"pong").unwrap();
            conn.flush_pending().unwrap();
        });

        let mut client = TcpTransfering::connect("127.0.0.1", port).unwrap();
        client.queue_send(b"ping").unwrap();
        thread::sleep(Duration::from_millis(40));
        let bytes = client.read_available().unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "pong");
        handle.join().unwrap();
    }
}
