use base64::Engine;
use bytes::{BufMut, BytesMut};
use etherparse::{
    Icmpv4Type, Icmpv6Type, IpSlice, PacketBuilder, icmpv4::DestUnreachableHeader,
    icmpv6::DestUnreachableCode,
};
use futures_lite::{Stream, StreamExt};
use iroh::EndpointId;
use std::net::{Ipv4Addr, Ipv6Addr};

pub const HOP_LIMIT: u8 = 64;

pub fn base64_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub fn base64_decode(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::engine::general_purpose::STANDARD.decode(encoded)
}

pub fn u8_pair(a: u8, b: u8) -> u16 {
    ((a as u16) << 8) | b as u16
}

pub fn ipv4_from_id(id: &EndpointId) -> Ipv4Addr {
    Ipv4Addr::new(10, id[0], id[1], id[2])
}

pub fn ipv6_from_id(id: &EndpointId) -> Ipv6Addr {
    Ipv6Addr::new(
        0xfd22,
        u8_pair(id[0], id[1]),
        u8_pair(id[2], id[3]),
        u8_pair(id[4], id[5]),
        u8_pair(id[6], id[7]),
        u8_pair(id[8], id[9]),
        u8_pair(id[10], id[11]),
        u8_pair(id[12], id[13]),
    )
}

// drains immideately available values from the stream
pub async fn drain<S>(stream: &mut S) -> usize
where
    S: Stream + Unpin,
{
    let mut drain = stream.drain();
    let mut drained = 0;
    while let Some(_) = drain.next().await {
        drained += 1;
    }
    drained
}

pub fn fragmentation_needed_response(ip: &IpSlice, payload: &[u8], mtu: usize) -> BytesMut {
    match ip {
        IpSlice::Ipv4(v4) => {
            let header = v4.header();
            let next_hop_mtu = mtu.try_into().unwrap_or(std::u16::MAX);
            let builder = PacketBuilder::ipv4(header.destination(), header.source(), HOP_LIMIT)
                .icmpv4(Icmpv4Type::DestinationUnreachable(
                    DestUnreachableHeader::FragmentationNeeded { next_hop_mtu },
                ));

            let payload_len = payload.len().min(header.slice().len() + 8); // ipv4 requires only ip header + 64bit (bytes) of transport payload
            let payload = &payload[..payload_len];

            let mut writer = BytesMut::with_capacity(builder.size(payload.len())).writer();
            builder.write(&mut writer, payload).unwrap();
            writer.into_inner()
        }
        IpSlice::Ipv6(v6) => {
            let header = v6.header();
            let mtu = mtu.try_into().unwrap_or(std::u32::MAX);
            let builder = PacketBuilder::ipv6(header.destination(), header.source(), HOP_LIMIT)
                .icmpv6(Icmpv6Type::PacketTooBig { mtu });

            let payload_len = payload.len().min(mtu as usize - header.slice().len()); // ipv6 requires entire ip payload in mtu
            let payload = &payload[..payload_len];

            let mut writer = BytesMut::with_capacity(builder.size(payload.len())).writer();
            builder.write(&mut writer, payload).unwrap(); // ipv6 does not care about payload size
            writer.into_inner()
        }
    }
}

pub fn unreachable_destination_response(ip: &IpSlice, payload: &[u8], mtu: usize) -> BytesMut {
    match ip {
        IpSlice::Ipv4(v4) => {
            let header = v4.header();
            let builder = PacketBuilder::ipv4(header.destination(), header.source(), HOP_LIMIT)
                .icmpv4(Icmpv4Type::DestinationUnreachable(
                    DestUnreachableHeader::Host,
                ));

            let payload_len = payload.len().min(header.slice().len() + 8); // ipv4 requires only ip header + 64bit (bytes) of transport payload
            let payload = &payload[..payload_len];

            let mut writer = BytesMut::with_capacity(builder.size(payload.len())).writer();
            builder.write(&mut writer, payload).unwrap();
            writer.into_inner()
        }
        IpSlice::Ipv6(v6) => {
            let header = v6.header();
            let mtu = mtu.try_into().unwrap_or(std::u32::MAX);
            let builder = PacketBuilder::ipv6(header.destination(), header.source(), HOP_LIMIT)
                .icmpv6(Icmpv6Type::DestinationUnreachable(
                    DestUnreachableCode::Address,
                ));

            let payload_len = payload.len().min(mtu as usize - header.slice().len()); // ipv6 requires entire ip payload in mtu
            let payload = &payload[..payload_len];

            let mut writer = BytesMut::with_capacity(builder.size(payload.len())).writer();
            builder.write(&mut writer, payload).unwrap(); // ipv6 does not care about payload size
            writer.into_inner()
        }
    }
}
