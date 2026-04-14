use std::{
    io::{self, ErrorKind, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, UdpSocket},
    process::exit,
    str,
    thread::sleep,
    time::Duration,
};

use clap::Args;

use crate::{
    discovery::{self, DiscoveryClient},
    utils,
};

#[derive(Args, Debug)]
pub struct NodeArgs {
    id: String,
    target: String,

    // Discovery server address
    #[arg(long, short, default_value_t = String::from("127.0.0.1:4444"))]
    discovery: String,
}

pub fn main(args: NodeArgs) {
    let discovery_addr = args.discovery.parse().unwrap();
    let stream = utils::socket_connect(None, discovery_addr).unwrap();
    let mut disco = DiscoveryClient::new(stream);

    loop {
        println!("searching for node {:?}", &args.target);
        let node_addr = discover_node(&mut disco, &args.id, &args.target).unwrap();

        println!(
            "trying to reach node {:?} via {} on {}",
            &args.target,
            node_addr,
            disco.stream.local_addr().unwrap()
        );

        let mut stream = match utils::socket_connect(disco.stream.local_addr().ok(), node_addr) {
            Ok(stream) => stream,
            Err(e) => {
                if e.kind() == io::ErrorKind::ConnectionRefused {
                    let listener = utils::socket_listen(disco.stream.local_addr().ok()).unwrap();
                    listen(&args, &mut disco, listener, node_addr);
                    continue;
                }

                panic!("connect error: {}", e)
            }
        };

        stream
            .write(format!("hi node {:?}!\n", &args.target).as_bytes())
            .unwrap();

        let mut buf = [0; 128];
        match stream.read(&mut buf) {
            Ok(received) => {
                let data = str::from_utf8(&buf[..received])
                    .map(str::trim)
                    .unwrap_or("");

                println!("{} -> {}", stream.peer_addr().unwrap(), data);

                stream
                    .write(format!("got the message, node {:?}!\n", &args.target).as_bytes())
                    .unwrap();

                println!("received a message from the node, success!");
                return;
            }
            Err(e) => {
                if e.kind() != ErrorKind::TimedOut
                    && e.kind() != ErrorKind::WouldBlock
                    && e.kind() != ErrorKind::ConnectionReset
                {
                    panic!("{}", e)
                }
            }
        }
    }
}

fn discover_node(
    disco: &mut discovery::DiscoveryClient,
    id: &str,
    target: &str,
) -> io::Result<SocketAddr> {
    loop {
        disco.register(id)?;
        match disco.get(target) {
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

fn listen(
    args: &NodeArgs,
    disco: &mut discovery::DiscoveryClient,
    listener: TcpListener,
    node_addr: SocketAddr,
) {
    println!(
        "listening for connections on {}",
        listener.local_addr().unwrap()
    );
    let (mut stream, mut addr) = listener.accept().unwrap();
    println!("got incoming connection: {}", addr);
    if addr != node_addr {
        addr = disco.get(&args.target).unwrap();
    }

    if addr != node_addr {
        return;
    }

    let mut buf = [0; 1024];
    match stream.read(&mut buf) {
        Ok(received) => {
            let data = str::from_utf8(&buf[..received])
                .map(str::trim)
                .unwrap_or("");

            println!("{} -> {}", stream.peer_addr().unwrap(), data);

            stream
                .write(format!("got the message, node {:?}!\n", &args.target).as_bytes())
                .unwrap();

            println!("received a message from the node, success!");
            exit(0);
        }
        Err(e) => {
            eprintln!("read error: {}", e);
        }
    }
}
