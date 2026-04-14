use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
    time::Duration,
};

use socket2::{Domain, Protocol, Socket, Type};

fn get_socket(bind: Option<SocketAddr>) -> io::Result<Socket> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Protocol::TCP.into())?;

    socket.set_reuse_port(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;

    if let Some(bind) = bind {
        socket.bind(&bind.into())?;
    }

    Ok(socket)
}

// just a TcpStream::connect with SO_REUSEPORT + addr binding + timeout options
pub fn socket_connect(bind: Option<SocketAddr>, addr: SocketAddr) -> io::Result<TcpStream> {
    let socket = get_socket(bind)?;
    socket.connect(&addr.into())?;
    Ok(socket.into())
}

pub fn socket_listen(bind: Option<SocketAddr>) -> io::Result<TcpListener> {
    let socket = get_socket(bind)?;
    socket.listen(128)?;
    Ok(socket.into())
}
