use std::collections::LinkedList;

use smoltcp::{phy::{RxToken, TxToken, Device, Checksum}, time::Instant};

pub struct VirtualDevice {
    packets_received: LinkedList<Vec<u8>>,
    packets_to_send: LinkedList<Vec<u8>>,
}

impl VirtualDevice {
    pub fn new() -> Self {
        VirtualDevice {
            packets_received: LinkedList::new(),
            packets_to_send: LinkedList::new(),
        }
    }

    pub fn add_received(&mut self, packet: &[u8]) {
        // TODO: don't copy
        self.packets_received.push_back(packet.into());
    }

    pub fn get_for_sending(&mut self) -> Option<Vec<u8>> {
        self.packets_to_send.pop_front()
    }
}

pub struct PreReceivedRxToken {
    buffer: Vec<u8>,
}

impl PreReceivedRxToken {
    pub fn new(buffer: Vec<u8>) -> Self {
        PreReceivedRxToken { buffer }
    }
}

impl RxToken for PreReceivedRxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.buffer)
    }
}

impl<'a> TxToken for &'a mut VirtualDevice {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf: Vec<u8> = vec![0; len];
        let result = f(&mut buf[..]);
        self.packets_to_send.push_back(buf);
        return result;
    }
}

impl Device for VirtualDevice {
    type RxToken<'a> = PreReceivedRxToken
    where
        Self: 'a;

    type TxToken<'a> = &'a mut VirtualDevice
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let Some(packet) = self.packets_received.pop_front() else { return None };
        return Some((PreReceivedRxToken::new(packet), self));
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        return Some(self);
    }

    fn capabilities(&self) -> smoltcp::phy::DeviceCapabilities {
        let mtu = 1400;
        let mut caps = smoltcp::phy::DeviceCapabilities::default();
        caps.medium = smoltcp::phy::Medium::Ip;
        caps.max_transmission_unit = mtu;
        caps.checksum = smoltcp::phy::ChecksumCapabilities::ignored();
        caps.checksum.tcp = Checksum::Tx;
        caps.checksum.ipv4 = Checksum::Tx;
        caps.checksum.icmpv4 = Checksum::Tx;
        caps.checksum.icmpv6 = Checksum::Tx;
        return caps;
    }
}