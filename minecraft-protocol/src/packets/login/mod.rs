pub mod clientbound_custom_query_packet;
pub mod clientbound_hello_packet;
pub mod serverbound_hello_packet;

use async_trait::async_trait;
use tokio::io::BufReader;

use crate::connect::PacketFlow;

use super::ProtocolPacket;

#[derive(Clone, Debug)]
pub enum LoginPacket
where
    Self: Sized,
{
    ClientboundCustomQueryPacket(clientbound_custom_query_packet::ClientboundCustomQueryPacket),
    ServerboundHelloPacket(serverbound_hello_packet::ServerboundHelloPacket),
    ClientboundHelloPacket(clientbound_hello_packet::ClientboundHelloPacket),
}

#[async_trait]
impl ProtocolPacket for LoginPacket {
    fn id(&self) -> u32 {
        match self {
            LoginPacket::ClientboundCustomQueryPacket(_packet) => 0x04,
            LoginPacket::ServerboundHelloPacket(_packet) => 0x00,
            LoginPacket::ClientboundHelloPacket(_packet) => 0x01,
        }
    }

    fn write(&self, buf: &mut Vec<u8>) {
        match self {
            LoginPacket::ClientboundCustomQueryPacket(packet) => packet.write(buf),
            LoginPacket::ServerboundHelloPacket(packet) => packet.write(buf),
            LoginPacket::ClientboundHelloPacket(packet) => packet.write(buf),
        }
    }

    /// Read a packet by its id, ConnectionProtocol, and flow
    async fn read<T: tokio::io::AsyncRead + std::marker::Unpin + std::marker::Send>(
        id: u32,
        flow: &PacketFlow,
        buf: &mut BufReader<T>,
    ) -> Result<LoginPacket, String>
    where
        Self: Sized,
    {
        Ok(match flow {
            PacketFlow::ServerToClient => match id {
                0x01 => clientbound_hello_packet::ClientboundHelloPacket::read(buf).await?,
                0x04 => {
                    clientbound_custom_query_packet::ClientboundCustomQueryPacket::read(buf).await?
                }
                _ => return Err(format!("Unknown ServerToClient status packet id: {}", id)),
            },
            PacketFlow::ClientToServer => match id {
                0x00 => serverbound_hello_packet::ServerboundHelloPacket::read(buf).await?,
                _ => return Err(format!("Unknown ClientToServer status packet id: {}", id)),
            },
        })
    }
}