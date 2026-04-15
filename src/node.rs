use std::{net::SocketAddr, process::exit, thread::sleep, time::Duration};

use anyhow::{Context, Result, bail};
use bytes::BytesMut;
use clap::Args;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, lookup_host},
    stream,
};

use crate::{discovery::DiscoveryClient, utils};

#[derive(Args, Debug)]
pub struct NodeArgs {
    id: String,
    target: String,

    // Discovery server address
    #[arg(long, short, default_value_t = String::from("127.0.0.1:4444"))]
    discovery: String,
}

pub async fn main(args: NodeArgs) -> Result<()> {
    let local_addr = {
        let listener = utils::reusable_socket(Some("0.0.0.0:0".parse()?))?.listen(8)?;
        let local_addr = listener.local_addr()?;

        tokio::spawn(async move {
            loop {
                let (stream, addr) = listener.accept().await.unwrap();
                println!("connection from {}", addr);
                exit(0);
            }
        });

        local_addr
    };

    let mut discovery = {
        let host = lookup_host(args.discovery)
            .await?
            .filter(|v| v.is_ipv4()) // no ipv6 support yet :p
            .next()
            .context("unable to lookup discovery address")?;

        let stream = utils::reusable_socket(Some(local_addr))?
            .connect(host)
            .await
            .with_context(|| format!("connect to discovery server via {}", host))?;

        DiscoveryClient::new(stream)
    };

    discovery.register(args.id.clone()).await?;
    match discovery.get(args.id.clone()).await? {
        Some(public_addr) => {
            println!(
                "registered as \"{}\" with address {}",
                &args.id, public_addr
            )
        }
        None => bail!("discovery server failed to register us"),
    };

    loop {
        if let Some(node_addr) = discovery.get(args.target.clone()).await.ok().flatten() {
            println!("trying to connect to {}", node_addr);
            match utils::reusable_socket(Some(local_addr))?
                .connect(node_addr)
                .await
            {
                Ok(mut stream) => {
                    println!("connected!");
                    stream
                        .write_all(format!("yo, i am {}", &args.id).as_bytes())
                        .await?;

                    let mut buf = BytesMut::new();
                    stream.read_buf(&mut buf).await?;
                    println!("message: {}", std::str::from_utf8(buf.as_ref()).unwrap());

                    sleep(Duration::from_secs(1));
                    exit(0);
                }
                Err(e) => {
                    println!("unable due {}", e);
                }
            }
        }

        sleep(Duration::from_secs(5));
    }

    Ok(())
}
