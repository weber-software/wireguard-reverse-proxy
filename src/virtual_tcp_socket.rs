use std::io::{self, Error};

use smoltcp::socket::tcp::{Socket, SendError};

pub struct VirtualTcpSocket {}

pub struct VirtualTcpSocketSyncSide {
    reciever: tokio::sync::mpsc::Receiver<Vec<u8>>,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,

    //close_virtual_sender: tokio::sync::oneshot::Sender<()>,
}

pub struct VirtualTcpSocketAsyncSide {
    reciever: tokio::sync::mpsc::Receiver<Vec<u8>>,
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,

    // close_virtual_receiver: tokio::sync::oneshot::Sender<()>,
}

impl VirtualTcpSocket {
    pub fn new() -> (VirtualTcpSocketSyncSide, VirtualTcpSocketAsyncSide) {
        let (a_sender, a_receiver) = tokio::sync::mpsc::channel(8);
        let (b_sender, b_receiver) = tokio::sync::mpsc::channel(8);

        //let (close_virtual_sender, close_virtual_receiver) = tokio::sync::oneshot::channel();

        (
            VirtualTcpSocketSyncSide {
                reciever: b_receiver,
                sender: a_sender,
            },
            VirtualTcpSocketAsyncSide {
                reciever: a_receiver,
                sender: b_sender,
            },
        )
    }
}

impl VirtualTcpSocketAsyncSide {
    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.reciever.recv().await {
            Some(received) => {
                if buf.len() < received.len() {
                    return Err(Error::new(io::ErrorKind::Other, "buffer is too small"))
                }

                buf[0..received.len()].clone_from_slice(&received);

                return Ok(received.len());
            },
            None => return Err(Error::new(io::ErrorKind::BrokenPipe, "the connection was closed")),
        }
    }

    pub async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Err(e) = self.sender.send(buf.to_vec()).await {
            return Err(Error::new(io::ErrorKind::Other, e))
        }

        Ok(buf.len())
    }

    pub async fn write_all(&mut self, buf: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        loop {
            if done >= buf.len() {
                return Ok(());
            }
            match self.write(&mut buf[done..]).await {
                Ok(size) => {
                    done += size;
                },
                Err(e) => return Err(e),
            }
        }

    }
}

impl VirtualTcpSocketSyncSide {
    pub fn process(&mut self, socket: &mut Socket<'_>) {
        if socket.can_recv() {
            // let receive_amount = socket.recv_capacity() - socket.recv_queue();

            let mut buf: Vec<u8> = vec![0; 4096];

            match socket.recv_slice(&mut buf[..]) {
                Ok(received) => {
                    // TODO: can be optimized
                    match self.sender.try_send(buf[..received].into()) {
                        Ok(_) => {

                        },
                        Err(e) => {
                            match e {
                                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                                    // maybe we should use peek_slice and check weather we have enough
                                    todo!()
                                },
                                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                                    socket.close();
                                },
                            }
                        },
                    }
                }
                Err(_) => todo!(),
            }
        }

        if socket.can_send() {
            let send_space = socket.send_capacity() - socket.send_queue();

            if send_space > 16 * 1024 {
                // for now we only send stuff if we have enough space
                // so that we have too less for edge cases

                match self.reciever.try_recv() {
                    Ok(buffer) => {
                        match socket.send_slice(&buffer[..]) {
                            Ok(sent) => {
                                assert!(sent == buffer.len())                                
                            },
                            Err(e) if e == SendError::InvalidState => todo!(),
                            Err(_) => todo!()
                        }; },
                    Err(e) if e == tokio::sync::mpsc::error::TryRecvError::Empty => {  },
                    Err(e) if e == tokio::sync::mpsc::error::TryRecvError::Disconnected => {  },
                    Err(_) => todo!()
                };
            }
        }
    }
}
