package snat

import (
	"encoding/binary"
	"net/netip"
)

const (
	protoTCP = 6
	protoUDP = 17
)

func parseIPv4Transport(packet []byte) (src, dst netip.Addr, proto uint8, sport, dport uint16, ok bool) {
	if len(packet) < 20 || packet[0]>>4 != 4 {
		return
	}
	ihl := int(packet[0]&0x0f) * 4
	if len(packet) < ihl {
		return
	}
	proto = packet[9]
	src = netip.AddrFrom4([4]byte{packet[12], packet[13], packet[14], packet[15]})
	dst = netip.AddrFrom4([4]byte{packet[16], packet[17], packet[18], packet[19]})
	switch proto {
	case protoTCP:
		if len(packet) < ihl+14 {
			return
		}
		off := ihl
		sport = binary.BigEndian.Uint16(packet[off : off+2])
		dport = binary.BigEndian.Uint16(packet[off+2 : off+4])
		ok = true
	case protoUDP:
		if len(packet) < ihl+8 {
			return
		}
		off := ihl
		sport = binary.BigEndian.Uint16(packet[off : off+2])
		dport = binary.BigEndian.Uint16(packet[off+2 : off+4])
		ok = true
	default:
		ok = false
	}
	return
}

func rewriteEndpoints(packet []byte, src, dst netip.Addr, sport, dport uint16) {
	src4 := src.As4()
	dst4 := dst.As4()
	copy(packet[12:16], src4[:])
	copy(packet[16:20], dst4[:])
	ihl := int(packet[0]&0x0f) * 4
	switch packet[9] {
	case protoTCP, protoUDP:
		binary.BigEndian.PutUint16(packet[ihl:ihl+2], sport)
		binary.BigEndian.PutUint16(packet[ihl+2:ihl+4], dport)
	}
}

func fixIPv4Checksum(packet []byte) {
	ihl := int(packet[0]&0x0f) * 4
	packet[10], packet[11] = 0, 0
	sum := ipChecksum(packet[:ihl])
	binary.BigEndian.PutUint16(packet[10:12], sum)
}

func fixTransportChecksum(packet []byte, proto uint8) {
	ihl := int(packet[0]&0x0f) * 4
	totalLen := int(binary.BigEndian.Uint16(packet[2:4]))
	payloadLen := totalLen - ihl
	if payloadLen <= 0 {
		return
	}
	src := netip.AddrFrom4([4]byte{packet[12], packet[13], packet[14], packet[15]})
	dst := netip.AddrFrom4([4]byte{packet[16], packet[17], packet[18], packet[19]})
	switch proto {
	case protoUDP:
		if payloadLen < 8 {
			return
		}
		packet[ihl+6], packet[ihl+7] = 0, 0
		sum := pseudoChecksum(src, dst, protoUDP, packet[ihl:ihl+payloadLen])
		binary.BigEndian.PutUint16(packet[ihl+6:ihl+8], sum)
	case protoTCP:
		if payloadLen < 20 {
			return
		}
		packet[ihl+16], packet[ihl+17] = 0, 0
		sum := pseudoChecksum(src, dst, protoTCP, packet[ihl:ihl+payloadLen])
		binary.BigEndian.PutUint16(packet[ihl+16:ihl+18], sum)
	}
}

func ipChecksum(b []byte) uint16 {
	var sum uint32
	for i := 0; i+1 < len(b); i += 2 {
		sum += uint32(binary.BigEndian.Uint16(b[i : i+2]))
	}
	if len(b)%2 == 1 {
		sum += uint32(b[len(b)-1]) << 8
	}
	for (sum >> 16) > 0 {
		sum = (sum & 0xffff) + (sum >> 16)
	}
	return ^uint16(sum)
}

func pseudoChecksum(src, dst netip.Addr, proto uint8, segment []byte) uint16 {
	var buf [12]byte
	copy(buf[0:4], src.AsSlice())
	copy(buf[4:8], dst.AsSlice())
	buf[9] = proto
	binary.BigEndian.PutUint16(buf[10:12], uint16(len(segment)))
	pseudo := append(buf[:], segment...)
	return ipChecksum(pseudo)
}
