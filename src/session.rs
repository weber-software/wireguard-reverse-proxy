use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

use boringtun::{
    noise::Tunn,
    x25519::{PublicKey, StaticSecret},
};
use smoltcp::{
    iface::{Config, Interface, SocketHandle, SocketSet, SocketStorage},
    socket::tcp::{self, Socket},
    time::Instant,
    wire::{HardwareAddress, IpCidr, IpEndpoint},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    select, spawn,
};

use crate::{
    virtual_device::VirtualDevice,
    virtual_tcp_socket::{VirtualTcpSocket, VirtualTcpSocketSyncSide},
};

pub struct Session<'a> {
    tunn: Tunn,
    wg_buffer: [u8; 4096],

    interface: Interface,
    device: VirtualDevice,

    sockets: SocketSet<'a>,

    tcp_socket: SocketHandle,

    virtual_tcp_socket_sync: VirtualTcpSocketSyncSide,

    udp: Arc<UdpSocket>,
    peer_address: SocketAddr,
}

fn create_tcp_socket<'a>() -> Socket<'a> {
    let buffer_size = 65535;

    let rx_buffer = tcp::SocketBuffer::new(vec![0; buffer_size]);
    let tx_buffer = tcp::SocketBuffer::new(vec![0; buffer_size]);

    return tcp::Socket::new(rx_buffer, tx_buffer);
}

