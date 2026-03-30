use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
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
