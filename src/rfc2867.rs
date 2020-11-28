// Code generated by machine generator; DO NOT EDIT.

use crate::avp::{AVPType, AVP};
use crate::packet::Packet;

pub type AcctStatusType = u32;
pub const ACCT_STATUS_TYPE_TUNNEL_START: AcctStatusType = 9;
pub const ACCT_STATUS_TYPE_TUNNEL_STOP: AcctStatusType = 10;
pub const ACCT_STATUS_TYPE_TUNNEL_REJECT: AcctStatusType = 11;
pub const ACCT_STATUS_TYPE_TUNNEL_LINK_START: AcctStatusType = 12;
pub const ACCT_STATUS_TYPE_TUNNEL_LINK_STOP: AcctStatusType = 13;
pub const ACCT_STATUS_TYPE_TUNNEL_LINK_REJECT: AcctStatusType = 14;

pub const ACCT_TUNNEL_CONNECTION_TYPE: AVPType = 68;
pub fn delete_acct_tunnel_connection(packet: &mut Packet) {
    packet.delete(ACCT_TUNNEL_CONNECTION_TYPE);
}
pub fn lookup_acct_tunnel_connection(packet: &Packet) -> Option<&AVP> {
    packet.lookup(ACCT_TUNNEL_CONNECTION_TYPE)
}
pub fn lookup_all_acct_tunnel_connection(packet: &Packet) -> Vec<&AVP> {
    packet.lookup_all(ACCT_TUNNEL_CONNECTION_TYPE)
}
pub fn add_acct_tunnel_connection(packet: &mut Packet, value: &str) {
    packet.add(AVP::from_string(ACCT_TUNNEL_CONNECTION_TYPE, value));
}

pub const ACCT_TUNNEL_PACKETS_LOST_TYPE: AVPType = 86;
pub fn delete_acct_tunnel_packets_lost(packet: &mut Packet) {
    packet.delete(ACCT_TUNNEL_PACKETS_LOST_TYPE);
}
pub fn lookup_acct_tunnel_packets_lost(packet: &Packet) -> Option<&AVP> {
    packet.lookup(ACCT_TUNNEL_PACKETS_LOST_TYPE)
}
pub fn lookup_all_acct_tunnel_packets_lost(packet: &Packet) -> Vec<&AVP> {
    packet.lookup_all(ACCT_TUNNEL_PACKETS_LOST_TYPE)
}
pub fn add_acct_tunnel_packets_lost(packet: &mut Packet, value: u32) {
    packet.add(AVP::from_u32(ACCT_TUNNEL_PACKETS_LOST_TYPE, value));
}