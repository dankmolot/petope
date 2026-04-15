use std::{
    collections::HashMap,
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use clap::Args;
use tokio::net::{TcpListener, TcpStream};

use crate::connection::{Command, Connection};

#[derive(Args, Debug)]
pub struct DiscoveryArgs {
    #[arg(long, short, default_value_t = 4444)]
    port: u16,
}

pub async fn main(args: DiscoveryArgs) -> io::Result<()> {
    let server = Arc::new(DiscoveryServer::new());
    server.listen(&get_addr(args.port)).await?;
    Ok(())
}

type Nodes = Mutex<HashMap<String, SocketAddr>>;

struct DiscoveryServer {
    nodes: Nodes,
}

impl DiscoveryServer {
    pub fn new() -> DiscoveryServer {
        DiscoveryServer {
            nodes: Mutex::new(HashMap::new()),
        }
    }

    pub async fn listen(self: Arc<Self>, addr: &str) -> io::Result<()> {
        let listener = TcpListener::bind(addr).await?;

        loop {
            let (stream, addr) = listener.accept().await?;
            let server = self.clone();
            tokio::spawn(async move {
                println!("+ {}", addr);
                server
                    .handle(stream)
                    .await
                    .with_context(|| format!("connection with {} was closed", addr))
                    .unwrap();
                println!("- {}", addr);
            });
        }
    }
    async fn handle(&self, stream: TcpStream) -> Result<()> {
        let addr = stream.peer_addr().context("get peer addr")?;
        let mut connection = Connection::new(stream);
        while let Some(command) = connection.read_command().await? {
            match command {
                Command::Ping => connection.send_command(&Command::Pong).await?,
                Command::Register { node_id } => {
                    self.nodes.lock().unwrap().insert(node_id, addr);
                }
                Command::Get { node_id } => {
                    let addr = self
                        .nodes
                        .lock()
                        .unwrap()
                        .get(&node_id)
                        .map(SocketAddr::to_owned);

                    match addr {
                        Some(addr) => connection.send_command(&Command::Node { addr }).await?,
                        None => connection.send_command(&Command::NotFound).await?,
                    };
                }
                _ => {}
            };
        }

        Ok(())
    }
}

fn get_addr(port: u16) -> String {
    format!("{}:{}", Ipv4Addr::UNSPECIFIED, port)
}

pub struct DiscoveryClient {
    server: Connection,
}

impl DiscoveryClient {
    pub fn new(stream: TcpStream) -> DiscoveryClient {
        let server = Connection::new(stream);
        DiscoveryClient { server }
    }

    pub async fn ping(&mut self) -> io::Result<()> {
        self.server.send_command(&Command::Ping).await?;
        match self.server.read_command().await? {
            Some(Command::Pong) => Ok(()),
            _ => Err(io::ErrorKind::NotFound.into()),
        }
    }

    pub async fn register(&mut self, id: String) -> io::Result<()> {
        self.server
            .send_command(&Command::Register { node_id: id })
            .await
    }

    pub async fn get(&mut self, id: String) -> io::Result<Option<SocketAddr>> {
        self.server
            .send_command(&Command::Get { node_id: id })
            .await?;

        match self.server.read_command().await? {
            Some(Command::Node { addr }) => Ok(Some(addr)),
            Some(Command::NotFound) => Ok(None),
            resp => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected node or 404 response, but got {:?}", resp),
            )),
        }
    }
}
