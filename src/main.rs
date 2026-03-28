use std::{env, net::UdpSocket, process};

fn main() {
    let args: Vec<String> = env::args().collect();

    if let Some(arg) = args.get(1) {
        match arg.to_lowercase().as_str() {
            "server" => return server(),
            "client" => return client(),
            _ => {}
        }
    }

    println!("please specify either \"server\" or \"client\" argument");
    process::exit(1);
}

fn server() {
    let socket = UdpSocket::bind("127.0.0.1:3333").unwrap();

    // Receives a single datagram message on the socket. If `buf` is too small to hold
    // the message, it will be cut off.
    let mut buf = [0; 32];
    let (amt, src) = socket.recv_from(&mut buf).unwrap();

    // Redeclare `buf` as slice of the received data and send reverse data back to origin.
    let buf = &mut buf[..amt];

    println!(
        "received {} from {}",
        str::from_utf8(buf).unwrap_or("raw bytes"),
        src
    );

    buf.reverse();
    socket.send_to(buf, &src).unwrap();
}

fn client() {
    let socket = UdpSocket::bind("127.0.0.1:3334").unwrap();
    socket.connect("127.0.0.1:3333").unwrap();
    socket.send("hello world".as_bytes()).unwrap();

    let mut buf = [0; 32];
    let (amt, src) = socket.recv_from(&mut buf).unwrap();

    let buf = &mut buf[..amt];

    println!(
        "received {} from {}",
        str::from_utf8(buf).unwrap_or("raw bytes"),
        src
    );
}
