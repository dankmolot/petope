use std::{
    collections::HashMap,
    io::{self, Error, ErrorKind, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    str::{self, FromStr},
    sync::{Arc, Mutex},
    thread,
};

use clap::Args;

#[derive(Args, Debug)]
pub struct DiscoveryArgs {
    #[arg(long, short, default_value_t = 4444)]
    port: u16,
}

pub fn main(args: DiscoveryArgs) {
    let socket = get_socket(args.port);
    let nodes = Arc::new(Mutex::new(HashMap::new()));

    for stream in socket.incoming() {
        match stream {
            Err(e) => {
                eprintln!("incoming connection error: {}", e);
            }
            Ok(stream) => {
                let addr = stream.peer_addr().unwrap();
                let nodes = nodes.clone();
                thread::spawn(move || {
                    handle_connection(stream, addr, nodes)
                        .unwrap_or_else(|e| eprintln!("handle connection: {e}"))
                });
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    nodes: Arc<Mutex<HashMap<String, SocketAddr>>>,
) -> io::Result<()> {
    let mut buf = [0; 1024];
    loop {
        let received = stream.read(&mut buf)?;
        process(&mut stream, addr, &buf[..received], &nodes)?;
    }
}

fn process(
    stream: &mut TcpStream,
    addr: SocketAddr,
    buf: &[u8],
    nodes: &Mutex<HashMap<String, SocketAddr>>,
) -> io::Result<()> {
    let Ok(mut data) = str::from_utf8(buf) else {
        return Ok(());
    };

    data = data.trim();
    if data.is_empty() {
        return Ok(());
    }

    let mut args = data.split("|");
    let Some(command) = args.next() else {
        return Ok(());
    };

    println!("{} -> {}", addr, data);

    match command {
        // you may ask why \n at the end? so I can debug with netcat :)
        "ping" => {
            stream.write(b"pong\n")?;
        }
        "register" => {
            let Some(node_id) = args.next() else {
                println!("{} forgot to send an id!", addr);
                return Ok(());
            };

            let mut nodes = nodes.lock().unwrap();
            nodes.insert(String::from(node_id), addr);
            println!(
                "now {} known as {:?} (total {}/{} nodes in map)",
                addr,
                node_id,
                nodes.len(),
                nodes.capacity()
            )
        }
        "get" => {
            let Some(node_id) = args.next() else {
                println!("{} forgot to send an id!", addr);
                return Ok(());
            };

            let nodes = nodes.lock().unwrap();
            match nodes.get(node_id) {
                Some(node_addr) => {
                    println!("node {:?} was found as {}", node_id, node_addr);
                    stream.write(format!("found|{}\n", node_addr).as_bytes())?;
                }
                None => {
                    println!("node {:?} not found", node_id);
                    stream.write("404\n".as_bytes())?;
                }
            }
        }
        _ => {}
    };

    Ok(())
}

fn get_socket(port: u16) -> TcpListener {
    let socket = TcpListener::bind(get_addr(port).as_str()).unwrap();
    println!(
        "listening discovery server on {}",
        socket.local_addr().unwrap()
    );
    return socket;
}

fn get_addr(port: u16) -> String {
    format!("{}:{}", Ipv4Addr::UNSPECIFIED, port)
}

#[derive(Debug)]
pub struct DiscoveryClient {
    pub stream: TcpStream,
}

impl DiscoveryClient {
    pub fn new(stream: TcpStream) -> DiscoveryClient {
        DiscoveryClient { stream }
    }

    fn recv(&mut self) -> io::Result<String> {
        let mut buf = [0; 128];
        let received = self.stream.read(&mut buf)?;

        let data: String = str::from_utf8(&buf[..received])
            .map(|v| String::from(v.trim()))
            .map_err(|_| Error::from(io::ErrorKind::InvalidData))?;

        // println!("{} -> {}", self.addr, data.as_str());

        Ok(data)
    }

    fn send(&mut self, args: &[&str]) -> io::Result<()> {
        let result = args.join("|");
        self.stream.write(result.as_bytes())?;
        // println!("{} -> {}", result.as_str(), self.addr);
        Ok(())
    }

    pub fn ping(&mut self) -> io::Result<()> {
        self.send(&["ping"])?;
        let pong = self.recv().map(|v| v == "pong").unwrap_or(false);

        if !pong {
            return Err(Error::from(ErrorKind::NotFound));
        }

        Ok(())
    }

    pub fn register(&mut self, node_id: &str) -> io::Result<()> {
        self.send(&["register", node_id])?;
        Ok(())
    }

    pub fn get(&mut self, node_id: &str) -> io::Result<SocketAddr> {
        self.send(&["get", node_id])?;
        let response = self.recv()?;
        let mut args = response.split("|");
        if let Some(result) = args.next() {
            if result == "found" {
                if let Some(node_addr) =
                    args.next().and_then(|addr| SocketAddr::from_str(addr).ok())
                {
                    return Ok(node_addr);
                }
            }
        }

        Err(Error::from(ErrorKind::NotFound))
    }
}
