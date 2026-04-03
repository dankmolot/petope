use std::{
    io::{self, ErrorKind},
    net::{SocketAddr, UdpSocket},
    str,
    thread::sleep,
    time::Duration,
};

use clap::Args;

use crate::discovery;

#[derive(Args, Debug)]
pub struct NodeArgs {
    id: String,
    target: String,

    // Discovery server address
    #[arg(long, short, default_value_t = String::from("127.0.0.1:4444"))]
    discovery: String,
}

pub fn main(args: NodeArgs) {
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

    // do not block for too long the reads
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let disco = discovery::DiscoveryClient::new(&args.discovery);
    loop {
        println!("searching for node {:?}", &args.target);
        let mut node_addr = discover_node(&socket, &disco, &args.id, &args.target).unwrap();

        println!(
            "trying to reach node {:?} via {} on {}",
            &args.target,
            node_addr,
            socket.local_addr().unwrap()
        );
        socket
            .send_to(
                format!("hi node {:?}!\n", &args.target).as_bytes(),
                node_addr,
            )
            .unwrap();

        let mut buf = [0; 128];
        match socket.recv_from(&mut buf) {
            Ok((received, addr)) => {
                let data = str::from_utf8(&buf[..received])
                    .map(str::trim)
                    .unwrap_or("");

                println!("{} -> {}", addr, data);
                if addr != node_addr {
                    // try to ask the discovery if the node is the right one
                    node_addr = disco.get(&socket, &args.target).unwrap();
                }

                if addr == node_addr {
                    socket
                        .send_to(
                            format!("got the message, node {:?}!\n", &args.target).as_bytes(),
                            node_addr,
                        )
                        .unwrap();

                    println!("received a message from the node, success!");
                    return;
                }
            }
            Err(e) => {
                if e.kind() != ErrorKind::TimedOut && e.kind() != ErrorKind::WouldBlock {
                    panic!("{}", e)
                }
            }
        }
    }
}

fn discover_node(
    socket: &UdpSocket,
    disco: &discovery::DiscoveryClient,
    id: &str,
    target: &str,
) -> io::Result<SocketAddr> {
    loop {
        disco.register(socket, id)?;
        match disco.get(socket, target) {
            Ok(addr) => {
                return Ok(addr);
            }
            Err(e) => {
                // could be transformed into a match
                if e.kind() != ErrorKind::NotFound
                    && e.kind() != ErrorKind::TimedOut
                    && e.kind() != ErrorKind::WouldBlock
                {
                    return Err(e);
                }
            }
        }

        sleep(Duration::from_secs(5));
    }
}
