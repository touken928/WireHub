package l4

import (
	"encoding/binary"
	"net/http"
	"net/netip"
	"testing"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

func buildTCPPacket(src, dst netip.Addr, sport, dport uint16) []byte {
	p := make([]byte, 40)
	p[0] = 0x45
	p[1] = 0
	binary.BigEndian.PutUint16(p[2:4], uint16(len(p)))
	p[9] = protoTCP
	s4 := src.As4()
	d4 := dst.As4()
	copy(p[12:16], s4[:])
	copy(p[16:20], d4[:])
	binary.BigEndian.PutUint16(p[20:22], sport)
	binary.BigEndian.PutUint16(p[22:24], dport)
	fixIPv4Checksum(p)
	fixTransportChecksum(p, protoTCP)
	return p
}

func buildUDPPacket(src, dst netip.Addr, sport, dport uint16) []byte {
	p := make([]byte, 28)
	p[0] = 0x45
	p[1] = 0
	binary.BigEndian.PutUint16(p[2:4], uint16(len(p)))
	p[9] = protoUDP
	s4 := src.As4()
	d4 := dst.As4()
	copy(p[12:16], s4[:])
	copy(p[16:20], d4[:])
	binary.BigEndian.PutUint16(p[20:22], sport)
	binary.BigEndian.PutUint16(p[22:24], dport)
	fixIPv4Checksum(p)
	fixTransportChecksum(p, protoUDP)
	return p
}

func packetAddrs(packet []byte) (src, dst netip.Addr) {
	return netip.AddrFrom4([4]byte{packet[12], packet[13], packet[14], packet[15]}),
		netip.AddrFrom4([4]byte{packet[16], packet[17], packet[18], packet[19]})
}

func packetSport(packet []byte) uint16 {
	ihl := int(packet[0]&0x0f) * 4
	return binary.BigEndian.Uint16(packet[ihl : ihl+2])
}

func newRelayTable(t *testing.T, hub netip.Addr, peers map[netip.Addr]uint, uni [][2]uint) *TransparentTable {
	t.Helper()
	tbl := NewTransparentTable()
	tbl.SetHubIP(hub)
	for ip, gid := range peers {
		tbl.RegisterPeer(ip, gid)
	}
	for _, link := range uni {
		tbl.RegisterUniLink(link[0], link[1])
	}
	return tbl
}

func newTestNetstack(t *testing.T, addrs ...netip.Addr) (*netstack.Net, func()) {
	t.Helper()
	if len(addrs) == 0 {
		t.Fatal("at least one address required")
	}
	tun, tnet, err := netstack.CreateNetTUN(addrs, []netip.Addr{addrs[0]}, 1420)
	if err != nil {
		t.Fatal(err)
	}
	return tnet, func() { _ = tun.Close() }
}

func testHTTPClient(tnet *netstack.Net) *http.Client {
	return &http.Client{
		Transport: &http.Transport{DialContext: tnet.DialContext},
		Timeout:   3 * time.Second,
	}
}

type staticHostResolver struct {
	hosts map[string]netip.Addr
}

func (r staticHostResolver) ResolveHost(host string) (netip.Addr, error) {
	if addr, ok := r.hosts[host]; ok {
		return addr, nil
	}
	return netip.Addr{}, errUnknownHost{host: host}
}

func (r staticHostResolver) ResolveForwardAddrs(host string) ([]netip.Addr, error) {
	addr, err := r.ResolveHost(host)
	if err != nil {
		return nil, err
	}
	return []netip.Addr{addr}, nil
}

type errUnknownHost struct{ host string }

func (e errUnknownHost) Error() string {
	return "unknown host: " + e.host
}
