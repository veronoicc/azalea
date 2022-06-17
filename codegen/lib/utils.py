import re

# utilities that could be used for things other than codegen


def to_snake_case(name: str):
    s = re.sub('([A-Z])', r'_\1', name)
    return s.lower().strip('_')


def to_camel_case(name: str):
    s = re.sub('_([a-z])', lambda m: m.group(1).upper(), name)
    return s[0].upper() + s[1:]


def padded_hex(n: int):
    return f'0x{n:02x}'


class PacketIdentifier:
    def __init__(self, packet_id: int, direction: str, state: str):
        self.packet_id = packet_id
        self.direction = direction
        self.state = state

    def __eq__(self, other):
        return self.packet_id == other.packet_id and self.direction == other.direction and self.state == other.state

    def __hash__(self):
        return hash((self.packet_id, self.direction, self.state))

    def __str__(self):
        return f'{self.packet_id} {self.direction} {self.state}'

    def __repr__(self):
        return f'PacketIdentifier({self.packet_id}, {self.direction}, {self.state})'


def group_packets(packets: list[PacketIdentifier]):
    packet_groups: dict[tuple[str, str], list[int]] = {}
    for packet in packets:
        key = (packet.direction, packet.state)
        if key not in packet_groups:
            packet_groups[key] = []
        packet_groups[key].append(packet.packet_id)
    return packet_groups