impl Session<'_> {
    pub fn new(
        private_key: &StaticSecret,
        peer_static_public: PublicKey,
        internal_address: IpAddr,
        udp: Arc<UdpSocket>,
        peer_address: SocketAddr,
    ) -> anyhow::Result<Self> {
        let wg_buffer: [u8; 4096] = [0; 4096];
        let persistent_keepalive = Some(25);

        let tunn: Tunn = boringtun::noise::Tunn::new(
            private_key.clone(),
            peer_static_public,
            None,
            persistent_keepalive,
            0,
            None,
        )
        .map_err(|e| anyhow::anyhow!(e))?;

        let mut sockets = SocketSet::new([SocketStorage::EMPTY; 2]);

        let mut tcp_socket = create_tcp_socket();
        tcp_socket
            .listen(IpEndpoint::new(internal_address.into(), 80))
            .map_err(|e| anyhow::anyhow!(e))?;

        // tcp_socket.state()

        let tcp_socket = sockets.add(tcp_socket);

        let mut device = VirtualDevice::new();
        let mut interface = Interface::new(
            Config::new(HardwareAddress::Ip),
            &mut device,
            Instant::now(),
        );

        interface.update_ip_addrs(|addresses| {
            let _ = addresses.push(IpCidr::new(internal_address.into(), 0));
        });

        let (virtual_tcp_socket_sync, mut virtual_tcp_socket_async) = VirtualTcpSocket::new();

        spawn(async move {
            // virtual_tcp_socket_async

            // TODO: maybe it could be a good idea to split sending+receving in two different tasks
            // that way one full buffer could not block the other one

            let target_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();

            println!("connecting");

            let Ok(mut tcp_stream) = TcpStream::connect(target_addr).await else {
                // virtual_tcp_socket_async.close();
                println!("failed to connect to tcp end...");

                virtual_tcp_socket_async.close().await;
                return;
            };

            let mut buf_a: [u8; 4096] = [0; 4096];
            let mut buf_b: [u8; 4096] = [0; 4096];

            loop {
                select! {
                    r = virtual_tcp_socket_async.read(& mut buf_a) => {
                        match r {
                            Ok(size) => {
                                println!("received {} from virtual", size);
                                match tcp_stream.write_all(& mut buf_a[..size]).await {
                                    Ok(_) => {
                                        println!("written to physical...");
                                    },
                                    Err(_) => todo!(),
                                }
                            },
                            Err(_) => todo!(),
                        }
                    },
                    r = tcp_stream.read(&mut buf_b) => {
                        match r {
                            Ok(0) => {
                                println!("closed by real tcp");
                                break;
                            }
                            Ok(size) => {
                                println!("received {} from real", size);
                                match virtual_tcp_socket_async.write_all(& mut buf_b[0..size]).await {
                                    Ok(_) => {
                                        println!("written to virtual...");
                                    },
                                    Err(_) => todo!(),
                                }
                            },
                            Err(_) => todo!(),
                        }
                    }
                }
            }
        });

        Ok(Session {
            tunn,
            wg_buffer,
            interface,
            device,
            sockets,

            tcp_socket,

            virtual_tcp_socket_sync,

            udp,
            peer_address,
        })
    }

    pub async fn process_wireguard(
        &mut self,
        buf: &[u8],
        udp: &tokio::net::UdpSocket,
        peer_address: SocketAddr,
    ) {
        let mut buf = buf;
        loop {
            match self.tunn.decapsulate(None, buf, &mut self.wg_buffer) {
                boringtun::noise::TunnResult::Done => {
                    return;
                }
                boringtun::noise::TunnResult::Err(e) => {
                    print!("wireguard error: {:?}", e)
                }
                boringtun::noise::TunnResult::WriteToNetwork(buf) => {
                    match udp.send_to(buf, peer_address).await {
                        Ok(_) => {}
                        Err(e) => {
                            println!("failed to send packet to peer: ${:?}", e);
                        }
                    }
                }

                boringtun::noise::TunnResult::WriteToTunnelV4(buf, _) => {
                    self.device.add_received(buf);
                    self.poll_stack();
                }
                boringtun::noise::TunnResult::WriteToTunnelV6(_, _) => todo!(),
            }

            buf = b"";
        }
    }

    pub async fn process_wireguard_timer(&mut self) {
        match self.tunn.update_timers(&mut self.wg_buffer) {
            boringtun::noise::TunnResult::Done => {
                return;
            }
            boringtun::noise::TunnResult::Err(e) => {
                print!("wireguard error: {:?}", e)
            }
            boringtun::noise::TunnResult::WriteToNetwork(buf) => {
                match self.udp.send_to(buf, self.peer_address).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("failed to send packet to peer: ${:?}", e);
                    }
                }
            }

            boringtun::noise::TunnResult::WriteToTunnelV4(_, _) => unreachable!(),
            boringtun::noise::TunnResult::WriteToTunnelV6(_, _) => unreachable!(),
        }

        loop {
            match self.tunn.decapsulate(None, b"", &mut self.wg_buffer) {
                boringtun::noise::TunnResult::Done => {
                    return;
                }
                boringtun::noise::TunnResult::Err(e) => {
                    print!("wireguard error: {:?}", e)
                }
                boringtun::noise::TunnResult::WriteToNetwork(buf) => {
                    match self.udp.send_to(buf, self.peer_address).await {
                        Ok(_) => {}
                        Err(e) => {
                            println!("failed to send packet to peer: ${:?}", e);
                        }
                    }
                }

                boringtun::noise::TunnResult::WriteToTunnelV4(buf, _) => {
                    self.device.add_received(buf);
                    self.poll_stack();
                }
                boringtun::noise::TunnResult::WriteToTunnelV6(_, _) => todo!(),
            }
        }
    }

    pub async fn send_udp(&mut self) {
        loop {
            let Some(packet) = self.device.get_for_sending() else {
                break;
            };
            match self.tunn.encapsulate(&packet, &mut self.wg_buffer) {
                boringtun::noise::TunnResult::Done => return,
                boringtun::noise::TunnResult::Err(e) => {
                    print!("wireguard error: {:?}", e)
                }
                boringtun::noise::TunnResult::WriteToNetwork(buf) => {
                    match self.udp.send_to(buf, self.peer_address).await {
                        Ok(_) => {}
                        Err(e) => {
                            println!("failed to send packet to peer: ${:?}", e);
                        }
                    }
                }

                boringtun::noise::TunnResult::WriteToTunnelV4(_, _) => todo!(),
                boringtun::noise::TunnResult::WriteToTunnelV6(_, _) => todo!(),
            }
        }
    }

    pub fn poll_stack(&mut self) {
        self.interface
            .poll(Instant::now(), &mut self.device, &mut self.sockets);
    }

    pub fn handle_tcp(&mut self) -> anyhow::Result<()> {
        let tcp_socket = self.sockets.get_mut::<tcp::Socket>(self.tcp_socket);

        self.virtual_tcp_socket_sync.process(tcp_socket);

        return Ok(());
    }
}
