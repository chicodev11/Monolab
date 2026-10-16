
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

const TYPE_RESPONSE: i32 = 0;
const TYPE_COMMAND: i32 = 2;
const TYPE_AUTH: i32 = 3;
const AUTH_FAILED: i32 = -1;
const MAX_PACKET: i32 = 4 * 1024 * 1024;

pub struct RconClient {
    stream: TcpStream,
    next_id: i32,
}

impl RconClient {
    pub fn connect(host: &str, port: u16, password: &str) -> Result<Self, String> {
        if password.is_empty() {
            return Err("The RCON password is empty. Check rcon.password in server.properties.".into());
        }
        let address: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|_| format!("The RCON address is malformed: {host}:{port}"))?;
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|_| {
            format!(
                "Could not connect to RCON port {port}. Make sure enable-rcon=true in server.properties."
            )
        })?;
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .map_err(|error| error.to_string())?;
        let mut client = Self {
            stream,
            next_id: 1,
        };
        client.authenticate(password)?;
        Ok(client)
    }

    fn authenticate(&mut self, password: &str) -> Result<(), String> {
        let id = self.send(TYPE_AUTH, password)?;
        loop {
            let (response_id, kind, _) = self.receive()?;
            if kind == TYPE_RESPONSE {
                continue;
            }
            if response_id == AUTH_FAILED {
                return Err("The RCON password is wrong.".into());
            }
            if response_id == id {
                return Ok(());
            }
        }
    }

    pub fn command(&mut self, command: &str) -> Result<String, String> {
        let id = self.send(TYPE_COMMAND, command)?;
        let sentinel = self.send(TYPE_COMMAND, "")?;
        let mut body = String::new();
        loop {
            let (response_id, _, text) = self.receive()?;
            if response_id == sentinel {
                return Ok(body);
            }
            if response_id == id {
                body.push_str(&text);
            }
        }
    }

    fn send(&mut self, kind: i32, body: &str) -> Result<i32, String> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let payload = body.as_bytes();
        let length = (10 + payload.len()) as i32;
        let mut packet = Vec::with_capacity(length as usize + 4);
        packet.extend_from_slice(&length.to_le_bytes());
        packet.extend_from_slice(&id.to_le_bytes());
        packet.extend_from_slice(&kind.to_le_bytes());
        packet.extend_from_slice(payload);
        packet.extend_from_slice(&[0, 0]);
        self.stream
            .write_all(&packet)
            .map_err(|error| format!("Could not send the RCON command: {error}"))?;
        self.stream.flush().map_err(|error| error.to_string())?;
        Ok(id)
    }

    fn receive(&mut self) -> Result<(i32, i32, String), String> {
        let mut header = [0_u8; 4];
        self.stream
            .read_exact(&mut header)
            .map_err(|error| format!("Could not read the RCON response: {error}"))?;
        let length = i32::from_le_bytes(header);
        if !(10..=MAX_PACKET).contains(&length) {
            return Err("The RCON response length is malformed.".into());
        }
        let mut rest = vec![0_u8; length as usize];
        self.stream
            .read_exact(&mut rest)
            .map_err(|error| format!("Could not read the RCON response: {error}"))?;
        let id = i32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
        let kind = i32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]);
        let body = &rest[8..rest.len().saturating_sub(2)];
        Ok((id, kind, String::from_utf8_lossy(body).to_string()))
    }
}

pub fn rcon_settings(properties: &str) -> Option<(u16, String)> {
    let mut enabled = false;
    let mut port = 25575_u16;
    let mut password = String::new();
    for line in properties.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "enable-rcon" => enabled = value.trim() == "true",
            "rcon.port" => port = value.trim().parse().unwrap_or(25575),
            "rcon.password" => password = crate::properties::unescape(value.trim()),
            _ => {}
        }
    }
    enabled.then_some((port, password))
}

#[cfg(test)]
mod tests {
    use super::rcon_settings;

    #[test]
    fn reads_rcon_settings_only_when_enabled() {
        let on = "enable-rcon=true\nrcon.port=25580\nrcon.password=hunter2\n";
        assert_eq!(rcon_settings(on), Some((25580, "hunter2".to_string())));
        let off = "enable-rcon=false\nrcon.port=25580\nrcon.password=hunter2\n";
        assert_eq!(rcon_settings(off), None);
    }

    #[test]
    fn falls_back_to_the_default_rcon_port() {
        let text = "enable-rcon=true\nrcon.password=abc\n";
        assert_eq!(rcon_settings(text), Some((25575, "abc".to_string())));
    }
}
