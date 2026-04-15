use std::{io, net::SocketAddr};
use tokio::net::TcpSocket;

pub fn reusable_socket(bind: Option<SocketAddr>) -> io::Result<TcpSocket> {
    let socket = TcpSocket::new_v4()?;

    socket.set_reuseport(true)?;

    if let Some(bind) = bind {
        socket.bind(bind)?;
    }

    Ok(socket)
}
