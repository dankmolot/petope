use bytes::{Buf, BytesMut};
use std::{
    io::{self, Cursor},
    net::SocketAddr,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufWriter},
    net::TcpStream,
};

// based on https://tokio.rs/tokio/tutorial/framing

#[derive(Debug)]
pub enum Command {
    Ping,
    Pong,
    Register { node_id: String },
    Get { node_id: String },
    Node { addr: SocketAddr },
    NotFound,
}

pub struct Connection {
    stream: BufWriter<TcpStream>,
    buffer: BytesMut,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        Connection {
            stream: BufWriter::new(stream),
            buffer: BytesMut::with_capacity(128),
        }
    }

    pub async fn read_command(&mut self) -> io::Result<Option<Command>> {
        loop {
            if let Some(command) = self.parse_command()? {
                return Ok(Some(command));
            }

            if 0 == self.stream.read_buf(&mut self.buffer).await? {
                if self.buffer.is_empty() {
                    return Ok(None);
                } else {
                    return Err(io::ErrorKind::ConnectionReset.into());
                }
            }
        }
    }

    fn parse_command(&mut self) -> io::Result<Option<Command>> {
        let mut cursor = Cursor::new(&self.buffer[..]);
        let Some(line) = get_line(&mut cursor) else {
            return Ok(None);
        };

        let line =
            str::from_utf8(line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        println!("{} -> {}", self.stream.get_ref().peer_addr()?, line);

        let mut pieces = line.split('|');

        let command = match pieces.next().unwrap() {
            "ping" => Ok(Command::Ping),
            "pong" => Ok(Command::Pong),
            "register" => match pieces.next() {
                Some(v) => Ok(Command::Register {
                    node_id: String::from(v),
                }),
                None => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected node_id, but got nothing for register"),
                )),
            },
            "get" => match pieces.next() {
                Some(v) => Ok(Command::Get {
                    node_id: String::from(v),
                }),
                None => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("expected node_id, but got nothing for get"),
                )),
            },
            "node" => match pieces.next().and_then(|v| v.parse().ok()) {
                Some(v) => Ok(Command::Node { addr: v }),
                None => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("got invalid node address on node"),
                )),
            },
            cmd => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("command \"{}\" is unknown", cmd),
            )),
        };

        let position = cursor.position() as usize;
        self.buffer.advance(position);

        Ok(Some(command?))
    }

    pub async fn send_command(&mut self, command: &Command) -> io::Result<()> {
        match command {
            Command::Ping => self.stream.write_all(b"ping").await?,
            Command::Pong => self.stream.write_all(b"pong").await?,
            Command::Register { node_id } => {
                self.stream.write_all(b"register|").await?;
                self.stream.write_all(node_id.as_bytes()).await?;
            }
            Command::Get { node_id } => {
                self.stream.write_all(b"get|").await?;
                self.stream.write_all(node_id.as_bytes()).await?;
            }
            Command::Node { addr } => {
                self.stream.write_all(b"node|").await?;
                self.stream.write_all(addr.to_string().as_bytes()).await?;
            }
            Command::NotFound => self.stream.write_all(b"404").await?,
        }

        println!(
            "{} -> {}",
            str::from_utf8(self.stream.buffer()).unwrap_or("???"),
            self.stream.get_ref().peer_addr()?
        );
        self.stream.write_all(b"\n").await?;
        self.stream.flush().await?;

        Ok(())
    }
}

// https://github.com/tokio-rs/mini-redis/blob/66aaf9ec9782e469ff04fcf2490f9ca677571761/src/frame.rs#L248-L265
fn get_line<'a>(src: &'a mut Cursor<&[u8]>) -> Option<&'a [u8]> {
    if src.get_ref().is_empty() {
        return None;
    }

    let start = src.position() as usize;
    let end = src.get_ref().len();

    for i in start..end {
        if src.get_ref()[i] == b'\n' {
            src.set_position((i + 1) as u64);
            return Some(&src.get_ref()[start..i]);
        }
    }

    None
}
