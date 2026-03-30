use std::{
    collections::HashMap,
    io::{self, Error, ErrorKind},
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    str::{self, FromStr},
};

use clap::Args;

#[derive(Args, Debug)]
pub struct DiscoveryArgs {
    #[arg(long, short, default_value_t = 4444)]
    port: u16,
}

pub fn main(args: DiscoveryArgs) {
    let socket = get_socket(args.port);

    let mut buf = [0; 1024];
    let mut nodes: HashMap<String, SocketAddr> = HashMap::new();

    loop {
        let (received, addr) = socket.recv_from(&mut buf).unwrap();
        process(&socket, addr, &buf[..received], &mut nodes);
    }
}

fn process(
    socket: &UdpSocket,
    addr: SocketAddr,
    buf: &[u8],
    nodes: &mut HashMap<String, SocketAddr>,
) {
    let Ok(mut data) = str::from_utf8(buf) else {
        return;
    };

    data = data.trim();
    if data.is_empty() {
        return;
    }

    let mut args = data.split("|");
    let Some(command) = args.next() else { return };

    println!("{} -> {}", addr, data);

    match command {
        // you may ask why \n at the end? so I can debug with netcat :)
        "ping" => {
            socket.send_to(b"pong\n", addr).unwrap();
        }
        "register" => {
            let Some(node_id) = args.next() else {
                println!("{} forgot to send an id!", addr);
                return;
            };

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
                return;
            };

            match nodes.get(node_id) {
                Some(node_addr) => {
                    println!("node {:?} was found as {}", node_id, node_addr);
                    socket
                        .send_to(format!("found|{}\n", node_addr).as_bytes(), addr)
                        .unwrap();
                }
                None => {
                    println!("node {:?} not found", node_id);
                    socket.send_to("404\n".as_bytes(), addr).unwrap();
                }
            }
        }
        _ => {}
    };
}

fn get_socket(port: u16) -> UdpSocket {
    let socket = UdpSocket::bind(get_addr(port).as_str()).unwrap();
    println!(
        "listening discovery server on {}",
        socket.local_addr().unwrap()
    );
    return socket;
}

fn get_addr(port: u16) -> String {
    format!("{}:{}", Ipv4Addr::UNSPECIFIED, port)
}

#[derive(Debug, Clone, Copy)]
pub struct DiscoveryClient {
    addr: SocketAddr,
}

impl DiscoveryClient {
    pub fn new(target: &str) -> DiscoveryClient {
        let addr =
            SocketAddr::from_str(target).expect("unable to parse given discovery server addr");

        DiscoveryClient { addr }
    }

    fn connect(&self, socket: &UdpSocket) -> io::Result<()> {
        socket.connect(self.addr)
    }

    fn recv(&self, socket: &UdpSocket) -> io::Result<String> {
        let mut buf = [0; 128];
        let received = socket.recv(&mut buf)?;
        let data = str::from_utf8(&buf[..received])
            .map(|v| String::from(v.trim()))
            .map_err(|_| Error::from(io::ErrorKind::InvalidData))?;

        // println!("{} -> {}", self.addr, data.as_str());

        Ok(data)
    }

    fn send(&self, socket: &UdpSocket, args: &[&str]) -> io::Result<()> {
        let result = args.join("|");
        socket.send(result.as_bytes())?;
        // println!("{} -> {}", result.as_str(), self.addr);
        Ok(())
    }

    pub fn ping(&self, socket: &UdpSocket) -> io::Result<()> {
        self.connect(socket)?;
        self.send(socket, &["ping"])?;
        let pong = self.recv(socket).map(|v| v == "pong").unwrap_or(false);

        if !pong {
            return Err(Error::from(ErrorKind::NotFound));
        }

        Ok(())
    }

    pub fn register(&self, socket: &UdpSocket, node_id: &str) -> io::Result<()> {
        self.connect(socket)?;
        self.send(socket, &["register", node_id])?;
        Ok(())
    }

    pub fn get(&self, socket: &UdpSocket, node_id: &str) -> io::Result<SocketAddr> {
        self.connect(socket)?;
        self.send(socket, &["get", node_id])?;
        let response = self.recv(socket)?;
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
