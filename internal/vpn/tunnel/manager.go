package tunnel

import (
	"fmt"
	"net/netip"
	"strings"
	"sync"
	"time"

	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/ingress"
	vpnnetstack "github.com/touken928/wirehub/internal/vpn/netstack"
	vpntun "github.com/touken928/wirehub/internal/vpn/tun"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	wgtun "golang.zx2c4.com/wireguard/tun"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

type PeerStats struct {
	PublicKey     string
	LastHandshake time.Time
	RxBytes       int64
	TxBytes       int64
}

type Manager struct {
	mu         sync.RWMutex
	closeOnce  sync.Once
	dev        *device.Device
	tnet       *netstack.Net
	tunDev     wgtun.Device
	filterTUN  *vpntun.TUN
	hubIP      netip.Addr
	listenPort int
}

func NewManager(hubIP string, dnsIP string, mapVIPAddrs []netip.Addr, listenPort int, mtu int) (*Manager, error) {
	hub, err := netip.ParseAddr(hubIP)
	if err != nil {
		return nil, fmt.Errorf("parse hub ip: %w", err)
	}
	dns, err := netip.ParseAddr(dnsIP)
	if err != nil {
		return nil, fmt.Errorf("parse dns ip: %w", err)
	}

	local := uniqueAddrs(append([]netip.Addr{hub, dns}, mapVIPAddrs...))
	rawTUN, tnet, err := netstack.CreateNetTUN(local, []netip.Addr{dns}, mtu)
	if err != nil {
		return nil, fmt.Errorf("create netstack tun: %w", err)
	}

	safeTUN := &onceTUN{Device: rawTUN}
	filterTUN := vpntun.NewTUN(safeTUN, hub)
	dev := device.NewDevice(filterTUN, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))

	return &Manager{
		dev:        dev,
		tnet:       tnet,
		tunDev:     safeTUN,
		filterTUN:  filterTUN,
		hubIP:      hub,
		listenPort: listenPort,
	}, nil
}

func (m *Manager) SetAccessRules(rules *vpntun.RuleSet) {
	m.filterTUN.SetAccessRules(rules)
}

func (m *Manager) SetAccessPolicy(p *vpntun.AccessPolicy) {
	m.filterTUN.SetAccessPolicy(p)
}

func (m *Manager) ReserveHubPorts(ports []int) {
	m.filterTUN.ReserveHubPorts(ports)
}

func (m *Manager) Net() *netstack.Net {
	return m.tnet
}

// EnsureMapVIPs registers map virtual IPs on the hub netstack (matches CreateNetTUN local addrs in production).
func (m *Manager) EnsureMapVIPs(addrs []netip.Addr) error {
	if len(addrs) == 0 {
		return nil
	}
	stk, err := vpnnetstack.StackFromNet(m.tnet)
	if err != nil {
		return err
	}
	return ingress.EnsureStackMapAddresses(stk, addrs)
}

// SetMapVIPs updates TUN bypass for map virtual IPs (ACL enforced in MapProxy).
func (m *Manager) SetMapVIPs(addrs []netip.Addr) {
	m.filterTUN.SetMapVIPs(addrs)
}

func (m *Manager) ConfigureServer(privateKey string, listenPort int) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.listenPort = listenPort

	privHex, err := keyToHex(privateKey)
	if err != nil {
		return err
	}
	cfg := fmt.Sprintf("private_key=%s\nlisten_port=%d\n", privHex, listenPort)
	return m.dev.IpcSet(cfg)
}

func (m *Manager) Up() error {
	return m.dev.Up()
}

func (m *Manager) Down() error {
	return m.dev.Down()
}

func (m *Manager) Close() error {
	m.closeOnce.Do(func() {
		if m.dev != nil {
			m.dev.Close()
		}
	})
	return nil
}

func (m *Manager) SyncPeer(peer runtime.WGPeer) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if !peer.Enabled {
		return m.removePeerLocked(peer.PublicKey)
	}

	pubHex, err := keyToHex(peer.PublicKey)
	if err != nil {
		return err
	}

	cfg := fmt.Sprintf("public_key=%s\nallowed_ip=%s/32\n", pubHex, peer.WGIP)
	return m.dev.IpcSet(cfg)
}

func (m *Manager) RemovePeer(publicKey string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.removePeerLocked(publicKey)
}

func (m *Manager) removePeerLocked(publicKey string) error {
	pubHex, err := keyToHex(publicKey)
	if err != nil {
		return err
	}
	cfg := fmt.Sprintf("public_key=%s\nremove=true\n", pubHex)
	return m.dev.IpcSet(cfg)
}

func (m *Manager) SyncAll(peers []runtime.WGPeer) error {
	for _, p := range peers {
		if err := m.SyncPeer(p); err != nil {
			return err
		}
	}
	return nil
}

func (m *Manager) GetStats() (map[string]PeerStats, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	status, err := m.dev.IpcGet()
	if err != nil {
		return nil, err
	}
	return parseIPCStatus(status), nil
}

func parseIPCStatus(status string) map[string]PeerStats {
	result := make(map[string]PeerStats)
	lines := strings.Split(status, "\n")
	var currentKey string
	var stats PeerStats

	flush := func() {
		if currentKey != "" {
			result[currentKey] = stats
		}
	}

	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		if k, v, ok := strings.Cut(line, "="); ok {
			switch k {
			case "public_key":
				flush()
				currentKey = v
				if b64, err := hexKeyToBase64(v); err == nil {
					currentKey = b64
				}
				stats = PeerStats{PublicKey: currentKey}
			case "last_handshake_time_sec":
				if sec, err := parseInt64(v); err == nil && sec > 0 {
					stats.LastHandshake = time.Unix(sec, 0)
				}
			case "rx_bytes":
				stats.RxBytes, _ = parseInt64(v)
			case "tx_bytes":
				stats.TxBytes, _ = parseInt64(v)
			}
		}
	}
	flush()
	return result
}

func parseInt64(s string) (int64, error) {
	var n int64
	_, err := fmt.Sscan(s, &n)
	return n, err
}

func (m *Manager) HubIP() netip.Addr {
	return m.hubIP
}

func uniqueAddrs(addrs []netip.Addr) []netip.Addr {
	seen := make(map[netip.Addr]struct{}, len(addrs))
	out := make([]netip.Addr, 0, len(addrs))
	for _, a := range addrs {
		if !a.IsValid() {
			continue
		}
		if _, ok := seen[a]; ok {
			continue
		}
		seen[a] = struct{}{}
		out = append(out, a)
	}
	return out
}
