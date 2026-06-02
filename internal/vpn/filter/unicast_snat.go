package filter

import (
	"crypto/rand"
	"encoding/binary"
	"math/big"
	"net/netip"
	"sync"
	"time"
)

const (
	uniSNATPortMin = 20000
	uniSNATPortMax = 65000
	uniSNATIdle    = 2 * time.Minute
	protoTCP       = 6
	protoUDP       = 17
)

type flowKey struct {
	client     netip.Addr
	server     netip.Addr
	clientPort uint16
	serverPort uint16
	proto      uint8
}

type uniSNATSession struct {
	key      flowKey
	hubPort  uint16
	lastSeen time.Time
}

// UniSNATTable performs ephemeral hub SNAT for unidirectional group links.
type UniSNATTable struct {
	hubIP    netip.Addr
	mu       sync.Mutex
	peerGrp  map[netip.Addr]uint
	uniLinks map[uint]map[uint]struct{} // fromGroup -> toGroups
	byFlow   map[flowKey]*uniSNATSession
	byPort   map[uint16]*uniSNATSession // hub ephemeral port
}

func NewUniSNATTable() *UniSNATTable {
	return &UniSNATTable{
		peerGrp:  make(map[netip.Addr]uint),
		uniLinks: make(map[uint]map[uint]struct{}),
		byFlow:   make(map[flowKey]*uniSNATSession),
		byPort:   make(map[uint16]*uniSNATSession),
	}
}

func (t *UniSNATTable) SetHubIP(addr netip.Addr) {
	t.mu.Lock()
	t.hubIP = addr
	t.mu.Unlock()
}

func (t *UniSNATTable) RegisterPeer(ip netip.Addr, groupID uint) {
	t.mu.Lock()
	t.peerGrp[ip] = groupID
	t.mu.Unlock()
}

func (t *UniSNATTable) RegisterUniLink(fromGroup, toGroup uint) {
	t.mu.Lock()
	if t.uniLinks[fromGroup] == nil {
		t.uniLinks[fromGroup] = make(map[uint]struct{})
	}
	t.uniLinks[fromGroup][toGroup] = struct{}{}
	t.mu.Unlock()
}

func (t *UniSNATTable) Reset() {
	t.mu.Lock()
	t.byFlow = make(map[flowKey]*uniSNATSession)
	t.byPort = make(map[uint16]*uniSNATSession)
	t.mu.Unlock()
}

func (t *UniSNATTable) needsSNAT(src, dst netip.Addr) bool {
	sg, ok1 := t.peerGrp[src]
	dg, ok2 := t.peerGrp[dst]
	if !ok1 || !ok2 || sg == dg {
		return false
	}
	to, ok := t.uniLinks[sg]
	if !ok {
		return false
	}
	_, ok = to[dg]
	return ok
}

// ProcessIngressFromWG handles packets WireGuard injects into the hub netstack (TUN Write).
// Rewrites return traffic destined to hub:ephemeral back to server → client.
func (t *UniSNATTable) ProcessIngressFromWG(packet []byte) bool {
	if t.hubIP == (netip.Addr{}) {
		return false
	}
	src, dst, proto, sport, dport, ok := parseIPv4Transport(packet)
	if !ok || dst != t.hubIP {
		return false
	}

	t.mu.Lock()
	defer t.mu.Unlock()

	if sess := t.byPort[dport]; sess != nil {
		sess.lastSeen = time.Now()
		rewriteEndpoints(packet, src, sess.key.client, sport, sess.key.clientPort)
		fixIPv4Checksum(packet)
		fixTransportChecksum(packet, proto)
		return true
	}
	return false
}

// ProcessEgressToWG handles packets the hub netstack forwards out to peers (TUN Read).
// SNATs allowed unidirectional flows to hub:ephemeral → server.
func (t *UniSNATTable) ProcessEgressToWG(packet []byte) bool {
	if t.hubIP == (netip.Addr{}) {
		return false
	}
	src, dst, proto, sport, dport, ok := parseIPv4Transport(packet)
	if !ok || !t.needsSNAT(src, dst) {
		return false
	}

	t.mu.Lock()
	defer t.mu.Unlock()

	key := flowKey{client: src, server: dst, clientPort: sport, serverPort: dport, proto: proto}
	sess := t.byFlow[key]
	if sess == nil {
		port, err := t.pickPortLocked()
		if err != nil {
			return false
		}
		sess = &uniSNATSession{key: key, hubPort: port, lastSeen: time.Now()}
		t.byFlow[key] = sess
		t.byPort[port] = sess
	}
	sess.lastSeen = time.Now()
	rewriteEndpoints(packet, t.hubIP, dst, sess.hubPort, dport)
	fixIPv4Checksum(packet)
	fixTransportChecksum(packet, proto)
	t.gcLocked(time.Now())
	return true
}

func (t *UniSNATTable) pickPortLocked() (uint16, error) {
	span := int64(uniSNATPortMax - uniSNATPortMin + 1)
	for i := 0; i < 256; i++ {
		n, err := rand.Int(rand.Reader, big.NewInt(span))
		if err != nil {
			return 0, err
		}
		p := uint16(uniSNATPortMin + int(n.Int64()))
		if _, used := t.byPort[p]; !used {
			return p, nil
		}
	}
	return 0, errNoSNATPort
}

var errNoSNATPort = errSNATPortsExhausted{}

type errSNATPortsExhausted struct{}

func (errSNATPortsExhausted) Error() string {
	return "no ephemeral SNAT port available"
}

func (t *UniSNATTable) gcLocked(now time.Time) {
	for k, sess := range t.byFlow {
		if now.Sub(sess.lastSeen) > uniSNATIdle {
			delete(t.byPort, sess.hubPort)
			delete(t.byFlow, k)
		}
	}
}

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
