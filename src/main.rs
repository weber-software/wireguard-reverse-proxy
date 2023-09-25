pub mod virtual_device;
pub mod session;
pub mod wireguard_helper;
pub mod virtual_tcp_socket;

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration, sync::Arc,
};

use boringtun::x25519::StaticSecret;
use hashbrown::HashMap;
use session::Session;
use wireguard_helper::parse_key;

use crate::wireguard_helper::{extract_handshake, print_key};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let bind_ip_port: SocketAddr = "0.0.0.0:51821".parse()?;
    // TODO: don't use that key!
    let private_key: StaticSecret =
        parse_key("sNLSbiLbh1NzkGeoQmeVxy3YJHMlJ+6WdkggInPgN0k=")?.into();

    let internal_address: IpAddr = "192.168.222.11".parse()?;

    let udp = tokio::net::UdpSocket::bind(bind_ip_port).await?;
    let udp = Arc::new(udp);

    let mut poll_wireguard_stack = tokio::time::interval(Duration::from_secs(1));
    let mut poll_internal_stack = tokio::time::interval(Duration::from_millis(10));

    let mut udp_recv_buf = [0; 4096 - 32];

    let mut connections: HashMap<SocketAddr, Session> = HashMap::new();

    loop {
        tokio::select! {
            _ = poll_wireguard_stack.tick() => {
                for (_remote, session) in &mut connections {
                    session.process_wireguard_timer().await;
                    session.send_udp().await;
                }
            }

            _ = poll_internal_stack.tick() => {
                for (_remote, session) in &mut connections {
                    session.poll_stack();
                    session.send_udp().await;

                    session.handle_tcp().unwrap();
                }
            }

            ret = udp.recv_from(&mut udp_recv_buf) => {
                let (size, remote) = ret?;
                let buf : &[u8] = &udp_recv_buf[0..size];

                match connections.entry(remote) {
                    hashbrown::hash_map::Entry::Occupied(entry) => {
                        let session = entry.into_mut();
                        session.process_wireguard(buf, &udp, remote).await;
                        session.send_udp().await;
                    },
                    hashbrown::hash_map::Entry::Vacant(entry) => {
                        if let Some(handshake) = extract_handshake(&private_key, buf) {
                            println!("new session...");
                            print_key(handshake.peer_static_public);
                            // TODO: check if we want to handle that peer...

                            let mut session = Session::new(&private_key, handshake.peer_static_public.into(), internal_address, udp.clone(), remote)?;

                            session.process_wireguard(buf, &udp, remote).await;
                            session.send_udp().await;

                            entry.insert(session);
                         };
                    },
                }
            }
        };
    }
}
