package snat

import (
	"net/netip"
	"sync"
	"time"
)

type flowKey struct {
	client     netip.Addr
	server     netip.Addr
	clientPort uint16
	serverPort uint16
	proto      uint8
}

type snatSession struct {
	key      flowKey
	hubPort  uint16
	lastSeen time.Time
}

// TransparentTable performs ephemeral hub SNAT for unidirectional group links.
// See package doc for contrast with ForwardProxy (admin Forward rules).
type TransparentTable struct {
	hubIP    netip.Addr
	mu       sync.Mutex
	peerGrp  map[netip.Addr]uint
	uniLinks map[uint]map[uint]struct{} // fromGroup -> toGroups
	byFlow   map[flowKey]*snatSession
	byPort   map[uint16]*snatSession
	ports    *ephemeralPortPool
}

func NewTransparentTable() *TransparentTable {
	return &TransparentTable{
		peerGrp:  make(map[netip.Addr]uint),
		uniLinks: make(map[uint]map[uint]struct{}),
		byFlow:   make(map[flowKey]*snatSession),
		byPort:   make(map[uint16]*snatSession),
		ports:    newEphemeralPortPool(),
	}
}

func (t *TransparentTable) SetHubIP(addr netip.Addr) {
	t.mu.Lock()
	t.hubIP = addr
	t.mu.Unlock()
}

func (t *TransparentTable) RegisterPeer(ip netip.Addr, groupID uint) {
	t.mu.Lock()
	t.peerGrp[ip] = groupID
	t.mu.Unlock()
}

func (t *TransparentTable) RegisterUniLink(fromGroup, toGroup uint) {
	t.mu.Lock()
	if t.uniLinks[fromGroup] == nil {
		t.uniLinks[fromGroup] = make(map[uint]struct{})
	}
	t.uniLinks[fromGroup][toGroup] = struct{}{}
	t.mu.Unlock()
}

// ReserveHubPorts marks hub listen ports (system + forward) so SNAT will not pick them.
func (t *TransparentTable) ReserveHubPorts(ports []int) {
	t.mu.Lock()
	t.ports.reserveHubPorts(ports)
	t.mu.Unlock()
}

func (t *TransparentTable) Reset() {
	t.mu.Lock()
	t.byFlow = make(map[flowKey]*snatSession)
	t.byPort = make(map[uint16]*snatSession)
	t.ports.resetInUse()
	t.mu.Unlock()
}

func (t *TransparentTable) needsSNAT(src, dst netip.Addr) bool {
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
func (t *TransparentTable) ProcessIngressFromWG(packet []byte) bool {
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
func (t *TransparentTable) ProcessEgressToWG(packet []byte) bool {
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
		port, err := t.ports.pick()
		if err != nil {
			return false
		}
		sess = &snatSession{key: key, hubPort: port, lastSeen: time.Now()}
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

func (t *TransparentTable) gcLocked(now time.Time) {
	for k, sess := range t.byFlow {
		if now.Sub(sess.lastSeen) > SessionIdle {
			delete(t.byPort, sess.hubPort)
			delete(t.byFlow, k)
			delete(t.ports.inUse, sess.hubPort)
		}
	}
}
