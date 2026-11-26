
use crate::PingSnapshot;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

pub(crate) fn write_varint(value: i32, output: &mut Vec<u8>) {
    let mut remaining = value as u32;
    loop {
        if remaining & !0x7f == 0 {
            output.push(remaining as u8);
            return;
        }
        output.push(((remaining & 0x7f) | 0x80) as u8);
        remaining >>= 7;
    }
}

pub(crate) fn read_varint(input: &mut impl Read) -> Result<i32, String> {
    let mut value = 0_u32;
    for position in 0..5 {
        let mut byte = [0_u8; 1];
        input
            .read_exact(&mut byte)
            .map_err(|error| format!("Could not read the status response: {error}"))?;
        value |= u32::from(byte[0] & 0x7f) << (position * 7);
        if byte[0] & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err("A VarInt in the status response is too long.".into())
}

pub(crate) fn query_server_ping(port: u16) -> Result<PingSnapshot, String> {
    query_at("127.0.0.1", port)
}

pub(crate) fn query_at(host: &str, port: u16) -> Result<PingSnapshot, String> {
    use std::net::ToSocketAddrs;
    let address: SocketAddr = (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("Could not resolve {host}:{port}."))?
        .next()
        .ok_or_else(|| format!("Could not resolve {host}:{port}."))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(800))
        .map_err(|_| format!("Could not connect to server port {host}:{port}."))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(1200)))
        .map_err(|error| error.to_string())?;

    let mut handshake = Vec::new();
    write_varint(0, &mut handshake);
    write_varint(-1, &mut handshake);
    write_varint(host.len() as i32, &mut handshake);
    handshake.extend_from_slice(host.as_bytes());
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(1, &mut handshake);
    let mut frame = Vec::new();
    write_varint(handshake.len() as i32, &mut frame);
    frame.extend_from_slice(&handshake);
    frame.extend_from_slice(&[1, 0]);
    stream
        .write_all(&frame)
        .map_err(|error| format!("Could not send the status request: {error}"))?;

    let packet_length = read_varint(&mut stream)?;
    if !(2..=1_048_576).contains(&packet_length) {
        return Err("The status response length is malformed.".into());
    }
    if read_varint(&mut stream)? != 0 {
        return Err("The status response is malformed.".into());
    }
    let json_length = read_varint(&mut stream)?;
    if json_length < 2 || json_length > packet_length {
        return Err("The status JSON length is malformed.".into());
    }
    let mut json = vec![0_u8; json_length as usize];
    stream
        .read_exact(&mut json)
        .map_err(|error| format!("Could not read the status JSON: {error}"))?;
    let response: serde_json::Value = serde_json::from_slice(&json)
        .map_err(|error| format!("The status JSON is malformed: {error}"))?;
    let players = response
        .get("players")
        .and_then(|value| value.as_object())
        .ok_or("The status has no player info.")?;
    Ok(PingSnapshot {
        online: players
            .get("online")
            .and_then(|value| value.as_u64())
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32,
        max: players
            .get("max")
            .and_then(|value| value.as_u64())
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32,
        names: sample_names(players.get("sample")),
    })
}

fn sample_names(sample: Option<&serde_json::Value>) -> Vec<String> {
    sample
        .and_then(|value| value.as_array())
        .map(|list| {
            list.iter()
                .filter(|one| one.get("id").and_then(|id| id.as_str()) != Some("00000000-0000-0000-0000-000000000000"))
                .filter_map(|one| one.get("name").and_then(|name| name.as_str()))
                .filter(|name| (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .take(12)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{read_varint, write_varint};
    use std::io::Cursor;

    #[test]
    fn encodes_and_decodes_minecraft_varints() {
        for value in [-1, 0, 1, 127, 128, 255, 2_097_151] {
            let mut encoded = Vec::new();
            write_varint(value, &mut encoded);
            assert_eq!(read_varint(&mut Cursor::new(encoded)).unwrap(), value);
        }
    }
}
